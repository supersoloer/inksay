use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

#[derive(Deserialize)]
struct InferenceResponse {
    text: String,
    language: Option<String>,
}

#[derive(Deserialize)]
struct VadResponse {
    speech: bool,
    segments: u32,
    speech_ms: f64,
}

pub struct Transcription {
    pub text: String,
    pub detected_language: Option<String>,
}

impl Transcription {
    pub fn output_language(&self, requested: Option<&str>) -> Option<String> {
        requested.map(str::to_owned).or_else(|| {
            self.detected_language
                .as_deref()
                .is_some_and(|language| {
                    matches!(
                        language.to_ascii_lowercase().as_str(),
                        "chinese" | "zh" | "zh-cn" | "zh-tw" | "zh-hk"
                    )
                })
                .then(|| "zh-CN".to_owned())
        })
    }
}

pub enum TranscriptionError {
    Empty,
    Failure(String),
}

pub struct WhisperService {
    child: Option<CommandChild>,
    #[cfg(target_os = "windows")]
    job: Option<crate::win_job::JobHandle>,
    address: Option<String>,
    model: Option<PathBuf>,
    terminated: Option<Arc<AtomicBool>>,
    client: reqwest::Client,
}

impl WhisperService {
    pub fn new() -> Self {
        Self {
            child: None,
            #[cfg(target_os = "windows")]
            job: None,
            address: None,
            model: None,
            terminated: None,
            client: reqwest::Client::new(),
        }
    }

    pub async fn ensure_loaded(
        &mut self,
        app: &tauri::AppHandle,
        model: &Path,
    ) -> Result<(), String> {
        self.ensure_service(app, Some(model)).await
    }

    pub async fn ensure_service(
        &mut self,
        app: &tauri::AppHandle,
        model: Option<&Path>,
    ) -> Result<(), String> {
        if self.model.as_deref() == model
            && !self
                .terminated
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            if let Some(address) = &self.address {
                if self.health(address).await {
                    return Ok(());
                }
            }
        }
        self.stop();
        if model.is_some_and(|path| !path.is_file()) {
            return Err("Whisper model not found".into());
        }
        let port = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| error.to_string())?
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let prefix = format!("/{}", uuid::Uuid::new_v4().simple());
        let vad_model = app
            .path()
            .resolve(
                "resources/ggml-silero-v6.2.0.bin",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|error| error.to_string())?;
        if !vad_model.is_file() {
            return Err("Bundled VAD model is missing".into());
        }
        let vad_arg = vad_model.to_string_lossy().into_owned();
        let model_args = match model {
            Some(path) => vec!["-m".to_owned(), path.to_string_lossy().into_owned()],
            None => vec!["--vad-only".to_owned()],
        };
        let port_arg = port.to_string();
        let (mut events, child) = app
            .shell()
            .sidecar("whisper-server")
            .map_err(|error| error.to_string())?
            .args(model_args)
            .args([
                "--vad-model",
                &vad_arg,
                "--host",
                "127.0.0.1",
                "--port",
                &port_arg,
                "--request-path",
                &prefix,
                "-l",
                "auto",
            ])
            .spawn()
            .map_err(|error| error.to_string())?;
        let child_pid = child.pid();
        crate::diagnostics::log(app, "asr", &format!("server spawned pid={child_pid}"));
        #[cfg(target_os = "windows")]
        let job = match crate::win_job::JobHandle::attach(child.pid()) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                return Err(format!("Whisper process supervision failed: {error}"));
            }
        };
        let terminated = Arc::new(AtomicBool::new(false));
        let terminated_for_events = terminated.clone();
        let event_app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    CommandEvent::Terminated(status) => {
                        crate::diagnostics::log(
                            &event_app,
                            "asr",
                            &format!(
                                "server terminated pid={child_pid} code={:?} signal={:?}",
                                status.code, status.signal
                            ),
                        );
                        terminated_for_events.store(true, Ordering::Relaxed);
                        break;
                    }
                    CommandEvent::Error(error) => {
                        crate::diagnostics::log(
                            &event_app,
                            "asr",
                            &format!("server event error: {error}"),
                        );
                    }
                    _ => {}
                }
            }
        });
        let address = format!("http://127.0.0.1:{port}{prefix}");
        self.child = Some(child);
        #[cfg(target_os = "windows")]
        {
            self.job = Some(job);
        }
        self.address = Some(address.clone());
        self.model = model.map(Path::to_path_buf);
        self.terminated = Some(terminated.clone());
        for _ in 0..300 {
            if terminated.load(Ordering::Relaxed) {
                self.stop();
                return Err("Whisper server stopped during model loading".into());
            }
            if self.health(&address).await {
                crate::diagnostics::log(app, "asr", &format!("server ready pid={child_pid}"));
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.stop();
        Err("Whisper model loading timed out".into())
    }

    pub async fn transcribe(
        &mut self,
        app: &tauri::AppHandle,
        recording: &Path,
        model: &Path,
        language: Option<&str>,
        vocabulary: &[String],
        trace: &crate::diagnostics::SessionTrace,
    ) -> Result<Transcription, TranscriptionError> {
        self.ensure_loaded(app, model)
            .await
            .map_err(TranscriptionError::Failure)?;
        let audio = self.detect_speech(recording, trace).await?;
        let address = self
            .address
            .as_ref()
            .ok_or_else(|| TranscriptionError::Failure("ASR service unavailable".into()))?;
        // Keep all original samples: a detection gate must not clip quiet consonants or short words.
        trace.event("asr", "started");
        let part = Part::bytes(audio).file_name("recording.wav");
        let mut form = Form::new()
            .part("file", part)
            .text(
                "response_format",
                if language.is_some() {
                    "json"
                } else {
                    "verbose_json"
                },
            )
            .text("language", language.unwrap_or("auto").to_owned())
            .text("temperature", "0.0");
        if language.is_none() {
            form = form
                .text("no_language_probabilities", "true")
                .text("no_timestamps", "true");
        }
        if !vocabulary.is_empty() {
            form = form.text("prompt", vocabulary.join("、"));
        }
        let response = self
            .client
            .post(format!("{address}/inference"))
            .multipart(form)
            .timeout(Duration::from_secs(300))
            .send()
            .await
            .map_err(|error| TranscriptionError::Failure(error.to_string()))?
            .error_for_status()
            .map_err(|error| TranscriptionError::Failure(error.to_string()))?;
        let response: InferenceResponse = response
            .json()
            .await
            .map_err(|error| TranscriptionError::Failure(error.to_string()))?;
        let text = response.text.trim().to_owned();
        if text.is_empty() {
            return Err(TranscriptionError::Empty);
        }
        Ok(Transcription {
            text,
            detected_language: response.language,
        })
    }

    pub async fn detect_speech(
        &self,
        recording: &Path,
        trace: &crate::diagnostics::SessionTrace,
    ) -> Result<Vec<u8>, TranscriptionError> {
        let audio = tokio::fs::read(recording)
            .await
            .map_err(|error| TranscriptionError::Failure(error.to_string()))?;
        let address = self
            .address
            .as_ref()
            .ok_or_else(|| TranscriptionError::Failure("Whisper server is not running".into()))?;
        trace.event("vad", "started");
        let vad_started = std::time::Instant::now();
        let detected = self
            .client
            .post(format!("{address}/vad"))
            .multipart(Form::new().part(
                "file",
                Part::bytes(audio.clone()).file_name("recording.wav"),
            ))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| {
                trace.event("vad", "request_failed");
                TranscriptionError::Failure(error.to_string())
            })?;
        let detected: VadResponse = detected.json().await.map_err(|error| {
            trace.event("vad", "response_failed");
            TranscriptionError::Failure(error.to_string())
        })?;
        trace.event(
            "vad",
            &format!(
                "completed speech={} segments={} speech_ms={:.0} elapsed_ms={}",
                detected.speech,
                detected.segments,
                detected.speech_ms,
                vad_started.elapsed().as_millis()
            ),
        );
        if !detected.speech {
            return Err(TranscriptionError::Empty);
        }
        Ok(audio)
    }

    pub fn stop(&mut self) {
        if let Some(child) = self.child.take() {
            let _ = child.kill();
        }
        #[cfg(target_os = "windows")]
        self.job.take();
        self.address = None;
        self.model = None;
        self.terminated = None;
    }

    async fn health(&self, address: &str) -> bool {
        self.client
            .get(format!("{address}/health"))
            .timeout(Duration::from_secs(1))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    }
}

impl Drop for WhisperService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_chinese_enables_simplification_without_changing_other_languages() {
        let response: InferenceResponse =
            serde_json::from_str(r#"{"text":"今天測試語音識別。","language":"chinese"}"#).unwrap();
        let transcription = Transcription {
            text: response.text,
            detected_language: response.language,
        };
        assert_eq!(
            transcription.output_language(None).as_deref(),
            Some("zh-CN")
        );
        assert_eq!(
            transcription.output_language(Some("en-US")).as_deref(),
            Some("en-US")
        );
        assert_eq!(
            crate::transcription::simplified(
                &transcription.text,
                transcription.output_language(None).as_deref(),
            ),
            "今天测试语音识别。"
        );
        let japanese = Transcription {
            text: "漢字".into(),
            detected_language: Some("japanese".into()),
        };
        assert_eq!(japanese.output_language(None), None);
    }
}

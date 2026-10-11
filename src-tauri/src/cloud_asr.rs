use crate::asr::Transcription;
use crate::cloud_audio::{self, AudioChunk};
use crate::diagnostics::SessionTrace;
use base64::Engine;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::time::Duration;

pub struct CloudConfig {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub protocol: Protocol,
    key: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Protocol {
    Transcription,
    AudioMessages,
    DashScope,
}

impl Protocol {
    fn from_values(values: &Map<String, Value>) -> Result<Self, String> {
        match values.get("cloudASRProvider").and_then(Value::as_str) {
            Some("openai") => return Ok(Self::Transcription),
            Some("bailian") => {
                return Ok(
                    if values.get("cloudASRProtocol").and_then(Value::as_str)
                        == Some("audioMessages")
                    {
                        Self::AudioMessages
                    } else {
                        Self::DashScope
                    },
                )
            }
            _ => {}
        }
        match values
            .get("cloudASRProtocol")
            .and_then(Value::as_str)
            .unwrap_or("transcription")
        {
            "transcription" => Ok(Self::Transcription),
            "audioMessages" => Ok(Self::AudioMessages),
            "dashscope" => Ok(Self::DashScope),
            _ => Err("不支持的识别接口协议".into()),
        }
    }
    fn path(self) -> &'static str {
        match self {
            Self::Transcription => "/audio/transcriptions",
            Self::AudioMessages => "/chat/completions",
            Self::DashScope => "/services/aigc/multimodal-generation/generation",
        }
    }
}

impl CloudConfig {
    pub fn from_values(values: &Map<String, Value>) -> Result<Self, String> {
        let provider = values
            .get("cloudASRProvider")
            .and_then(Value::as_str)
            .ok_or("请选择云端识别服务")?;
        let endpoint = endpoint(values)?;
        let model = values
            .get("cloudASRModel")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let key = values
            .get("cloudASRAPIKey")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if model.is_empty() || model.chars().any(char::is_control) {
            return Err("请填写有效的识别模型".into());
        }
        if key.is_empty() {
            return Err("请填写云端识别 API Key".into());
        }
        Ok(Self {
            provider: provider.to_owned(),
            endpoint,
            model: model.to_owned(),
            key: key.to_owned(),
            protocol: Protocol::from_values(values)?,
        })
    }

    fn supports_flac(&self) -> bool {
        self.protocol == Protocol::DashScope
            && matches!(
                self.model.as_str(),
                "qwen-audio-3.1-asr-flash"
                    | "qwen-audio-3.0-asr-flash"
                    | "fun-asr-flash-2026-06-15"
            )
    }
}

pub fn endpoint(values: &Map<String, Value>) -> Result<String, String> {
    let provider = values
        .get("cloudASRProvider")
        .and_then(Value::as_str)
        .ok_or("请选择云端识别服务")?;
    let base = match provider {
        "openai" => "https://api.openai.com/v1",
        "compatible" | "bailian" => values
            .get("cloudASRBaseURL")
            .and_then(Value::as_str)
            .ok_or("请填写识别服务地址")?,
        _ => return Err("不支持的识别服务".into()),
    };
    let url = reqwest::Url::parse(base.trim()).map_err(|_| "识别服务地址无效")?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("识别服务地址须使用 HTTPS，且不能包含凭据、查询参数或片段".into());
    }
    let protocol = Protocol::from_values(values)?;
    if values
        .get("cloudASRFullURL")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(url.to_string());
    }
    Ok(format!(
        "{}{}",
        url.as_str().trim_end_matches('/'),
        protocol.path()
    ))
}

#[derive(Deserialize, Default)]
pub struct CloudUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub seconds: Option<f64>,
}

#[derive(Deserialize)]
struct CloudResponse {
    text: String,
    language: Option<String>,
    usage: Option<CloudUsage>,
}

pub struct CloudResult {
    pub transcription: Transcription,
    pub request_id: Option<String>,
    pub usage: CloudUsage,
    pub attempts: u32,
}

pub async fn transcribe(
    config: &CloudConfig,
    audio: Vec<u8>,
    language: Option<&str>,
    vocabulary: &[String],
    trace: &SessionTrace,
) -> Result<CloudResult, String> {
    let original_bytes = audio.len();
    let chunks = if config.supports_flac() {
        tokio::task::spawn_blocking(move || cloud_audio::prepare(audio))
            .await
            .map_err(|error| error.to_string())??
    } else {
        vec![AudioChunk {
            bytes: audio,
            format: "wav",
            sample_rate: 0,
            samples: 0,
        }]
    };
    trace.event(
        "cloud_audio",
        &format!(
            "prepared original_bytes={original_bytes} chunks={} upload_bytes={}",
            chunks.len(),
            chunks.iter().map(|chunk| chunk.bytes.len()).sum::<usize>()
        ),
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建识别请求")?;
    let mut combined: Option<CloudResult> = None;
    for (index, chunk) in chunks.iter().enumerate() {
        trace.event(
            "cloud_asr",
            &format!(
                "chunk_started index={} total={} format={} bytes={} duration_ms={}",
                index + 1,
                chunks.len(),
                chunk.format,
                chunk.bytes.len(),
                if chunk.sample_rate > 0 {
                    chunk.samples as u64 * 1000 / u64::from(chunk.sample_rate)
                } else {
                    0
                }
            ),
        );
        let result = transcribe_chunk(&client, config, chunk, language, vocabulary, trace).await?;
        if let Some(total) = &mut combined {
            total.transcription.text.push('\n');
            total
                .transcription
                .text
                .push_str(&result.transcription.text);
            if total.transcription.detected_language != result.transcription.detected_language {
                total.transcription.detected_language = None;
            }
            total.usage.input_tokens = total
                .usage
                .input_tokens
                .zip(result.usage.input_tokens)
                .map(|(a, b)| a + b);
            total.usage.output_tokens = total
                .usage
                .output_tokens
                .zip(result.usage.output_tokens)
                .map(|(a, b)| a + b);
            total.usage.seconds = total
                .usage
                .seconds
                .zip(result.usage.seconds)
                .map(|(a, b)| a + b);
            total.attempts += result.attempts;
            // Every chunk ID is recorded in the session trace; history retains the final request ID.
            total.request_id = result.request_id;
        } else {
            combined = Some(result);
        }
    }
    combined.ok_or_else(|| "云端未返回识别文字".into())
}

async fn transcribe_chunk(
    client: &reqwest::Client,
    config: &CloudConfig,
    audio: &AudioChunk,
    language: Option<&str>,
    vocabulary: &[String],
    trace: &SessionTrace,
) -> Result<CloudResult, String> {
    if audio.bytes.len() > 25_000_000 {
        return Err("录音超过云端识别的 25 MB 文件限制".into());
    }
    let json_body = if config.protocol == Protocol::Transcription {
        None
    } else {
        Some(audio_message_body(config, audio, language, vocabulary)?)
    };
    for attempt in 1..=2 {
        trace.event(
            "cloud_asr",
            &format!(
                "request_started provider={} protocol={:?} attempt={attempt} bytes={}",
                config.provider,
                config.protocol,
                audio.bytes.len()
            ),
        );
        let request = client
            .post(&config.endpoint)
            .bearer_auth(&config.key)
            .timeout(Duration::from_secs(120));
        let request = match config.protocol {
            Protocol::Transcription => {
                let mut form = Form::new()
                    .part(
                        "file",
                        Part::bytes(audio.bytes.clone())
                            .file_name("recording.wav")
                            .mime_str("audio/wav")
                            .map_err(|_| "无效的音频格式")?,
                    )
                    .text("model", config.model.clone())
                    .text("response_format", "json");
                if let Some(language) = language {
                    form = form.text("language", language.to_owned());
                }
                if !vocabulary.is_empty() {
                    form = form.text("prompt", vocabulary.join("、"));
                }
                request.multipart(form)
            }
            Protocol::AudioMessages => request.json(json_body.as_ref().unwrap()),
            Protocol::DashScope => request
                .header("X-DashScope-SSE", "disable")
                .json(json_body.as_ref().unwrap()),
        };
        let response = request.send().await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let code = if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "connection"
                } else {
                    "network"
                };
                trace.event(
                    "cloud_asr",
                    &format!("failed attempt={attempt} code={code}"),
                );
                if attempt == 1 && error.is_connect() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                return Err(if error.is_timeout() {
                    "云端识别超时，请稍后重试"
                } else {
                    "无法连接云端识别服务，请检查网络与服务地址"
                }
                .into());
            }
        };
        let status = response.status();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
                    .take(128)
                    .collect::<String>()
            })
            .filter(|id| !id.is_empty());
        trace.event(
            "cloud_asr",
            &format!(
                "response attempt={attempt} status={} request_id={}",
                status.as_u16(),
                request_id.as_deref().unwrap_or("none")
            ),
        );
        if attempt == 1 && matches!(status.as_u16(), 502 | 503 | 504) {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }
        if !status.is_success() {
            let error_code = if status.as_u16() == 429 {
                response
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|value| value["error"]["code"].as_str().map(str::to_owned))
            } else {
                None
            };
            return Err(status_message(status.as_u16(), error_code.as_deref()).into());
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|_| "云端返回格式不符合语音转写协议")?;
        let (response, body_request_id) = parse_response(config.protocol, payload)?;
        let request_id = request_id.or(body_request_id);
        let text = response.text.trim().to_owned();
        if text.is_empty() {
            return Err("云端未返回识别文字".into());
        }
        trace.event(
            "cloud_asr",
            &format!("completed attempt={attempt} chars={}", text.chars().count()),
        );
        return Ok(CloudResult {
            transcription: Transcription {
                text,
                detected_language: response.language,
            },
            request_id,
            usage: response.usage.unwrap_or_default(),
            attempts: attempt,
        });
    }
    Err("云端识别暂时不可用".into())
}

fn audio_message_body(
    config: &CloudConfig,
    audio: &AudioChunk,
    language: Option<&str>,
    vocabulary: &[String],
) -> Result<Value, String> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(&audio.bytes);
    // The JSON audio APIs use smaller inline payloads than multipart uploads.
    if encoded.len() + 22 > cloud_audio::INLINE_LIMIT {
        return Err("编码后的录音超过 10 MB，请缩短录音".into());
    }
    let qwen = config.model == "qwen3-asr-flash" || config.model.starts_with("qwen3-asr-flash-20");
    if config.protocol == Protocol::AudioMessages {
        let mut messages = Vec::new();
        let mut context = vocabulary.join("、");
        if !qwen {
            context = format!("Transcribe the audio faithfully. Return only the transcription.\nVocabulary: {context}");
            if let Some(language) = language {
                context.push_str(&format!("\nLanguage: {language}"));
            }
        }
        if !context.is_empty() {
            messages.push(json!({"role":"system","content":context}));
        }
        let data = if qwen {
            format!("data:audio/{};base64,{encoded}", audio.format)
        } else {
            encoded
        };
        messages.push(json!({"role":"user","content":[{"type":"input_audio","input_audio":{"data":data,"format":"wav"}}]}));
        let mut body = json!({"model":config.model,"messages":messages,"stream":false});
        if qwen {
            if let Some(language) = language {
                body["asr_options"] = json!({"language":language});
            }
        }
        return Ok(body);
    }
    let (sample_rate, duration) = if audio.format == "wav" {
        let reader = hound::WavReader::new(std::io::Cursor::new(&audio.bytes))
            .map_err(|_| "无效的 WAV 录音")?;
        (
            reader.spec().sample_rate,
            reader.duration() as f64 / reader.spec().sample_rate as f64,
        )
    } else {
        (
            audio.sample_rate,
            audio.samples as f64 / audio.sample_rate as f64,
        )
    };
    if duration > 300.0 {
        return Err("同步识别录音不能超过 5 分钟".into());
    }
    let mut parameters = json!({"format":audio.format,"sample_rate":sample_rate.to_string()});
    if let Some(language) = language {
        parameters["language_hints"] = json!([language]);
    }
    let mut messages = Vec::new();
    if !vocabulary.is_empty() {
        if matches!(
            config.model.as_str(),
            "qwen-audio-3.1-asr-flash" | "qwen-audio-3.0-asr-flash"
        ) {
            let words: Map<String, Value> = vocabulary
                .iter()
                .take(2000)
                .map(|word| (word.clone(), json!(3)))
                .collect();
            parameters["vocabulary"] = Value::Object(words);
        } else {
            // The documented context channel also accepts domain vocabulary.
            let context: String = vocabulary.join("、").chars().take(400).collect();
            messages.push(json!({"role":"user","content":[{"type":"input_text","text":context}]}));
        }
    }
    messages.push(json!({"role":"user","content":[{"type":"input_audio","input_audio":{"data":format!("data:audio/{};base64,{encoded}", audio.format)}}]}));
    Ok(json!({"model":config.model,"input":{"messages":messages},"parameters":parameters}))
}

fn parse_response(
    protocol: Protocol,
    payload: Value,
) -> Result<(CloudResponse, Option<String>), String> {
    let invalid = "云端返回格式不符合所选识别协议";
    let request_id = payload["request_id"].as_str().map(|id| {
        id.chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
            .take(128)
            .collect()
    });
    if protocol == Protocol::Transcription {
        return serde_json::from_value(payload)
            .map(|response| (response, request_id))
            .map_err(|_| invalid.into());
    }
    let text = match protocol {
        Protocol::AudioMessages => payload["choices"][0]["message"]["content"].as_str(),
        Protocol::DashScope => payload["output"]["text"].as_str(),
        Protocol::Transcription => unreachable!(),
    }
    .ok_or(invalid)?
    .to_owned();
    let usage = &payload["usage"];
    let (input_tokens, output_tokens, seconds) = match protocol {
        Protocol::AudioMessages => (
            usage["prompt_tokens"].as_u64(),
            usage["completion_tokens"].as_u64(),
            None,
        ),
        _ => (
            usage["input_tokens"].as_u64(),
            usage["output_tokens"].as_u64(),
            usage["duration"].as_f64(),
        ),
    };
    Ok((
        CloudResponse {
            text,
            language: None,
            usage: Some(CloudUsage {
                input_tokens,
                output_tokens,
                seconds,
            }),
        },
        request_id,
    ))
}

fn status_message(status: u16, code: Option<&str>) -> &'static str {
    match status {
        401 => "云端识别 API Key 无效",
        403 => "当前密钥没有语音识别权限",
        404 => "找不到识别接口或模型，请检查服务地址与模型",
        413 => "录音超过该服务的文件限制",
        429 if code == Some("insufficient_quota") => "云端识别额度不足",
        429 => "云端识别请求受限，请稍后重试",
        500..=599 => "云端识别服务暂时不可用",
        300..=399 => "识别服务地址发生重定向，请填写最终地址",
        _ => "云端识别请求被拒绝，请检查配置",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_recording_request(fail_second: bool) -> Result<CloudResult, String> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut decoded_samples = 0usize;
            for index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let body_start = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = header
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break end + 4;
                        }
                    }
                };
                let body: Value = serde_json::from_slice(&bytes[body_start..]).unwrap();
                assert_eq!(body["parameters"]["format"], "flac");
                assert_eq!(body["parameters"]["sample_rate"], "16000");
                assert_eq!(body["parameters"]["vocabulary"]["InkSay"], 3);
                let data = body["input"]["messages"][0]["content"][0]["input_audio"]["data"]
                    .as_str()
                    .unwrap();
                assert!(data.len() <= cloud_audio::INLINE_LIMIT);
                let flac = base64::engine::general_purpose::STANDARD
                    .decode(data.strip_prefix("data:audio/flac;base64,").unwrap())
                    .unwrap();
                let mut decoder = claxon::FlacReader::new(std::io::Cursor::new(flac)).unwrap();
                let samples = decoder.samples().map(Result::unwrap).collect::<Vec<_>>();
                for (offset, &sample) in samples.iter().enumerate() {
                    assert_eq!(
                        sample,
                        ((decoded_samples + offset) % 160) as i32 * 200 - 16000
                    );
                }
                decoded_samples += samples.len();
                assert!(samples.len() <= 300 * 16000);
                let response = json!({"output":{"text":if index == 0 {"第一段"} else {"第二段"}},
                    "request_id":format!("chunk-{index}"),"usage":{"input_tokens":10,"output_tokens":2,"duration":samples.len() as f64 / 16000.0}}).to_string();
                let status = if fail_second && index == 1 {
                    "400 Bad Request"
                } else {
                    "200 OK"
                };
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            }
            assert_eq!(decoded_samples, 310 * 16000);
        });
        let values = json!({"cloudASRProvider":"compatible","cloudASRBaseURL":format!("http://{address}/v1"),
            "cloudASRProtocol":"dashscope","cloudASRModel":"qwen-audio-3.1-asr-flash","cloudASRAPIKey":"test"});
        let config = CloudConfig::from_values(values.as_object().unwrap()).unwrap();
        let audio = cloud_audio::tests::wav(
            &(0..310 * 16000)
                .map(|i| ((i % 160) as i16 - 80) * 200)
                .collect::<Vec<_>>(),
        );
        let directory =
            std::env::temp_dir().join(format!("inksay-long-cloud-{}", uuid::Uuid::new_v4()));
        let trace = SessionTrace::new(crate::diagnostics::AppLog::open(directory.clone()).unwrap());
        let result = tauri::async_runtime::block_on(transcribe(
            &config,
            audio,
            Some("zh"),
            &["InkSay".into()],
            &trace,
        ));
        server.join().unwrap();
        trace.finish(if result.is_ok() {
            "completed"
        } else {
            "failed"
        });
        drop(trace);
        std::fs::remove_dir_all(directory).unwrap();
        result
    }

    #[test]
    fn long_recording_uploads_decodable_flac_and_combines_results_and_usage() {
        let result = long_recording_request(false).unwrap();
        assert_eq!(result.transcription.text, "第一段\n第二段");
        assert_eq!(result.request_id.as_deref(), Some("chunk-1"));
        assert_eq!(result.attempts, 2);
        assert_eq!(result.usage.input_tokens, Some(20));
        assert_eq!(result.usage.output_tokens, Some(4));
        assert_eq!(result.usage.seconds, Some(310.0));
    }

    #[test]
    fn a_failed_segment_does_not_return_a_partial_transcription() {
        assert!(long_recording_request(true).is_err());
    }

    #[test]
    fn complete_addresses_and_protocol_paths_are_independent() {
        for (protocol, path) in [
            ("transcription", "/audio/transcriptions"),
            ("audioMessages", "/chat/completions"),
            (
                "dashscope",
                "/services/aigc/multimodal-generation/generation",
            ),
        ] {
            let mut values = json!({"cloudASRProvider":"compatible","cloudASRBaseURL":"https://example.com/v1/","cloudASRProtocol":protocol});
            assert_eq!(
                endpoint(values.as_object().unwrap()).unwrap(),
                format!("https://example.com/v1{path}")
            );
            values["cloudASRFullURL"] = json!(true);
            values["cloudASRBaseURL"] = json!("https://example.com/custom/asr/");
            assert_eq!(
                endpoint(values.as_object().unwrap()).unwrap(),
                "https://example.com/custom/asr/"
            );
        }
    }

    #[test]
    fn json_protocols_send_audio_dictionary_and_parse_their_own_contracts() {
        use std::io::{Read, Write};
        for protocol in ["audioMessages", "dashscope"] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let body_start = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = header
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break end + 4;
                        }
                    }
                };
                let header = String::from_utf8_lossy(&bytes[..body_start]).to_lowercase();
                assert!(header.starts_with("post /custom "));
                assert!(header.contains("content-type: application/json"));
                let body: Value = serde_json::from_slice(&bytes[body_start..]).unwrap();
                let data = if protocol == "dashscope" {
                    assert!(header.contains("x-dashscope-sse: disable"));
                    assert_eq!(body["parameters"]["vocabulary"]["Inksay"], 3);
                    assert_eq!(body["parameters"]["language_hints"], json!(["zh"]));
                    assert_eq!(body["parameters"]["sample_rate"], "16000");
                    body["input"]["messages"][0]["content"][0]["input_audio"]["data"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("data:audio/wav;base64,")
                        .unwrap()
                } else {
                    assert!(body["messages"][0]["content"]
                        .as_str()
                        .unwrap()
                        .contains("Inksay"));
                    assert_eq!(body["stream"], false);
                    body["messages"][1]["content"][0]["input_audio"]["data"]
                        .as_str()
                        .unwrap()
                };
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .unwrap();
                assert!(decoded.starts_with(b"RIFF"));
                let response = if protocol == "dashscope" { json!({"output":{"text":"百炼结果"},"request_id":"dash-123","usage":{"input_tokens":10,"output_tokens":2,"duration":1}}) }
                    else { json!({"choices":[{"message":{"content":"消息结果"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}) }.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                )
                .unwrap();
            });
            let values = json!({"cloudASRProvider":"compatible","cloudASRBaseURL":format!("http://{address}/custom"),"cloudASRFullURL":true,"cloudASRProtocol":protocol,"cloudASRModel":"qwen-audio-3.1-asr-flash","cloudASRAPIKey":"test"});
            let config = CloudConfig::from_values(values.as_object().unwrap()).unwrap();
            let mut audio = std::io::Cursor::new(Vec::new());
            let mut writer = hound::WavWriter::new(
                &mut audio,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 16000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            writer.write_sample(0i16).unwrap();
            writer.finalize().unwrap();
            let directory =
                std::env::temp_dir().join(format!("inksay-protocol-{}", uuid::Uuid::new_v4()));
            let trace =
                SessionTrace::new(crate::diagnostics::AppLog::open(directory.clone()).unwrap());
            let result = tauri::async_runtime::block_on(transcribe(
                &config,
                audio.into_inner(),
                Some("zh"),
                &["Inksay".into()],
                &trace,
            ))
            .unwrap();
            server.join().unwrap();
            assert_eq!(result.usage.input_tokens, Some(10));
            assert_eq!(result.usage.output_tokens, Some(2));
            if protocol == "dashscope" {
                assert_eq!(result.request_id.as_deref(), Some("dash-123"));
                assert_eq!(result.usage.seconds, Some(1.0));
            }
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn mismatched_response_is_rejected_instead_of_guessed() {
        assert!(parse_response(Protocol::AudioMessages, json!({"text":"wrong"})).is_err());
        assert!(parse_response(
            Protocol::DashScope,
            json!({"choices":[{"message":{"content":"wrong"}}]})
        )
        .is_err());
    }
    #[test]
    fn cancellation_discards_late_cloud_response() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            assert!(stream.read(&mut bytes).unwrap() > 0);
            sent.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            let body = r#"{"text":"late response"}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        let values = serde_json::json!({"cloudASRProvider":"compatible","cloudASRBaseURL":format!("http://{address}/v1"),"cloudASRModel":"test","cloudASRAPIKey":"test"});
        let config = CloudConfig::from_values(values.as_object().unwrap()).unwrap();
        let directory =
            std::env::temp_dir().join(format!("inksay-cloud-cancel-{}", uuid::Uuid::new_v4()));
        let log = crate::diagnostics::AppLog::open(directory.clone()).unwrap();
        let trace = SessionTrace::new(log.clone());
        let task_trace = trace.clone();
        tauri::async_runtime::block_on(async move {
            let task = tokio::spawn(async move {
                transcribe(&config, vec![0; 512], None, &[], &task_trace).await
            });
            received.await.unwrap();
            trace.finish("cancelled");
            task.abort();
            assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        });
        server.join().unwrap();
        let content = std::fs::read_to_string(log.path()).unwrap();
        assert!(content.contains("outcome=cancelled"));
        assert!(!content.contains("completed attempt="));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn transcriptions_use_audio_contract_retry_once_and_keep_usage() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for (status, body) in [
                (
                    "503 Service Unavailable",
                    r#"{"error":{"message":"not logged"}}"#,
                ),
                (
                    "200 OK",
                    r#"{"text":"语音结果","usage":{"input_tokens":12,"output_tokens":5}}"#,
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(end) = request.windows(4).position(|x| x == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let size: usize = header
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if request.len() >= end + 4 + size {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&request);
                assert!(request.starts_with("POST /v1/audio/transcriptions "));
                assert!(request.contains("Bearer cloud-only-key"));
                assert!(request.contains("name=\"file\""));
                assert!(request.contains("name=\"model\""));
                assert!(!request.contains("chat/completions"));
                write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nx-request-id: test-123\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let values = serde_json::json!({"cloudASRProvider":"compatible","cloudASRBaseURL":format!("http://{address}/v1"),"cloudASRModel":"whisper-1","cloudASRAPIKey":"cloud-only-key"});
        let config = CloudConfig::from_values(values.as_object().unwrap()).unwrap();
        let directory =
            std::env::temp_dir().join(format!("inksay-cloud-http-{}", uuid::Uuid::new_v4()));
        let log = crate::diagnostics::AppLog::open(directory.clone()).unwrap();
        let trace = SessionTrace::new(log.clone());
        let result = tauri::async_runtime::block_on(transcribe(
            &config,
            b"test-audio".to_vec(),
            Some("zh"),
            &[],
            &trace,
        ))
        .unwrap();
        server.join().unwrap();
        assert_eq!(result.transcription.text, "语音结果");
        assert_eq!(result.usage.input_tokens, Some(12));
        assert_eq!(result.usage.output_tokens, Some(5));
        assert_eq!(result.attempts, 2);
        assert_eq!(result.request_id.as_deref(), Some("test-123"));
        trace.finish("completed");
        let content = std::fs::read_to_string(log.path()).unwrap();
        assert!(!content.contains("cloud-only-key"));
        assert!(!content.contains("语音结果"));
        assert!(content.contains("attempt=2"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn endpoint_validation_rejects_credentials_and_nonlocal_plaintext() {
        for base in [
            "http://example.com/v1",
            "https://key@example.com/v1",
            "https://example.com/v1?key=secret",
            "https://example.com/#token",
        ] {
            let config = serde_json::json!({"cloudASRProvider":"compatible", "cloudASRBaseURL":base, "cloudASRModel":"whisper-1", "cloudASRAPIKey":"secret"});
            assert!(CloudConfig::from_values(config.as_object().unwrap()).is_err());
        }
    }
    #[test]
    fn errors_do_not_echo_provider_bodies() {
        assert_eq!(
            status_message(429, Some("insufficient_quota")),
            "云端识别额度不足"
        );
        assert_ne!(status_message(429, None), status_message(401, None));
    }
}

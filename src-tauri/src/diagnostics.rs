use regex::Regex;
use serde_json::{Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

const MAX_LOG_BYTES: u64 = 512 * 1024;

#[derive(Clone)]
pub struct AppLog {
    path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

impl AppLog {
    pub fn open(directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let log = Self {
            path: directory.join("inksay.log"),
            write_lock: Arc::new(Mutex::new(())),
        };
        log.write(
            "app",
            &format!(
                "started pid={} metadata={}",
                std::process::id(),
                crate::build_info::metadata()
            ),
        )?;
        Ok(log)
    }

    pub fn write(&self, category: &str, event: &str) -> Result<(), String> {
        let _guard = self.write_lock.lock().map_err(|error| error.to_string())?;
        if self
            .path
            .metadata()
            .is_ok_and(|meta| meta.len() > MAX_LOG_BYTES)
        {
            let previous = self.path.with_extension("log.1");
            if previous.exists() {
                fs::remove_file(&previous).map_err(|error| error.to_string())?;
            }
            fs::rename(&self.path, previous).map_err(|error| error.to_string())?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&self.path)
            .map_err(|error| error.to_string())?;
        writeln!(
            file,
            "{} [{category}] {event}",
            chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false)
        )
        .map_err(|error| error.to_string())
    }

    fn contents(&self) -> Result<String, String> {
        let _guard = self.write_lock.lock().map_err(|error| error.to_string())?;
        let previous = self.path.with_extension("log.1");
        let mut parts = Vec::new();
        for path in [&previous, &self.path] {
            match fs::read_to_string(path) {
                Ok(content) => parts.push(content),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(parts.join("\n"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn log(app: &tauri::AppHandle, category: &str, event: &str) {
    if let Err(error) = app.state::<AppLog>().write(category, event) {
        eprintln!("file log failed: {error}");
    }
}

/// A trace belongs to a recording, never to the globally latest request.
#[derive(Clone)]
pub struct SessionTrace(Arc<SessionTraceInner>);

struct SessionTraceInner {
    id: String,
    log: AppLog,
    started: Instant,
    state: Mutex<TraceState>,
}

struct TraceState {
    stage: &'static str,
    finished: bool,
}

impl SessionTrace {
    pub fn new(log: AppLog) -> Self {
        let trace = Self(Arc::new(SessionTraceInner {
            id: uuid::Uuid::new_v4().to_string().to_uppercase(),
            log,
            started: Instant::now(),
            state: Mutex::new(TraceState {
                stage: "recording",
                finished: false,
            }),
        }));
        trace.event("recording", "requested");
        trace
    }

    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn event(&self, stage: &'static str, event: &str) {
        if let Ok(mut state) = self.0.state.lock() {
            if state.finished {
                return;
            }
            state.stage = stage;
            self.0.write(stage, event);
        }
    }

    pub fn finish(&self, outcome: &'static str) {
        if let Ok(mut state) = self.0.state.lock() {
            if state.finished {
                return;
            }
            state.finished = true;
            self.0
                .write(state.stage, &format!("ended outcome={outcome}"));
        }
    }
}

impl SessionTraceInner {
    fn write(&self, stage: &str, event: &str) {
        // Callers supply fixed event codes and numeric metadata, never user text/errors.
        let line = format!(
            "session={} stage={stage} session_elapsed_ms={} {event}",
            self.id,
            self.started.elapsed().as_millis()
        );
        if let Err(error) = self.log.write("voice", &line) {
            eprintln!("session log failed: {error}");
        }
    }
}

impl Drop for SessionTraceInner {
    fn drop(&mut self) {
        if let Ok(state) = self.state.lock() {
            if !state.finished {
                self.write(state.stage, "ended outcome=interrupted");
            }
        }
    }
}

#[tauri::command]
pub async fn diagnostics_export(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let name = format!(
        "Inksay-诊断报告-{}.txt",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    log(&app, "diagnostics", "export dialog opened");
    let dialog_app = app.clone();
    let destination = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title("导出 Inksay 诊断报告")
            .set_file_name(name)
            .add_filter("Text", &["txt"])
            .blocking_save_file()
    })
    .await
    .map_err(|error| error.to_string())?;
    let Some(destination) = destination else {
        return Ok(None);
    };
    let destination = destination.into_path().map_err(|error| error.to_string())?;
    let config = app
        .state::<crate::AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .snapshot()
        .values;
    let log_content = app.state::<AppLog>().contents()?;
    let model_bytes = crate::model::total_bytes(&app)?;
    let recording_bytes = app
        .state::<crate::AppStore>()
        .lock()
        .map_err(|error| error.to_string())?
        .recording_bytes()?;
    #[cfg(target_os = "macos")]
    let permissions = {
        let status = crate::read_permissions()?;
        format!(
            "- 麦克风: {}\n- 辅助功能: {}\n- 输入监控: {}",
            if status.microphone == "authorized" {
                "已授权"
            } else {
                "缺失"
            },
            if status.accessibility {
                "已授权"
            } else {
                "缺失"
            },
            if status.input_monitoring {
                "已授权"
            } else {
                "缺失"
            }
        )
    };
    #[cfg(not(target_os = "macos"))]
    let permissions = String::new();
    let report = format!(
        "Inksay 诊断报告\n生成时间: {}\n应用版本: {}\nBundle ID: {}\n系统: {}\n架构: {}\n\n配置摘要\n{}\n本机模型: {}\n模型占用: {:.1} MB\n录音占用: {:.1} MB\n\n权限\n{}\n\n运行日志\n{}",
        chrono::Local::now().to_rfc3339(),
        crate::build_info::metadata(),
        app.config().identifier,
        std::env::consts::OS,
        std::env::consts::ARCH,
        config_summary(&config),
        config.get("localWhisperModel").and_then(Value::as_str).unwrap_or(crate::model::DEFAULT_MODEL),
        model_bytes as f64 / 1_000_000.0,
        recording_bytes as f64 / 1_000_000.0,
        permissions,
        if log_content.is_empty() { "（暂无运行日志）" } else { &log_content },
    );
    let home = app.path().home_dir().map_err(|error| error.to_string())?;
    let sanitized = sanitize_report(&report, &home, &std::env::temp_dir());
    fs::write(&destination, sanitized).map_err(|error| error.to_string())?;
    let bytes = destination
        .metadata()
        .map_err(|error| error.to_string())?
        .len();
    log(
        &app,
        "diagnostics",
        &format!("report exported bytes={bytes}"),
    );
    reveal(&destination)?;
    Ok(Some(destination.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn diagnostics_reveal_log(app: tauri::AppHandle) -> Result<(), String> {
    let log = app.state::<AppLog>();
    if !log.path().exists() {
        log.write("app", "log created")?;
    }
    reveal(log.path())
}

fn config_summary(values: &Map<String, Value>) -> String {
    let text = |key: &str, default: &str| {
        values
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_owned()
    };
    let flag =
        |key: &str, default: bool| values.get(key).and_then(Value::as_bool).unwrap_or(default);
    format!(
        "热键={} 方式={} 语言={} 自动检测={} 模型={} 风格={} 上下文={} 写入={} 留存={} 识别方式={} 云端服务={} 云端模型={}",
        text("hotkeyTrigger", "rightOption"),
        text("hotkeyMode", "hold"),
        text("language", "zh-CN"),
        flag("autoDetectLanguage", false),
        text("localWhisperModel", crate::model::DEFAULT_MODEL),
        text("refineStyle", "clean"),
        flag("includeContext", true),
        text("injectionStrategy", "automatic"),
        text("recordingRetention", "day30"),
        text("transcriptionBackend", "localWhisper"),
        text("cloudASRProvider", "openai"),
        text("cloudASRModel", "")
    )
}

fn sanitize_report(text: &str, home: &Path, temporary: &Path) -> String {
    let result = text
        .replace(&home.to_string_lossy().to_string(), "~")
        .replace(&temporary.to_string_lossy().to_string(), "<temporary>");
    let key = Regex::new(r"sk-[A-Za-z0-9_-]{8,}").unwrap();
    let result = key.replace_all(&result, "[REDACTED_API_KEY]");
    let bearer = Regex::new(r"(?i)Bearer\s+[^\s]+").unwrap();
    bearer
        .replace_all(&result, "Bearer [REDACTED]")
        .into_owned()
}

#[cfg(target_os = "macos")]
fn reveal(path: &Path) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .status()
        .map_err(|error| error.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or("could not reveal file in Finder".into())
}

#[cfg(target_os = "windows")]
fn reveal(path: &Path) -> Result<(), String> {
    let status = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .status()
        .map_err(|error| error.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or("could not reveal file in Explorer".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_keep_distinct_ids_and_only_one_terminal_event() {
        let directory = std::env::temp_dir().join(format!("inksay-trace-{}", uuid::Uuid::new_v4()));
        let log = AppLog::open(directory.clone()).unwrap();
        let first = SessionTrace::new(log.clone());
        let second = SessionTrace::new(log.clone());
        assert_ne!(first.id(), second.id());
        assert!(uuid::Uuid::parse_str(first.id()).is_ok());
        first.event("asr", "started");
        second.finish("cancelled");
        second.event("insertion", "late_event_must_not_appear");
        let clone = first.clone();
        let thread = std::thread::spawn(move || clone.finish("completed"));
        thread.join().unwrap();
        first.finish("failed");
        let content = log.contents().unwrap();
        for id in [first.id(), second.id()] {
            assert_eq!(
                content
                    .lines()
                    .filter(|line| line.contains(id) && line.contains("ended outcome="))
                    .count(),
                1
            );
        }
        assert!(content.contains("stage=asr"));
        assert!(!content.contains("late_event_must_not_appear"));
        assert!(!content.contains("outcome=failed"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unfinished_session_reports_interruption_when_last_owner_drops() {
        let directory =
            std::env::temp_dir().join(format!("inksay-trace-drop-{}", uuid::Uuid::new_v4()));
        let log = AppLog::open(directory.clone()).unwrap();
        let trace = SessionTrace::new(log.clone());
        let id = trace.id().to_owned();
        let pending_result = trace.clone();
        drop(trace);
        assert!(!log.contents().unwrap().contains("outcome=interrupted"));
        pending_result.event("result", "rendered chars=10");
        drop(pending_result);
        let content = log.contents().unwrap();
        assert!(content.contains("stage=result"));
        assert!(content.contains("outcome=interrupted"));
        assert!(
            sanitize_report(&content, Path::new("/Users/person"), Path::new("/tmp")).contains(&id)
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn report_redacts_credentials_and_private_paths() {
        let report = "key sk-abcdefgh12345678 Bearer abc.def /Users/person/private /tmp/voice.wav";
        let clean = sanitize_report(report, Path::new("/Users/person"), Path::new("/tmp"));
        assert!(!clean.contains("sk-abcdefgh12345678"));
        assert!(!clean.contains("Bearer abc.def"));
        assert!(!clean.contains("/Users/person"));
        assert!(!clean.contains("/tmp/voice.wav"));
    }

    #[test]
    fn configuration_summary_never_includes_api_key() {
        let mut values = Map::new();
        values.insert(
            "refineAPIKey".into(),
            Value::from("private-token-without-sk-prefix"),
        );
        values.insert("refineStyle".into(), Value::from("clean"));
        let summary = config_summary(&values);
        assert!(!summary.contains("private-token-without-sk-prefix"));
    }

    #[test]
    fn log_rotates_without_exposing_previous_content() {
        let directory = std::env::temp_dir().join(format!("inksay-log-{}", uuid::Uuid::new_v4()));
        let log = AppLog::open(directory.clone()).unwrap();
        fs::write(log.path(), "x".repeat(MAX_LOG_BYTES as usize + 1)).unwrap();
        log.write("test", "rotated").unwrap();
        assert!(log.path().with_extension("log.1").exists());
        assert!(fs::read_to_string(log.path())
            .unwrap()
            .contains("[test] rotated"));
        fs::remove_dir_all(directory).unwrap();
    }
}

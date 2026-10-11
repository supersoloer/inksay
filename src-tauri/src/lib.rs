mod asr;
mod audio;
mod build_info;
mod cloud_asr;
mod cloud_audio;
mod context;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod correction;
mod data_commands;
mod diagnostics;
mod hotkey;
mod language;
mod learning;
mod lexicon;
#[cfg(target_os = "macos")]
mod mac_ask;
#[cfg(target_os = "macos")]
mod mac_ax_text;
#[cfg(target_os = "macos")]
mod mac_context;
#[cfg(target_os = "macos")]
mod mac_floating;
#[cfg(target_os = "macos")]
mod mac_focus;
#[cfg(target_os = "macos")]
mod mac_hotkey;
#[cfg(target_os = "macos")]
mod mac_output;
#[cfg(target_os = "macos")]
mod mac_paste;
mod model;
mod persistence;
#[cfg(feature = "prompt-eval")]
mod prompt_eval;
mod recording;
mod settings;
mod statistics;
mod storage;
mod text_format;
mod transcription;
mod tray;
#[cfg(target_os = "windows")]
mod win_ask;
#[cfg(target_os = "windows")]
mod win_context;
#[cfg(target_os = "windows")]
mod win_floating;
#[cfg(target_os = "windows")]
mod win_focus;
#[cfg(target_os = "windows")]
mod win_hotkey;
#[cfg(target_os = "windows")]
mod win_input;
#[cfg(target_os = "windows")]
mod win_job;
#[cfg(target_os = "windows")]
mod win_output;
#[cfg(target_os = "windows")]
mod win_paste;

use audio::normalize_wav;

use recording::Recorder;
use serde::Serialize;
use settings::{Settings, SettingsSnapshot};
use std::sync::Mutex;
use storage::Store;
use tauri::{Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

fn startup_data_error(app: &tauri::AppHandle, error: String) -> std::io::Error {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(&error)
        .title("InkSay 数据读取失败")
        .kind(MessageDialogKind::Error)
        .blocking_show();
    std::io::Error::other(error)
}

fn open_page_window(app: &tauri::AppHandle, page: &str) -> Result<(), String> {
    let (title, width, height) = match page {
        "history" => ("历史记录", 660.0, 560.0),
        "dictionary" => ("词典", 620.0, 540.0),
        _ => return Err("unknown page window".into()),
    };
    if let Some(window) = app.get_webview_window(page) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let url = tauri::WebviewUrl::App(format!("index.html?view={page}").into());
    let builder = tauri::WebviewWindowBuilder::new(app, page, url)
        .title(title)
        .inner_size(width, height)
        .min_inner_size(520.0, 440.0);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(20.0, 18.0));
    let window = builder.build().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    if let Some(marker) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(marker).activate();
    }
    window.set_focus().map_err(|error| error.to_string())?;
    let visible = window.is_visible().map_err(|error| error.to_string())?;
    let focused = window.is_focused().map_err(|error| error.to_string())?;
    diagnostics::log(
        app,
        "window",
        &format!("opened {page} visible={visible} focused={focused}"),
    );
    Ok(())
}

#[tauri::command]
async fn open_workspace(app: tauri::AppHandle, page: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || match page.as_str() {
        "home" => tray::show_main(&app),
        "history" | "dictionary" => open_page_window(&app, &page),
        _ => Err("unknown workspace page".into()),
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn app_exit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn open_settings(app: tauri::AppHandle, section: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || show_settings_window(app, section))
        .await
        .map_err(|error| error.to_string())?
}

fn show_settings_window(app: tauri::AppHandle, section: Option<String>) -> Result<(), String> {
    if let Some(section) = section.as_deref() {
        if !matches!(
            section,
            "general" | "transcription" | "refine" | "permission" | "about"
        ) {
            return Err("unknown settings section".into());
        }
    }
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        if let Some(section) = section {
            diagnostics::log(&app, "window", &format!("settings section={section}"));
            let _ = app.emit_to("settings", "settings-section", section);
        }
        return Ok(());
    }
    let section = section.unwrap_or_else(|| "general".to_owned());
    diagnostics::log(&app, "window", &format!("settings section={section}"));
    let url = tauri::WebviewUrl::App(format!("index.html?view=settings&section={section}").into());
    let builder = tauri::WebviewWindowBuilder::new(&app, "settings", url)
        .title("Inksay 设置")
        .inner_size(700.0, 560.0)
        .min_inner_size(640.0, 500.0);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(20.0, 18.0));
    let window = builder.build().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    if let Some(marker) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(marker).activate();
    }
    window.set_focus().map_err(|error| error.to_string())?;
    let visible = window.is_visible().map_err(|error| error.to_string())?;
    let focused = window.is_focused().map_err(|error| error.to_string())?;
    diagnostics::log(
        &app,
        "window",
        &format!("settings created visible={visible} focused={focused}"),
    );
    Ok(())
}

type AppStore = Mutex<Store>;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn remember_successful_dictation(app: &tauri::AppHandle, text: &str) {
    let result = app
        .state::<AppStore>()
        .lock()
        .map_err(|error| error.to_string())
        .and_then(|mut store| store.learn_from_text(text));
    match result {
        Ok(true) => {
            let _ = app.emit("dictionary-changed", ());
        }
        Err(_) => diagnostics::log(app, "dictionary", "term learning failed"),
        Ok(false) => {}
    }
}
type AppRecorder = Mutex<Recorder>;
type AppSettings = Mutex<Settings>;
type AppWhisper = tokio::sync::Mutex<asr::WhisperService>;
type AppSelection = Mutex<String>;
#[cfg(target_os = "macos")]
type AppOutputMute = Mutex<mac_output::OutputMute>;
#[cfg(target_os = "windows")]
type AppOutputMute = Mutex<win_output::OutputMute>;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn restore_output(app: &tauri::AppHandle) {
    let result = app
        .state::<AppOutputMute>()
        .lock()
        .map_err(|error| error.to_string())
        .and_then(|output| output.restore());
    if let Err(error) = result {
        eprintln!("output mute restore failed: {error}");
        diagnostics::log(app, "audio", "output mute restore failed");
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn mute_output(app: &tauri::AppHandle) {
    let result = app
        .state::<AppOutputMute>()
        .lock()
        .map_err(|error| error.to_string())
        .and_then(|output| output.mute());
    if let Err(error) = result {
        eprintln!("output mute failed: {error}");
        diagnostics::log(app, "audio", "output mute failed");
    }
}

#[tauri::command]
fn selected_mode(state: State<'_, AppSelection>) -> Result<String, String> {
    Ok(state.lock().map_err(|error| error.to_string())?.clone())
}

#[tauri::command]
fn select_mode(
    app: tauri::AppHandle,
    state: State<'_, AppSelection>,
    mode: String,
    notify: Option<bool>,
) -> Result<String, String> {
    if !matches!(mode.as_str(), "dictate" | "translate" | "ask") {
        return Err(format!("unknown voice mode: {mode}"));
    }
    *state.lock().map_err(|error| error.to_string())? = mode.clone();
    let _ = tray::refresh(&app);
    if notify.unwrap_or(true) {
        let _ = app.emit("mode-selected", mode.as_str());
    }
    Ok(mode)
}
#[cfg(target_os = "macos")]
type AppHotkey = Mutex<Option<mac_hotkey::MacHotkeyController>>;
#[cfg(target_os = "windows")]
type AppHotkey = Mutex<Option<win_hotkey::WinHotkeyController>>;

#[cfg(target_os = "macos")]
#[tauri::command]
fn hotkey_start(app: tauri::AppHandle) -> Result<(), String> {
    use hotkey::TriggerMode;

    let config = app
        .state::<AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values();
    let trigger = config
        .get("hotkeyTrigger")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("rightOption");
    let mode = match config
        .get("hotkeyMode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("hold")
    {
        "hold" => TriggerMode::Hold,
        "toggle" => TriggerMode::Toggle,
        value => return Err(format!("unknown hotkey mode: {value}")),
    };
    let hotkey = app.state::<AppHotkey>();
    let mut slot = hotkey.lock().map_err(|error| error.to_string())?;
    if slot.is_some() {
        return Ok(());
    }
    *slot = Some(mac_hotkey::MacHotkeyController::start(
        app.clone(),
        trigger,
        mode,
    )?);
    drop(slot);
    let _ = app.emit("hotkey-status-changed", true);
    let _ = tray::refresh(&app);
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn hotkey_stop(app: tauri::AppHandle) -> Result<(), String> {
    let controller = app
        .state::<AppHotkey>()
        .lock()
        .map_err(|error| error.to_string())?
        .take();
    drop(controller);
    let _ = app.emit("hotkey-status-changed", false);
    let _ = tray::refresh(&app);
    app.state::<AppRecorder>()
        .lock()
        .map_err(|error| error.to_string())?
        .cancel();
    restore_output(&app);
    let _ = mac_floating::set(&app, "idle", "dictate");
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn hotkey_start(app: tauri::AppHandle) -> Result<(), String> {
    use hotkey::TriggerMode;
    let config = app
        .state::<AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values();
    let trigger = config
        .get("hotkeyTrigger")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("rightControl");
    let mode = match config
        .get("hotkeyMode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("hold")
    {
        "hold" => TriggerMode::Hold,
        "toggle" => TriggerMode::Toggle,
        value => return Err(format!("unknown hotkey mode: {value}")),
    };
    let hotkey = app.state::<AppHotkey>();
    let mut slot = hotkey.lock().map_err(|error| error.to_string())?;
    if slot.is_some() {
        return Ok(());
    }
    *slot = Some(win_hotkey::WinHotkeyController::start(
        app.clone(),
        trigger,
        mode,
    )?);
    drop(slot);
    let _ = app.emit("hotkey-status-changed", true);
    let _ = tray::refresh(&app);
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn hotkey_stop(app: tauri::AppHandle) -> Result<(), String> {
    let controller = app
        .state::<AppHotkey>()
        .lock()
        .map_err(|error| error.to_string())?
        .take();
    drop(controller);
    let _ = app.emit("hotkey-status-changed", false);
    let _ = tray::refresh(&app);
    app.state::<AppRecorder>()
        .lock()
        .map_err(|error| error.to_string())?
        .cancel();
    restore_output(&app);
    let _ = win_floating::set(&app, "idle", "dictate");
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn hotkey_status(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(app
        .state::<AppHotkey>()
        .lock()
        .map_err(|error| error.to_string())?
        .is_some())
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn hotkey_status(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(app
        .state::<AppHotkey>()
        .lock()
        .map_err(|error| error.to_string())?
        .is_some())
}

#[cfg(target_os = "macos")]
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct MacPermissions {
    microphone: &'static str,
    accessibility: bool,
    input_monitoring: bool,
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn permissions_status(app: tauri::AppHandle) -> Result<MacPermissions, String> {
    let status = read_permissions()?;
    complete_initial_setup(&app, &status)?;
    let _ = tray::refresh(&app);
    Ok(status)
}

#[cfg(target_os = "macos")]
fn complete_initial_setup(app: &tauri::AppHandle, status: &MacPermissions) -> Result<(), String> {
    if !permissions_ready(status) {
        return Ok(());
    }
    let state = app.state::<AppSettings>();
    let mut settings = state.lock().map_err(|error| error.to_string())?;
    if settings.snapshot().values["hasShownInitialSetup"] != true {
        let next = settings.set("hasShownInitialSetup", serde_json::Value::from(true))?;
        drop(settings);
        let _ = app.emit("settings-changed", next);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn permissions_ready(status: &MacPermissions) -> bool {
    status.microphone == "authorized" && status.accessibility && status.input_monitoring
}

#[cfg(target_os = "macos")]
fn read_permissions() -> Result<MacPermissions, String> {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    let media_type = unsafe { AVMediaTypeAudio }.ok_or("audio media type is unavailable")?;
    let microphone = match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) } {
        AVAuthorizationStatus::Authorized => "authorized",
        AVAuthorizationStatus::NotDetermined => "notDetermined",
        _ => "denied",
    };
    Ok(MacPermissions {
        microphone,
        accessibility: mac_focus::has_permission(),
        input_monitoring: mac_hotkey::has_permission(),
    })
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn microphone_request() -> Result<bool, String> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};

    let media_type = unsafe { AVMediaTypeAudio }.ok_or("audio media type is unavailable")?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    {
        let handler = RcBlock::new(move |granted: Bool| {
            if let Ok(mut sender) = sender.lock() {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(granted.as_bool());
                }
            }
        });
        unsafe {
            AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler)
        };
    }
    tokio::time::timeout(std::time::Duration::from_secs(120), receiver)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn permissions_open(kind: String) -> Result<(), String> {
    let anchor = match kind.as_str() {
        "accessibility" => {
            mac_focus::request_permission();
            "Privacy_Accessibility"
        }
        "inputMonitoring" => {
            mac_hotkey::request_permission();
            "Privacy_ListenEvent"
        }
        "microphone" => "Privacy_Microphone",
        _ => return Err("unsupported permission".into()),
    };
    let url =
        format!("x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?{anchor}");
    let status = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("could not open system settings: {status}"))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AskResultSnapshot {
    session_id: String,
    text: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceResult {
    transcript: String,
    refined_text: String,
    inserted: bool,
    #[serde(skip_serializing)]
    history_id: String,
    #[serde(skip_serializing)]
    trace: diagnostics::SessionTrace,
    #[serde(skip_serializing)]
    correction_marker: Option<String>,
}

struct RemoveFileOnDrop(Option<std::path::PathBuf>);

impl Drop for RemoveFileOnDrop {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[tauri::command]
fn contact_developer(channel: String) -> Result<(), String> {
    let address = match channel.as_str() {
        "email" => "mailto:imseantang@gmail.com",
        "xiaohongshu" => "https://www.xiaohongshu.com/user/profile/5df9060d0000000001004d95",
        _ => return Err("不支持的联系方式".into()),
    };
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(address).status();
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", address])
        .status();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = std::process::Command::new("xdg-open").arg(address).status();
    match status {
        Ok(status) if status.success() => Ok(()),
        _ => Err("无法打开链接，请检查系统默认应用".into()),
    }
}

#[tauri::command]
fn app_metadata() -> serde_json::Value {
    build_info::metadata()
}

#[tauri::command]
fn settings_get(state: State<'_, AppSettings>) -> Result<SettingsSnapshot, String> {
    Ok(state.lock().map_err(|error| error.to_string())?.snapshot())
}

#[tauri::command]
fn settings_set(
    app: tauri::AppHandle,
    state: State<'_, AppSettings>,
    key: String,
    value: serde_json::Value,
) -> Result<SettingsSnapshot, String> {
    let next = state
        .lock()
        .map_err(|error| error.to_string())?
        .set(&key, value)?;
    if key == "recordingRetention" {
        let store_state = app.state::<AppStore>();
        let mut store = store_state.lock().map_err(|error| error.to_string())?;
        store.prune_recordings(
            next.values["recordingRetention"]
                .as_str()
                .ok_or("invalid recording retention")?,
        )?;
        drop(store);
        let _ = app.emit("history-changed", ());
    }
    if key == "silenceAutoStopSeconds" {
        app.state::<AppRecorder>()
            .lock()
            .map_err(|error| error.to_string())?
            .set_silence_seconds(
                next.values["silenceAutoStopSeconds"]
                    .as_f64()
                    .ok_or("invalid silence duration")?,
            );
    }
    if key == "transcriptionBackend" {
        if let Ok(mut service) = app.state::<AppWhisper>().try_lock() {
            service.stop();
        }
    }
    if (key == "localWhisperModel" || key == "transcriptionBackend")
        && next.values["transcriptionBackend"] != "cloudASR"
    {
        let selected = next.values["localWhisperModel"]
            .as_str()
            .ok_or("invalid Whisper model")?
            .to_owned();
        let prewarm_app = app.clone();
        if model::has_cached_candidate(&prewarm_app, &selected)? {
            tauri::async_runtime::spawn(async move {
                match model::prepare(&prewarm_app, &selected).await {
                    Ok(path) => {
                        let whisper_state = prewarm_app.state::<AppWhisper>();
                        let mut whisper = whisper_state.lock().await;
                        let current =
                            prewarm_app
                                .state::<AppSettings>()
                                .lock()
                                .is_ok_and(|settings| {
                                    settings.snapshot().values["transcriptionBackend"] != "cloudASR"
                                        && settings
                                            .backend_values()
                                            .get("localWhisperModel")
                                            .and_then(serde_json::Value::as_str)
                                            == Some(selected.as_str())
                                });
                        if current {
                            if let Err(error) = whisper.ensure_loaded(&prewarm_app, &path).await {
                                eprintln!("ASR model prewarm failed: {error}");
                            }
                        }
                    }
                    Err(error) => eprintln!("ASR model validation failed: {error}"),
                }
            });
        }
    }
    let _ = app.emit("settings-changed", next.clone());
    diagnostics::log(&app, "settings", &format!("updated {key}"));
    let _ = tray::refresh(&app);
    Ok(next)
}

#[tauri::command]
fn recording_bytes(state: State<'_, AppStore>) -> Result<u64, String> {
    state
        .lock()
        .map_err(|error| error.to_string())?
        .recording_bytes()
}

#[tauri::command]
fn recording_start(
    app: tauri::AppHandle,
    state: State<'_, AppRecorder>,
    settings: State<'_, AppSettings>,
) -> Result<(), String> {
    let config = settings
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values();
    if config
        .get("transcriptionBackend")
        .and_then(serde_json::Value::as_str)
        == Some("cloudASR")
    {
        cloud_asr::CloudConfig::from_values(&config)?;
    }
    let preferred = config
        .get("preferredMicrophoneUID")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let silence = config
        .get("silenceAutoStopSeconds")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let mut recorder = state.lock().map_err(|error| error.to_string())?;
    if recorder.is_active() {
        return Err("recording already active".into());
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let muted = config
        .get("muteSystemOutput")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    #[cfg(target_os = "macos")]
    {
        let sounds = config
            .get("interactionSounds")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if sounds && !muted {
            mac_output::play_start(&app);
        }
    }
    #[cfg(target_os = "windows")]
    if !muted
        && config
            .get("interactionSounds")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
    {
        win_output::play_start();
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if muted {
        mute_output(&app);
    }
    let trace = diagnostics::SessionTrace::new(app.state::<diagnostics::AppLog>().inner().clone());
    let started = recorder.start(preferred, silence);
    if started.is_ok() {
        trace.event("recording", "started");
        recorder.trace = Some(trace);
    } else {
        trace.finish("start_failed");
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if started.is_err() {
        restore_output(&app);
    }
    #[cfg(target_os = "macos")]
    if started.is_err() {
        if config
            .get("interactionSounds")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
        {
            mac_output::play_failure(&app);
        }
    }
    #[cfg(target_os = "windows")]
    if started.is_err()
        && config
            .get("interactionSounds")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true)
    {
        win_output::play_failure();
    }
    started
}

#[tauri::command]
async fn cloud_asr_test(app: tauri::AppHandle) -> Result<(), String> {
    let config = app
        .state::<AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values();
    let cloud = cloud_asr::CloudConfig::from_values(&config)?;
    let trace = diagnostics::SessionTrace::new(app.state::<diagnostics::AppLog>().inner().clone());
    trace.event("cloud_test", "started bundled_sample=true");
    let path = app
        .path()
        .resolve(
            "resources/asr-test.wav",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|error| error.to_string())?;
    let audio = tokio::fs::read(path)
        .await
        .map_err(|_| "无法读取测试音频")?;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        cloud_asr::transcribe(&cloud, audio, Some("en"), &[], &trace),
    )
    .await
    .map_err(|_| "云端识别测试超时".to_owned())
    .and_then(|result| result);
    trace.finish(if result.is_ok() {
        "test_passed"
    } else {
        "test_failed"
    });
    result.map(|_| ())
}

#[tauri::command]
fn audio_input_devices() -> Result<Vec<recording::AudioInputDevice>, String> {
    recording::input_devices()
}

fn recording_stop(
    app: tauri::AppHandle,
    state: State<'_, AppRecorder>,
    retention: &str,
    trace: &diagnostics::SessionTrace,
) -> Result<String, String> {
    let stopped = (|| {
        let directory = if retention == "off" {
            app.path()
                .app_cache_dir()
                .map_err(|error| error.to_string())?
                .join("inflight-recordings")
        } else {
            app.path()
                .app_data_dir()
                .map_err(|error| error.to_string())?
                .join("recordings")
        };
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let id = trace.id();
        let raw = directory.join(format!("{id}.source.wav"));
        let path = directory.join(format!("{id}.wav"));
        let stopped = state.lock().map_err(|error| error.to_string())?.stop(&raw);
        if let Err(error) = stopped {
            let _ = std::fs::remove_file(&raw);
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        Ok::<_, String>((raw, path))
    })();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    restore_output(&app);
    #[cfg(target_os = "macos")]
    {
        let sounds = app
            .state::<AppSettings>()
            .lock()
            .map_err(|error| error.to_string())?
            .backend_values()
            .get("interactionSounds")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if sounds {
            mac_output::play_stop(&app);
        }
    }
    #[cfg(target_os = "windows")]
    if app
        .state::<AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values()
        .get("interactionSounds")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
    {
        win_output::play_stop();
    }
    let (raw, path) = stopped?;
    trace.event("finalize", "recording_stopped");
    if let Err(error) = normalize_wav(&raw, &path) {
        let _ = std::fs::remove_file(&raw);
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    let _ = std::fs::remove_file(&raw);
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn model_prepare(app: tauri::AppHandle) -> Result<String, String> {
    let selected = app
        .state::<AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values()
        .get("localWhisperModel")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(model::DEFAULT_MODEL)
        .to_owned();
    let path = model::prepare(&app, &selected).await?;
    app.state::<AppWhisper>()
        .lock()
        .await
        .ensure_loaded(&app, &path)
        .await?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn model_size(app: tauri::AppHandle) -> Result<u64, String> {
    model::total_bytes(&app)
}

async fn finish_voice(
    app: tauri::AppHandle,
    recorder: State<'_, AppRecorder>,
    settings: State<'_, AppSettings>,
    store: State<'_, AppStore>,
    mode: String,
    expected_pid: Option<u32>,
) -> Result<Option<VoiceResult>, String> {
    if !matches!(mode.as_str(), "dictate" | "translate" | "ask") {
        return Err(format!("unknown voice mode: {mode}"));
    }
    let trace = recorder
        .lock()
        .map_err(|error| error.to_string())?
        .trace
        .take()
        .ok_or("recording trace unavailable")?;
    let started = std::time::Instant::now();
    trace.event("finalize", &format!("started mode={mode}"));
    let config = settings
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values();
    let retention = config
        .get("recordingRetention")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("day30");
    let path = match recording_stop(app.clone(), recorder, retention, &trace) {
        Ok(path) => path,
        Err(error) if error == recording::EMPTY_RECORDING => {
            trace.finish("empty");
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let _temporary = RemoveFileOnDrop((retention == "off").then(|| path.clone().into()));
    let encoding_finalization_duration = started.elapsed().as_secs_f64();
    let recording = std::path::Path::new(&path);
    let file_name = recording
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("recording has no file name")?;
    let audio_file_name = (retention != "off").then(|| file_name.to_owned());
    let reader = hound::WavReader::open(recording).map_err(|error| error.to_string())?;
    let duration = reader.duration() as f64 / reader.spec().sample_rate as f64;
    drop(reader);
    trace.event(
        "finalize",
        &format!(
            "audio ready finalize_ms={} duration_ms={} retained={}",
            (encoding_finalization_duration * 1000.0) as u64,
            (duration * 1000.0) as u64,
            audio_file_name.is_some()
        ),
    );
    if duration < 0.35 {
        let _ = tokio::fs::remove_file(recording).await;
        trace.finish("too_short");
        return Ok(None);
    }
    trace.event("context", "started");
    let context_started = std::time::Instant::now();
    let context: Option<context::FocusedContext> = if config
        .get("includeContext")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
    {
        match expected_pid {
            Some(pid) => {
                #[cfg(target_os = "macos")]
                {
                    tokio::task::spawn_blocking(move || mac_context::read(pid as i32))
                        .await
                        .map_err(|error| error.to_string())?
                }
                #[cfg(target_os = "windows")]
                {
                    tokio::task::spawn_blocking(move || win_context::read(pid))
                        .await
                        .map_err(|error| error.to_string())?
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    None
                }
            }
            None => None,
        }
    } else {
        None
    };
    trace.event(
        "context",
        &format!(
            "context elapsed_ms={} available={} selected={} surrounding={}",
            context_started.elapsed().as_millis(),
            context.is_some(),
            context
                .as_ref()
                .is_some_and(|value| value.selected_text.is_some()),
            context.as_ref().is_some_and(|value| {
                value.text_before_cursor.is_some() || value.text_after_cursor.is_some()
            })
        ),
    );
    let context_description = context
        .as_ref()
        .and_then(|value| value.prompt_description());
    let language = if config
        .get("autoDetectLanguage")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        None
    } else {
        Some(
            config
                .get("language")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("zh-CN")
                .to_owned(),
        )
    };
    let mut vocabulary = store
        .lock()
        .map_err(|error| error.to_string())?
        .dictionary_terms();

    vocabulary.sort();
    vocabulary.dedup();
    let model_id = config
        .get("localWhisperModel")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(model::DEFAULT_MODEL);
    let whisper_language = language
        .as_deref()
        .map(|value| value.chars().take(2).collect::<String>());
    trace.event("model_prepare", "started");
    let transcription_started = std::time::Instant::now();
    let _ = app.emit("voice-stage", "transcribing");
    tray::set_stage(&app, "transcribing");
    let is_cloud = config
        .get("transcriptionBackend")
        .and_then(serde_json::Value::as_str)
        == Some("cloudASR");
    let backend = if is_cloud { "Cloud" } else { "Whisper" };
    let recognition_model = if is_cloud {
        config
            .get("cloudASRModel")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    } else {
        model_id
    };
    let mut cloud_usage = cloud_asr::CloudUsage::default();
    let mut cloud_request_id: Option<String> = None;
    let mut cloud_attempts: Option<u32> = None;
    let recognized = if is_cloud {
        async {
            let cloud = cloud_asr::CloudConfig::from_values(&config)
                .map_err(asr::TranscriptionError::Failure)?;
            let audio = {
                let service = app.state::<AppWhisper>();
                let mut service = service.lock().await;
                service
                    .ensure_service(&app, None)
                    .await
                    .map_err(asr::TranscriptionError::Failure)?;
                service.detect_speech(recording, &trace).await?
            };
            let result = cloud_asr::transcribe(
                &cloud,
                audio,
                whisper_language.as_deref(),
                &vocabulary,
                &trace,
            )
            .await
            .map_err(asr::TranscriptionError::Failure)?;
            cloud_usage = result.usage;
            cloud_request_id = result.request_id;
            cloud_attempts = Some(result.attempts);
            Ok(result.transcription)
        }
        .await
    } else {
        match model::prepare(&app, model_id).await {
            Ok(model) => {
                app.state::<AppWhisper>()
                    .lock()
                    .await
                    .transcribe(
                        &app,
                        recording,
                        &model,
                        whisper_language.as_deref(),
                        &vocabulary,
                        &trace,
                    )
                    .await
            }
            Err(error) => Err(asr::TranscriptionError::Failure(error)),
        }
    };
    let recognized = match recognized {
        Ok(text) => text,
        Err(asr::TranscriptionError::Empty) => {
            let _ = tokio::fs::remove_file(recording).await;
            trace.finish("no_speech");
            return Ok(None);
        }
        Err(asr::TranscriptionError::Failure(error)) => {
            trace.event(
                "asr",
                &format!(
                    "asr failed elapsed_ms={}",
                    transcription_started.elapsed().as_millis()
                ),
            );
            let entry = serde_json::json!({
                "id": trace.id(),
                "sessionId": trace.id(),
                "buildInfo": build_info::metadata(),
                "createdAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                "transcript": "",
                "refinedText": "",
                "insertedText": null,
                "duration": duration,
                "processingDuration": started.elapsed().as_secs_f64(),
                "encodingFinalizationDuration": encoding_finalization_duration,
                "transcriptionDuration": transcription_started.elapsed().as_secs_f64(),
                "modelDuration": null,
                "audioFileName": audio_file_name,
                "appName": context.as_ref().and_then(|value| value.app_name.as_deref()),
                "bundleID": context.as_ref().and_then(|value| value.bundle_id.as_deref()),
                "windowTitle": context.as_ref().and_then(|value| value.window_title.as_deref()),
                "language": language,
                "backend": backend,
                "asrProvider": if is_cloud { config.get("cloudASRProvider").cloned() } else { None },
                "recognitionModel": recognition_model,
                "textModel": null,
                "refineStyle": config.get("refineStyle").and_then(serde_json::Value::as_str).unwrap_or("clean"),
                "mode": mode,
                "error": error,
            });
            let mut store = store.lock().map_err(|error| error.to_string())?;
            store.add_history(entry)?;
            let current_retention = settings
                .lock()
                .map_err(|error| error.to_string())?
                .snapshot()
                .values["recordingRetention"]
                .as_str()
                .ok_or("invalid recording retention")?
                .to_owned();
            store.prune_recordings(&current_retention)?;
            drop(store);
            let _ = app.emit("history-changed", ());
            return Err(error);
        }
    };
    let output_language = recognized.output_language(language.as_deref());
    let transcript = transcription::simplified(&recognized.text, output_language.as_deref());
    if transcription::reject_short_prompt_echo(&transcript, &vocabulary, duration) {
        let _ = tokio::fs::remove_file(recording).await;
        trace.finish("prompt_echo");
        return Ok(None);
    }
    let transcription_duration = transcription_started.elapsed().as_secs_f64();
    trace.event(
        "asr",
        &format!(
            "asr completed elapsed_ms={} chars={}",
            (transcription_duration * 1000.0) as u64,
            transcript.chars().count()
        ),
    );
    let (rule_corrected_text, applied_rules) = if mode == "dictate" {
        app.state::<AppStore>()
            .lock()
            .map_err(|error| error.to_string())?
            .apply_rules(&transcript)
    } else {
        (transcript.clone(), Vec::new())
    };
    let model_started = std::time::Instant::now();
    let needs_model = mode != "dictate"
        || config
            .get("refineStyle")
            .and_then(serde_json::Value::as_str)
            != Some("off");
    if needs_model {
        let _ = app.emit("voice-stage", "refining");
        tray::set_stage(&app, "refining");
        #[cfg(target_os = "macos")]
        let _ = mac_floating::set(&app, "refining", &mode);
        #[cfg(target_os = "windows")]
        let _ = win_floating::set(&app, "refining", &mode);
    }
    trace.event("model", if needs_model { "started" } else { "skipped" });
    let processed = language::process(
        &rule_corrected_text,
        &mode,
        &config,
        &vocabulary,
        context_description.as_deref(),
    )
    .await;
    let model_duration = if needs_model {
        model_started.elapsed().as_secs_f64()
    } else {
        0.0
    };
    let (refined_text, error, prompt_tokens, completion_tokens) = match processed {
        Ok(result) => (
            result.text,
            None,
            result.prompt_tokens,
            result.completion_tokens,
        ),
        Err(error) if mode == "dictate" => (rule_corrected_text.clone(), Some(error), None, None),
        Err(error) => (String::new(), Some(error), None, None),
    };
    if needs_model {
        trace.event(
            "model",
            &format!(
                "model completed elapsed_ms={} chars={} failed={}",
                (model_duration * 1000.0) as u64,
                refined_text.chars().count(),
                error.is_some()
            ),
        );
    }
    let formatted_text = if mode == "dictate" {
        text_format::apply(&refined_text, &config)
    } else {
        refined_text.clone()
    };
    let history_id = trace.id().to_owned();
    trace.event("history", "saving");
    let entry = serde_json::json!({
        "id": history_id,
        "sessionId": trace.id(),
                "buildInfo": build_info::metadata(),
        "createdAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "transcript": transcript,
        "ruleCorrectedText": rule_corrected_text,
        "appliedRules": applied_rules,
        "refinedText": refined_text,
        "formattedText": formatted_text,
        "insertedText": null,
        "duration": duration,
        "processingDuration": started.elapsed().as_secs_f64(),
        "asrInputTokens": cloud_usage.input_tokens,
        "asrOutputTokens": cloud_usage.output_tokens,
        "asrBilledSeconds": cloud_usage.seconds,
        "asrRequestId": cloud_request_id,
        "asrAttempts": cloud_attempts,
        "promptTokens": prompt_tokens,
        "completionTokens": completion_tokens,
        "encodingFinalizationDuration": encoding_finalization_duration,
        "transcriptionDuration": transcription_duration,
        "modelDuration": model_duration,
        "audioFileName": audio_file_name,
        "appName": context.as_ref().and_then(|value| value.app_name.as_deref()),
        "bundleID": context.as_ref().and_then(|value| value.bundle_id.as_deref()),
        "windowTitle": context.as_ref().and_then(|value| value.window_title.as_deref()),
        "language": output_language,
        "backend": backend,
                "asrProvider": if is_cloud { config.get("cloudASRProvider").cloned() } else { None },
        "recognitionModel": recognition_model,
        "textModel": if needs_model { config.get("refineModel").cloned().unwrap_or(serde_json::Value::Null) } else { serde_json::Value::Null },
        "refineStyle": config.get("refineStyle").and_then(serde_json::Value::as_str).unwrap_or("clean"),
        "mode": mode,
        "error": error,
    });
    {
        let mut store = store.lock().map_err(|error| error.to_string())?;
        store.add_history(entry)?;
        let current_retention = settings
            .lock()
            .map_err(|error| error.to_string())?
            .snapshot()
            .values["recordingRetention"]
            .as_str()
            .ok_or("invalid recording retention")?
            .to_owned();
        store.prune_recordings(&current_retention)?;
    }
    if let Some(error) = error {
        if mode != "dictate" {
            let _ = app.emit("history-changed", ());
            return Err(error);
        }
    }
    trace.event(
        "result",
        &format!(
            "completed mode={mode} chars={} elapsed_ms={}",
            refined_text.chars().count(),
            started.elapsed().as_millis()
        ),
    );
    Ok(Some(VoiceResult {
        transcript,
        refined_text: formatted_text,
        inserted: false,
        history_id,
        trace,
        correction_marker: context
            .as_ref()
            .and_then(|value| value.text_before_cursor.as_deref())
            .filter(|before| !before.is_empty())
            .map(|before| {
                before
                    .chars()
                    .rev()
                    .take(60)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect()
            }),
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if std::env::args().any(|arg| arg == "--build-info") {
        println!("{}", build_info::metadata());
        return;
    }
    #[cfg(feature = "prompt-eval")]
    if std::env::args().any(|arg| arg == "--prompt-eval") {
        prompt_eval::run();
        return;
    }
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let has_visible_window = app
                .webview_windows()
                .values()
                .any(|window| window.is_visible().is_ok_and(|visible| visible));
            if has_visible_window {
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ));
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    builder
        .setup(|app| {
            let inflight = app.path().app_cache_dir()?.join("inflight-recordings");
            if let Err(error) = std::fs::remove_dir_all(inflight) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(error.into());
                }
            }
            let mut store = Store::open(app.handle())
                .map_err(|error| startup_data_error(app.handle(), error))?;
            let settings_directory = app.path().app_data_dir()?;
            let log = diagnostics::AppLog::open(settings_directory.clone())
                .map_err(std::io::Error::other)?;
            app.manage(log);
            #[cfg(target_os = "macos")]
            {
                let output = mac_output::OutputMute::new(settings_directory.clone());
                if let Err(error) = output.restore() {
                    eprintln!("output mute recovery failed: {error}");
                }
                app.manage(Mutex::new(output));
            }
            #[cfg(target_os = "windows")]
            {
                let output = win_output::OutputMute::new(settings_directory.clone());
                if let Err(error) = output.restore() {
                    eprintln!("output mute recovery failed: {error}");
                }
                app.manage(Mutex::new(output));
            }
            let settings = Settings::open(settings_directory)
                .map_err(|error| startup_data_error(app.handle(), error))?;
            let startup_values = settings.snapshot().values;
            #[cfg(target_os = "macos")]
            let initial_setup_done = startup_values
                .get("hasShownInitialSetup")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let preload_model = startup_values
                .get("localWhisperModel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(model::DEFAULT_MODEL)
                .to_owned();
            let launch_at_login = startup_values
                .get("launchAtLogin")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            store
                .prune_recordings(
                    startup_values
                        .get("recordingRetention")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("day30"),
                )
                .map_err(std::io::Error::other)?;
            app.manage(Mutex::new(store));
            app.manage(Mutex::new(Recorder::new()));
            app.manage(Mutex::new(settings));
            let autostart = app.autolaunch();
            let autostart_result = autostart.is_enabled().and_then(|enabled| {
                if launch_at_login && !enabled {
                    autostart.enable()
                } else if !launch_at_login && enabled {
                    autostart.disable()
                } else {
                    Ok(())
                }
            });
            if let Err(error) = autostart_result {
                diagnostics::log(
                    app.handle(),
                    "startup",
                    &format!("autostart sync failed: {error}"),
                );
            }
            app.manage(Mutex::new("dictate".to_owned()));
            app.manage(model::PreparationLock::new(()));
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            app.manage(correction::CorrectionGeneration::new(0));
            app.manage(Mutex::new(tray::TrayState::default()));
            app.manage(tokio::sync::Mutex::new(asr::WhisperService::new()));
            #[cfg(target_os = "macos")]
            app.manage(Mutex::new(None::<mac_hotkey::MacHotkeyController>));
            #[cfg(target_os = "windows")]
            app.manage(Mutex::new(None::<win_hotkey::WinHotkeyController>));
            #[cfg(target_os = "macos")]
            app.manage(Mutex::new(None::<mac_ask::PendingAsk>));
            #[cfg(target_os = "windows")]
            app.manage(Mutex::new(None::<win_ask::PendingAsk>));
            #[cfg(target_os = "macos")]
            app.manage(Mutex::new(mac_floating::FloatingState::default()));
            #[cfg(target_os = "macos")]
            if let Err(error) = mac_floating::prepare(app.handle()) {
                diagnostics::log(
                    app.handle(),
                    "floating",
                    &format!("native panel preparation failed: {error}"),
                );
            }
            #[cfg(target_os = "windows")]
            app.manage(Mutex::new(win_floating::FloatingState::default()));
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Err(error) = hotkey_start(app.handle().clone()) {
                eprintln!("hotkey startup failed: {error}");
            }
            let preload_app = app.handle().clone();
            if startup_values["transcriptionBackend"] != "cloudASR"
                && model::has_cached_candidate(&preload_app, &preload_model)
                    .map_err(std::io::Error::other)?
            {
                tauri::async_runtime::spawn(async move {
                    diagnostics::log(&preload_app, "asr", "startup prewarm started");
                    match model::prepare(&preload_app, &preload_model).await {
                        Ok(path) => {
                            let whisper_state = preload_app.state::<AppWhisper>();
                            let mut whisper = whisper_state.lock().await;
                            let still_selected = preload_app
                                .state::<AppSettings>()
                                .lock()
                                .is_ok_and(|settings| {
                                    settings.snapshot().values["transcriptionBackend"] != "cloudASR"
                                        && settings
                                            .backend_values()
                                            .get("localWhisperModel")
                                            .and_then(serde_json::Value::as_str)
                                            == Some(preload_model.as_str())
                                });
                            if still_selected {
                                if let Err(error) = whisper.ensure_loaded(&preload_app, &path).await
                                {
                                    eprintln!("ASR preload failed: {error}");
                                    diagnostics::log(
                                        &preload_app,
                                        "asr",
                                        &format!("startup prewarm failed: {error}"),
                                    );
                                } else {
                                    diagnostics::log(&preload_app, "asr", "startup prewarm ready");
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("ASR model validation failed: {error}");
                            diagnostics::log(
                                &preload_app,
                                "asr",
                                &format!("model validation failed: {error}"),
                            );
                        }
                    }
                });
            }
            tray::install(app.handle()).map_err(std::io::Error::other)?;
            #[cfg(target_os = "macos")]
            {
                let startup_app = app.handle().clone();
                let permission_status = read_permissions();
                app.run_on_main_thread(move || match permission_status {
                    Ok(status) if !permissions_ready(&status) => {
                        if let Err(error) =
                            show_settings_window(startup_app.clone(), Some("permission".into()))
                        {
                            diagnostics::log(
                                &startup_app,
                                "window",
                                &format!("permission settings failed: {error}"),
                            );
                        }
                    }
                    Ok(status) if !initial_setup_done => {
                        if let Err(error) = complete_initial_setup(&startup_app, &status) {
                            diagnostics::log(
                                &startup_app,
                                "startup",
                                &format!("setup completion failed: {error}"),
                            );
                        }
                        if let Err(error) = tray::show_main(&startup_app) {
                            diagnostics::log(
                                &startup_app,
                                "window",
                                &format!("initial main window failed: {error}"),
                            );
                        }
                    }
                    Err(error) => {
                        diagnostics::log(
                            &startup_app,
                            "permissions",
                            &format!("startup status failed: {error}"),
                        );
                        if let Err(error) = tray::show_main(&startup_app) {
                            diagnostics::log(
                                &startup_app,
                                "window",
                                &format!("fallback main window failed: {error}"),
                            );
                        }
                    }
                    _ => {}
                })?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                let startup_app = app.handle().clone();
                app.run_on_main_thread(move || {
                    if let Err(error) = tray::show_main(&startup_app) {
                        diagnostics::log(
                            &startup_app,
                            "window",
                            &format!("startup main window failed: {error}"),
                        );
                    }
                })?;
            }
            #[cfg(target_os = "macos")]
            {
                let permission_app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut previous = read_permissions().ok();
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        let Ok(current) = read_permissions() else {
                            continue;
                        };
                        if previous.as_ref() != Some(&current) {
                            previous = Some(current.clone());
                            if let Err(error) = complete_initial_setup(&permission_app, &current) {
                                diagnostics::log(
                                    &permission_app,
                                    "permissions",
                                    &format!("setup completion failed: {error}"),
                                );
                            }
                            let _ = permission_app.emit("permissions-changed", current);
                            let _ = tray::refresh(&permission_app);
                            diagnostics::log(&permission_app, "permissions", "status changed");
                        }
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.label() == "ask-result" {
                    #[cfg(target_os = "macos")]
                    let _ = mac_ask::ask_result_dismiss(window.app_handle().clone(), None);
                    #[cfg(target_os = "windows")]
                    let _ = win_ask::ask_result_dismiss(window.app_handle().clone(), None);
                }
                let _ = window.hide();
            }
            #[cfg(target_os = "macos")]
            if window.label() == "tray-panel" && matches!(event, tauri::WindowEvent::Focused(false))
            {
                let panel = window.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if panel.is_focused().is_ok_and(|focused| !focused) {
                        let _ = panel.hide();
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_metadata,
            contact_developer,
            app_exit,
            open_workspace,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            tray::tray_snapshot,
            data_commands::load_snapshot,
            data_commands::statistics_get,
            data_commands::dictionary_add,
            data_commands::dictionary_update,
            data_commands::dictionary_import,
            data_commands::dictionary_accept,
            data_commands::rule_save,
            data_commands::rule_delete,
            data_commands::history_correct,
            data_commands::dictionary_delete,
            data_commands::history_delete,
            data_commands::history_clear,
            data_commands::history_audio,
            recording_start,
            recording_bytes,
            audio_input_devices,
            cloud_asr_test,
            model_prepare,
            model_size,
            diagnostics::diagnostics_export,
            diagnostics::diagnostics_reveal_log,
            settings_get,
            settings_set,
            open_settings,
            selected_mode,
            select_mode,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            hotkey_start,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            hotkey_stop,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            hotkey_status,
            #[cfg(target_os = "macos")]
            permissions_status,
            #[cfg(target_os = "macos")]
            microphone_request,
            #[cfg(target_os = "macos")]
            permissions_open,
            #[cfg(target_os = "macos")]
            mac_ask::ask_result_get,
            #[cfg(target_os = "macos")]
            mac_ask::ask_result_copy,
            #[cfg(target_os = "macos")]
            mac_ask::ask_result_presented,
            #[cfg(target_os = "macos")]
            mac_ask::ask_result_dismiss,
            #[cfg(target_os = "macos")]
            mac_ask::ask_result_write,
            #[cfg(target_os = "macos")]
            mac_floating::floating_get,
            #[cfg(target_os = "windows")]
            win_ask::ask_result_get,
            #[cfg(target_os = "windows")]
            win_ask::ask_result_copy,
            #[cfg(target_os = "windows")]
            win_ask::ask_result_presented,
            #[cfg(target_os = "windows")]
            win_ask::ask_result_dismiss,
            #[cfg(target_os = "windows")]
            win_ask::ask_result_write,
            #[cfg(target_os = "windows")]
            win_floating::floating_get,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Inksay Tauri")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if matches!(
                &event,
                tauri::RunEvent::Reopen {
                    has_visible_windows: false,
                    ..
                }
            ) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            if matches!(event, tauri::RunEvent::Exit) {
                diagnostics::log(app, "app", "exiting");
                if let Ok(mut recorder) = app.state::<AppRecorder>().lock() {
                    if let Some(trace) = recorder.trace.take() {
                        trace.finish("app_exit");
                    }
                }
                #[cfg(target_os = "macos")]
                if let Ok(pending) = app.state::<mac_ask::PendingAskState>().lock() {
                    if let Some(result) = pending.as_ref() {
                        result.trace.finish("app_exit");
                    }
                }
                #[cfg(target_os = "windows")]
                if let Ok(pending) = app.state::<win_ask::PendingAskState>().lock() {
                    if let Some(result) = pending.as_ref() {
                        result.trace.finish("app_exit");
                    }
                }
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                restore_output(app);
                if let Ok(mut whisper) = app.state::<AppWhisper>().try_lock() {
                    whisper.stop();
                }
            }
        });
}

use serde::Serialize;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatingState {
    pub stage: String,
    pub mode: String,
}

impl Default for FloatingState {
    fn default() -> Self {
        Self {
            stage: "idle".into(),
            mode: "dictate".into(),
        }
    }
}

pub type FloatingStore = Mutex<FloatingState>;

#[tauri::command]
pub fn floating_get(state: tauri::State<'_, FloatingStore>) -> Result<FloatingState, String> {
    Ok(state.lock().map_err(|error| error.to_string())?.clone())
}

pub fn set(app: &tauri::AppHandle, stage: &str, mode: &str) -> Result<(), String> {
    let state = FloatingState {
        stage: stage.to_owned(),
        mode: mode.to_owned(),
    };
    *app.state::<FloatingStore>()
        .lock()
        .map_err(|error| error.to_string())? = state.clone();
    if stage == "idle" {
        if let Some(window) = app.get_webview_window("floating") {
            window.hide().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let window = if let Some(window) = app.get_webview_window("floating") {
        window
    } else {
        tauri::WebviewWindowBuilder::new(
            app,
            "floating",
            tauri::WebviewUrl::App("index.html?view=floating".into()),
        )
        .title("录音状态")
        .inner_size(190.0, 40.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .visible(false)
        .shadow(false)
        .build()
        .map_err(|error| error.to_string())?
    };
    position(&window)?;
    let _ = app.emit_to("floating", "floating-stage", state);
    window.show().map_err(|error| error.to_string())
}

fn position(window: &tauri::WebviewWindow) -> Result<(), String> {
    let foreground = unsafe { GetForegroundWindow() };
    let monitor = unsafe { MonitorFromWindow(HWND(foreground.0), MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return Err("could not read active screen".into());
    }
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let margin = (14.0 * window.scale_factor().map_err(|error| error.to_string())?) as i32;
    let x = (info.rcWork.left + info.rcWork.right - size.width as i32) / 2;
    let y = info.rcWork.bottom - size.height as i32 - margin;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

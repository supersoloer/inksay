use crate::{mac_focus, mac_paste, AppSettings, AppStore};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[derive(Clone)]
pub struct PendingAsk {
    pub text: String,
    pub history_id: String,
    pub trace: crate::diagnostics::SessionTrace,
    pub target_pid: i32,
}

pub type PendingAskState = Mutex<Option<PendingAsk>>;

pub fn show(app: &tauri::AppHandle, result: PendingAsk) -> Result<(), String> {
    let snapshot = crate::AskResultSnapshot {
        session_id: result.trace.id().to_owned(),
        text: result.text.clone(),
    };
    result.trace.event("result", "presenting");
    if let Some(previous) = app
        .state::<PendingAskState>()
        .lock()
        .map_err(|error| error.to_string())?
        .replace(result)
    {
        previous.trace.finish("superseded");
    }
    if let Some(window) = app.get_webview_window("ask-result") {
        present(&window)?;
        if let Err(error) = app.emit_to("ask-result", "ask-result-changed", snapshot) {
            let _ = window.hide();
            return Err(error.to_string());
        }
        return Ok(());
    }
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "ask-result",
        tauri::WebviewUrl::App("index.html?view=ask-result".into()),
    )
    .title("询问结果")
    .inner_size(430.0, 170.0)
    .min_inner_size(430.0, 170.0)
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .build()
    .map_err(|error| error.to_string())?;
    present(&window)
}

fn present(window: &tauri::WebviewWindow) -> Result<(), String> {
    window
        .with_webview(|webview| unsafe {
            use objc2::MainThreadMarker;
            use objc2_app_kit::{
                NSColor, NSFloatingWindowLevel, NSScreen, NSWindow, NSWindowCollectionBehavior,
            };
            use objc2_foundation::NSPoint;
            use objc2_web_kit::WKWebView;

            let native: &NSWindow = &*webview.ns_window().cast();
            let view: &WKWebView = &*webview.inner().cast();
            let clear = NSColor::clearColor();
            native.setOpaque(false);
            native.setBackgroundColor(Some(&clear));
            view.setUnderPageBackgroundColor(Some(&clear));
            native.setHidesOnDeactivate(false);
            native.setLevel(NSFloatingWindowLevel);
            native.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            if let Some(marker) = MainThreadMarker::new() {
                if let Some(screen) = NSScreen::mainScreen(marker) {
                    let visible = screen.visibleFrame();
                    native.setFrameOrigin(NSPoint::new(
                        visible.origin.x + (visible.size.width - native.frame().size.width) / 2.0,
                        visible.origin.y + 120.0,
                    ));
                }
            }
            native.orderFrontRegardless();
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn ask_result_get(app: tauri::AppHandle) -> Result<Option<crate::AskResultSnapshot>, String> {
    Ok(app
        .state::<PendingAskState>()
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
        .map(|result| crate::AskResultSnapshot {
            session_id: result.trace.id().to_owned(),
            text: result.text.clone(),
        }))
}

#[tauri::command]
pub fn ask_result_dismiss(app: tauri::AppHandle, session_id: Option<String>) -> Result<(), String> {
    let result = {
        let state = app.state::<PendingAskState>();
        let mut pending = state.lock().map_err(|error| error.to_string())?;
        if session_id.as_ref().is_some_and(|id| {
            pending
                .as_ref()
                .is_none_or(|result| result.trace.id() != id)
        }) {
            return Ok(());
        }
        pending.take()
    };
    if let Some(result) = result {
        result.trace.finish("dismissed");
    }
    if let Some(window) = app.get_webview_window("ask-result") {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn ask_result_write(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let result = app
        .state::<PendingAskState>()
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or("没有待写入的询问结果")?;
    if result.trace.id() != session_id {
        return Err("没有待写入的询问结果".into());
    }
    let pasteboard_only = app
        .state::<AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .backend_values()
        .get("injectionStrategy")
        .and_then(serde_json::Value::as_str)
        == Some("pasteboardOnly");
    let text = result.text.clone();
    let target_pid = result.target_pid;
    let insertion_started = std::time::Instant::now();
    result.trace.event("insertion", "started source=ask_result");
    let insertion = tauri::async_runtime::spawn_blocking(move || {
        mac_focus::activate_pid(target_pid)?;
        if !pasteboard_only && mac_focus::insert_selected_text(&text, target_pid)? {
            return Ok(());
        }
        mac_paste::paste_text(&text, target_pid)
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|value| value);
    if let Err(error) = insertion {
        result.trace.event("insertion", "failed source=ask_result");
        return Err(error);
    }
    result.trace.event(
        "insertion",
        &format!(
            "completed elapsed_ms={}",
            insertion_started.elapsed().as_millis()
        ),
    );
    let updated = app
        .state::<AppStore>()
        .lock()
        .map_err(|error| error.to_string())
        .and_then(|mut store| {
            store.update_insertion(
                &result.history_id,
                Some(&result.text),
                None,
                Some(insertion_started.elapsed().as_secs_f64()),
            )
        });
    result.trace.finish(if updated.is_ok() {
        "completed"
    } else {
        "history_failed"
    });
    let dismissed = ask_result_dismiss(app.clone(), Some(session_id));
    if let Err(error) = updated {
        let _ = app.emit("voice-error", error.clone());
        dismissed?;
        return Err(error);
    }
    dismissed?;
    let _ = app.emit("history-changed", ());
    Ok(())
}

#[tauri::command]
pub fn ask_result_copy(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let result = app
        .state::<PendingAskState>()
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .ok_or("没有待写入的询问结果")?;
    if result.trace.id() != session_id {
        return Err("没有待写入的询问结果".into());
    }
    match app.clipboard().write_text(&result.text) {
        Ok(()) => {
            result.trace.event("result", "copied");
            Ok(())
        }
        Err(error) => {
            result.trace.event("result", "copy_failed");
            Err(error.to_string())
        }
    }
}

#[tauri::command]
pub fn ask_result_presented(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    let state = app.state::<PendingAskState>();
    let pending = state.lock().map_err(|error| error.to_string())?;
    if let Some(result) = pending
        .as_ref()
        .filter(|result| result.trace.id() == session_id)
    {
        result.trace.event(
            "result",
            &format!("rendered chars={}", result.text.chars().count()),
        );
    }
    Ok(())
}

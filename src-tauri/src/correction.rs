use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{Emitter, Manager};

pub type CorrectionGeneration = AtomicU64;

pub fn cancel(app: &tauri::AppHandle) {
    app.state::<CorrectionGeneration>()
        .fetch_add(1, Ordering::Relaxed);
}

pub fn schedule(
    app: tauri::AppHandle,
    inserted: String,
    marker: Option<String>,
    expected_pid: u32,
) {
    let generation = app
        .state::<CorrectionGeneration>()
        .fetch_add(1, Ordering::Relaxed)
        + 1;
    let Some(marker) = marker else { return };
    if marker.is_empty() || inserted.chars().count() > 240 {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut previous: Option<String> = None;
        for _ in 0..8 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if app.state::<CorrectionGeneration>().load(Ordering::Relaxed) != generation {
                return;
            }
            let current = tokio::task::spawn_blocking(move || {
                if focused_pid()? != expected_pid {
                    return None;
                }
                let context = read_context(expected_pid)?;
                if focused_pid()? != expected_pid {
                    return None;
                }
                Some(context)
            })
            .await
            .ok()
            .flatten();
            if app.state::<CorrectionGeneration>().load(Ordering::Relaxed) != generation {
                return;
            }
            let Some(context) = current else {
                return;
            };
            // Selecting a word is part of editing; wait for the replacement to settle.
            if context
                .selected_text
                .as_ref()
                .is_some_and(|s| !s.is_empty())
            {
                previous = None;
                continue;
            }
            let Some(before) = context.text_before_cursor else {
                return;
            };
            if before.matches(&marker).count() != 1 {
                return;
            }
            let edited = before.rsplit_once(&marker).unwrap().1.to_owned();
            if edited == inserted {
                previous = None;
                continue;
            }
            if previous.as_ref() != Some(&edited) {
                previous = Some(edited);
                continue;
            }
            let added = app
                .state::<crate::AppStore>()
                .lock()
                .map_err(|error| error.to_string())
                .and_then(|mut store| store.suggest_correction(&inserted, &edited));
            match added {
                Ok(true) => {
                    let _ = app.emit("dictionary-changed", ());
                    return;
                }
                Err(_) => crate::diagnostics::log(&app, "dictionary", "suggestion save failed"),
                _ => {}
            }
        }
    });
}

#[cfg(target_os = "macos")]
fn focused_pid() -> Option<u32> {
    crate::mac_focus::frontmost_pid().ok().map(|pid| pid as u32)
}

#[cfg(target_os = "windows")]
fn focused_pid() -> Option<u32> {
    crate::win_focus::focused_pid().ok()
}

#[cfg(target_os = "macos")]
fn read_context(expected_pid: u32) -> Option<crate::context::FocusedContext> {
    crate::mac_context::read(expected_pid as i32)
}

#[cfg(target_os = "windows")]
fn read_context(expected_pid: u32) -> Option<crate::context::FocusedContext> {
    crate::win_context::read(expected_pid)
}

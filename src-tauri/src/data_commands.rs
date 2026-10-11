use crate::storage::Snapshot;
use crate::{statistics, AppStore};
use base64::Engine;
use tauri::{Emitter, State};

#[tauri::command]
pub(crate) fn statistics_get(
    state: State<'_, AppStore>,
) -> Result<statistics::StatisticsSnapshot, String> {
    Ok(state
        .lock()
        .map_err(|error| error.to_string())?
        .statistics())
}

#[tauri::command]
pub(crate) fn load_snapshot(state: State<'_, AppStore>) -> Result<Snapshot, String> {
    Ok(state.lock().map_err(|error| error.to_string())?.snapshot())
}

#[tauri::command]
pub(crate) fn dictionary_add(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    term: String,
) -> Result<Option<Snapshot>, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    let next = store.add_dictionary(&term)?.then(|| store.snapshot());
    drop(store);
    if next.is_some() {
        let _ = app.emit("dictionary-changed", ());
    }
    Ok(next)
}

#[tauri::command]
pub(crate) fn dictionary_update(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    id: String,
    term: String,
) -> Result<Option<Snapshot>, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    let next = store
        .update_dictionary(&id, &term)?
        .then(|| store.snapshot());
    drop(store);
    if next.is_some() {
        let _ = app.emit("dictionary-changed", ());
    }
    Ok(next)
}

#[tauri::command]
pub(crate) fn dictionary_import(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    content: String,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    let added = store.import_dictionary(&content)?;
    let next = store.snapshot();
    drop(store);
    if added > 0 {
        let _ = app.emit("dictionary-changed", ());
    }
    Ok(next)
}

#[tauri::command]
pub(crate) fn rule_save(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    id: Option<String>,
    from: String,
    to: String,
    enabled: bool,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    store.save_rule(id.as_deref(), &from, &to, enabled)?;
    let next = store.snapshot();
    drop(store);
    let _ = app.emit("dictionary-changed", ());
    Ok(next)
}

#[tauri::command]
pub(crate) fn rule_delete(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    id: String,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    store.delete_rule(&id)?;
    let next = store.snapshot();
    drop(store);
    let _ = app.emit("dictionary-changed", ());
    Ok(next)
}

#[tauri::command]
pub(crate) fn history_correct(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    id: String,
    text: String,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    store.correct_history(&id, &text)?;
    let next = store.snapshot();
    drop(store);
    let _ = app.emit("history-changed", ());
    let _ = app.emit("dictionary-changed", ());
    Ok(next)
}

#[tauri::command]
pub(crate) fn dictionary_accept(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    id: String,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    store.accept_suggestion(&id)?;
    let next = store.snapshot();
    drop(store);
    let _ = app.emit("dictionary-changed", ());
    Ok(next)
}

#[tauri::command]
pub(crate) fn dictionary_delete(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    id: String,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    store.delete_dictionary(&id)?;
    let next = store.snapshot();
    drop(store);
    let _ = app.emit("dictionary-changed", ());
    Ok(next)
}

#[tauri::command]
pub(crate) fn history_delete(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
    id: String,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    store.delete_history(&id)?;
    let next = store.snapshot();
    drop(store);
    let _ = app.emit("history-changed", ());
    Ok(next)
}

#[tauri::command]
pub(crate) fn history_clear(
    app: tauri::AppHandle,
    state: State<'_, AppStore>,
) -> Result<Snapshot, String> {
    let mut store = state.lock().map_err(|error| error.to_string())?;
    store.clear_history()?;
    let next = store.snapshot();
    drop(store);
    let _ = app.emit("history-changed", ());
    Ok(next)
}

#[tauri::command]
pub(crate) fn history_audio(state: State<'_, AppStore>, id: String) -> Result<String, String> {
    let (bytes, mime_type) = state
        .lock()
        .map_err(|error| error.to_string())?
        .audio_bytes(&id)?;
    Ok(format!(
        "data:{mime_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

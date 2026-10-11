use crate::hotkey::{Action, HotkeyMachine, Key, ProcessingPhase, TriggerMode};
use crate::{
    finish_voice, recording_start, win_ask, win_floating, win_focus, win_input, AppRecorder,
    AppSelection, AppSettings, AppStore,
};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LSHIFT, VK_RCONTROL, VK_RSHIFT, VK_SHIFT, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, PeekMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, KBDLLHOOKSTRUCT_FLAGS, LLKHF_EXTENDED, LLKHF_INJECTED,
    MSG, PM_NOREMOVE, PM_REMOVE, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

pub struct RawKeyEvent {
    pub key: Key,
    pub down: bool,
}

static HOOK_SENDER: Mutex<Option<mpsc::Sender<RawKeyEvent>>> = Mutex::new(None);
static TRIGGER: AtomicU8 = AtomicU8::new(0);

pub struct WinHotkeyTap {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WinHotkeyTap {
    pub fn start(trigger: &str) -> Result<(Self, mpsc::Receiver<RawKeyEvent>), String> {
        let trigger_code = match trigger {
            "rightControl" => 1,
            "leftControl" => 2,
            _ => return Err(format!("unsupported Windows hotkey trigger: {trigger}")),
        };
        let (sender, receiver) = mpsc::channel();
        let mut slot = HOOK_SENDER.lock().map_err(|error| error.to_string())?;
        if slot.is_some() {
            return Err("keyboard hook is already active".into());
        }
        *slot = Some(sender);
        TRIGGER.store(trigger_code, Ordering::Relaxed);
        drop(slot);

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready, startup) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let hook = match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) }
            {
                Ok(hook) => hook,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            let mut message = MSG::default();
            let _ = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) };
            let _ = ready.send(Ok(()));
            while !thread_stop.load(Ordering::Relaxed) {
                while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        });
        match startup.recv() {
            Ok(Ok(())) => Ok((
                Self {
                    stop,
                    thread: Some(thread),
                },
                receiver,
            )),
            Ok(Err(error)) => {
                let _ = thread.join();
                HOOK_SENDER
                    .lock()
                    .map_err(|error| error.to_string())?
                    .take();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                HOOK_SENDER
                    .lock()
                    .map_err(|error| error.to_string())?
                    .take();
                Err(error.to_string())
            }
        }
    }
}

impl Drop for WinHotkeyTap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Ok(mut sender) = HOOK_SENDER.lock() {
            *sender = None;
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if !event.flags.contains(LLKHF_INJECTED) {
            let down = wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN;
            let up = wparam.0 as u32 == WM_KEYUP || wparam.0 as u32 == WM_SYSKEYUP;
            if down || up {
                if let Some(key) =
                    map_key(event.vkCode, event.flags, TRIGGER.load(Ordering::Relaxed))
                        .or_else(|| down.then_some(Key::Other))
                {
                    if let Ok(sender) = HOOK_SENDER.lock() {
                        if let Some(sender) = sender.as_ref() {
                            let _ = sender.send(RawKeyEvent { key, down });
                        }
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn map_key(code: u32, flags: KBDLLHOOKSTRUCT_FLAGS, trigger: u8) -> Option<Key> {
    let is_control =
        code == VK_CONTROL.0 as u32 || code == VK_LCONTROL.0 as u32 || code == VK_RCONTROL.0 as u32;
    let right = code == VK_RCONTROL.0 as u32
        || (code == VK_CONTROL.0 as u32 && flags.contains(LLKHF_EXTENDED));
    let left = code == VK_LCONTROL.0 as u32
        || (code == VK_CONTROL.0 as u32 && !flags.contains(LLKHF_EXTENDED));
    if is_control && ((trigger == 1 && right) || (trigger == 2 && left)) {
        Some(Key::Trigger)
    } else if code == VK_SHIFT.0 as u32 || code == VK_LSHIFT.0 as u32 || code == VK_RSHIFT.0 as u32
    {
        Some(Key::Shift)
    } else if code == VK_SPACE.0 as u32 {
        Some(Key::Space)
    } else if code == VK_ESCAPE.0 as u32 {
        Some(Key::Escape)
    } else {
        None
    }
}

pub struct WinHotkeyController {
    tap: Option<WinHotkeyTap>,
    worker: Option<JoinHandle<()>>,
}

impl WinHotkeyController {
    pub fn start(app: tauri::AppHandle, trigger: &str, mode: TriggerMode) -> Result<Self, String> {
        let (tap, receiver) = WinHotkeyTap::start(trigger)?;
        let worker = std::thread::spawn(move || {
            let mut machine = HotkeyMachine::new(mode);
            let mut active_mode: Option<String> = None;
            let mut active_target: Option<(isize, u32)> = None;
            let processing = Arc::new(AtomicBool::new(false));
            let phase = Arc::new(ProcessingPhase::new());
            let mut active_trace: Option<crate::diagnostics::SessionTrace> = None;
            let mut processing_task: Option<tauri::async_runtime::JoinHandle<()>> = None;
            let mut last_level_emit = Instant::now();
            loop {
                let action = match receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(event)
                        if event.down
                            && event.key == Key::Escape
                            && processing.load(Ordering::SeqCst) =>
                    {
                        phase.try_cancel().then_some(Action::Cancel)
                    }
                    Ok(event) if event.down => machine.key_down(event.key, Instant::now()),
                    Ok(event) => machine.key_up(event.key),
                    Err(mpsc::RecvTimeoutError::Timeout) => machine.tick(Instant::now()),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let auto_stop = if active_mode.is_some() {
                    let status = app
                        .state::<AppRecorder>()
                        .lock()
                        .ok()
                        .and_then(|mut recorder| recorder.poll());
                    if let Some((level, should_stop)) = status {
                        if last_level_emit.elapsed() >= Duration::from_millis(50) {
                            let _ = app.emit("voice-level", level);
                            last_level_emit = Instant::now();
                        }
                        should_stop
                    } else {
                        false
                    }
                } else {
                    false
                };
                let action =
                    if auto_stop && !matches!(action, Some(Action::Cancel | Action::Finish)) {
                        machine.reset();
                        Some(Action::Finish)
                    } else {
                        action
                    };
                let Some(action) = action else { continue };
                match action {
                    Action::BeginDictate | Action::BeginTranslate | Action::BeginAsk => {
                        if processing.load(Ordering::Relaxed) {
                            machine.reset();
                            continue;
                        }
                        crate::correction::cancel(&app);
                        let target = match win_focus::focused_window() {
                            Ok(target) => target,
                            Err(error) => {
                                machine.reset();
                                let _ = app.emit("voice-error", error);
                                crate::tray::set_stage(&app, "failed");
                                continue;
                            }
                        };
                        let selected = match action {
                            Action::BeginDictate => app
                                .state::<AppSelection>()
                                .lock()
                                .map(|mode| mode.clone())
                                .map_err(|error| error.to_string()),
                            Action::BeginTranslate => Ok("translate".to_owned()),
                            Action::BeginAsk => Ok("ask".to_owned()),
                            _ => unreachable!(),
                        };
                        let started = selected.and_then(|selected| {
                            recording_start(
                                app.clone(),
                                app.state::<AppRecorder>(),
                                app.state::<AppSettings>(),
                            )?;
                            Ok(selected)
                        });
                        let selected = match started {
                            Ok(selected) => selected,
                            Err(error) => {
                                machine.reset();
                                let _ = app.emit("voice-error", error);
                                crate::tray::set_stage(&app, "failed");
                                continue;
                            }
                        };
                        active_target = Some(target);
                        active_trace = app
                            .state::<AppRecorder>()
                            .lock()
                            .ok()
                            .and_then(|recorder| recorder.trace.clone());
                        active_mode = Some(selected.clone());
                        if let Some(trace) = &active_trace {
                            trace.event(
                                "recording",
                                &format!("mode={selected} target_pid={}", target.1),
                            );
                        }
                        let _ = app.emit("voice-stage", "recording");
                        crate::tray::set_stage(&app, "recording");
                        let _ = win_floating::set(&app, "recording", &selected);
                    }
                    Action::Finish => {
                        if let Some(trace) = &active_trace {
                            trace.event(
                                "finalize",
                                if auto_stop {
                                    "stop_requested source=automatic"
                                } else {
                                    "stop_requested source=hotkey"
                                },
                            );
                        }
                        let Some(mode) = active_mode.take() else {
                            continue;
                        };
                        let target = active_target.take();
                        processing.store(true, Ordering::Relaxed);
                        phase.start();
                        let task_app = app.clone();
                        let processing = processing.clone();
                        let task_phase = phase.clone();
                        let trace = active_trace.clone();
                        let _ = app.emit("voice-stage", "transcribing");
                        crate::tray::set_stage(&app, "transcribing");
                        let _ = win_floating::set(&app, "transcribing", &mode);
                        processing_task = Some(tauri::async_runtime::spawn(async move {
                            let result = finish_voice(
                                task_app.clone(),
                                task_app.state::<AppRecorder>(),
                                task_app.state::<AppSettings>(),
                                task_app.state::<AppStore>(),
                                mode.clone(),
                                target.map(|(_, pid)| pid),
                            )
                            .await;
                            if !task_phase.try_commit() {
                                return;
                            }
                            let mut succeeded = result.is_ok();
                            match result {
                                Ok(None) => {
                                    let _ = task_app.emit("voice-stage", "idle");
                                }
                                Ok(Some(value)) if mode == "ask" => {
                                    crate::tray::set_result(&task_app, &value.refined_text);
                                    let shown = target
                                        .ok_or("target window unavailable".to_owned())
                                        .and_then(|(window, pid)| {
                                            win_ask::show(
                                                &task_app,
                                                win_ask::PendingAsk {
                                                    text: value.refined_text.clone(),
                                                    history_id: value.history_id.clone(),
                                                    trace: value.trace.clone(),
                                                    target_window: window,
                                                    target_pid: pid,
                                                },
                                            )
                                        });
                                    match shown {
                                        Ok(()) => {
                                            let _ = task_app.emit("voice-complete", value);
                                        }
                                        Err(error) => {
                                            succeeded = false;
                                            let _ = task_app.emit("voice-error", error);
                                            let _ = task_app.emit("history-changed", ());
                                        }
                                    }
                                }
                                Ok(Some(mut value)) => {
                                    if let Some((window, pid)) = target {
                                        let _ = task_app.emit("voice-stage", "injecting");
                                        crate::tray::set_stage(&task_app, "injecting");
                                        let _ = win_floating::set(&task_app, "injecting", &mode);
                                        if let Some(trace) = &trace {
                                            trace.event("insertion", "started");
                                        }
                                        let insertion_started = Instant::now();
                                        let text = value.refined_text.clone();
                                        let paste_only = task_app
                                            .state::<AppSettings>()
                                            .lock()
                                            .map_err(|error| error.to_string())
                                            .map(|settings| {
                                                settings
                                                    .backend_values()
                                                    .get("injectionStrategy")
                                                    .and_then(serde_json::Value::as_str)
                                                    == Some("pasteboardOnly")
                                            });
                                        let insertion_app = task_app.clone();
                                        let inserted = tauri::async_runtime::spawn_blocking(move || {
                                            if win_focus::focused_window()? != (window, pid) {
                                                return Err("target window focus changed during processing".into());
                                            }
                                            win_input::insert_text(&insertion_app, &text, pid, paste_only?)
                                        }).await.map_err(|error| error.to_string()).and_then(|result| result);
                                        value.inserted = inserted.is_ok();
                                        let insertion_error = inserted.err();
                                        let insertion_duration =
                                            insertion_started.elapsed().as_secs_f64();
                                        if let Some(trace) = &trace {
                                            trace.event(
                                                "insertion",
                                                &format!(
                                                    "insertion completed elapsed_ms={} success={}",
                                                    (insertion_duration * 1000.0) as u64,
                                                    value.inserted
                                                ),
                                            );
                                        }
                                        if insertion_error.is_some() {
                                            succeeded = false;
                                        }
                                        let updated = task_app
                                            .state::<AppStore>()
                                            .lock()
                                            .map_err(|error| error.to_string())
                                            .and_then(|mut store| {
                                                store.update_insertion(
                                                    &value.history_id,
                                                    value
                                                        .inserted
                                                        .then_some(value.refined_text.as_str()),
                                                    insertion_error.as_deref(),
                                                    Some(insertion_duration),
                                                )
                                            });
                                        if let Err(error) = updated {
                                            succeeded = false;
                                            let _ = task_app.emit("voice-error", error);
                                        }
                                        if let Some(error) = insertion_error {
                                            let _ = task_app.emit("voice-error", error);
                                        }
                                        if value.inserted && mode == "dictate" {
                                            crate::remember_successful_dictation(
                                                &task_app,
                                                &value.refined_text,
                                            );
                                            crate::correction::schedule(
                                                task_app.clone(),
                                                value.refined_text.clone(),
                                                value.correction_marker.clone(),
                                                pid,
                                            );
                                        }
                                    }
                                    if !value.inserted {
                                        value.trace.finish("not_inserted");
                                    }
                                    crate::tray::set_result(&task_app, &value.refined_text);
                                    if succeeded {
                                        let _ = task_app.emit("voice-complete", value);
                                    } else {
                                        let _ = task_app.emit("history-changed", ());
                                    }
                                }
                                Err(error) => {
                                    succeeded = false;
                                    let _ = task_app.emit("voice-error", error);
                                }
                            }
                            if mode != "ask" || !succeeded {
                                if let Some(trace) = &trace {
                                    trace.finish(if succeeded { "completed" } else { "failed" });
                                }
                            }
                            if succeeded {
                                crate::tray::set_stage(&task_app, "idle");
                                let _ = win_floating::set(&task_app, "idle", &mode);
                            } else {
                                crate::tray::set_stage(&task_app, "failed");
                                if task_app
                                    .state::<AppSettings>()
                                    .lock()
                                    .is_ok_and(|settings| {
                                        settings
                                            .backend_values()
                                            .get("interactionSounds")
                                            .and_then(serde_json::Value::as_bool)
                                            .unwrap_or(true)
                                    })
                                {
                                    crate::win_output::play_failure();
                                }
                                let _ = win_floating::set(&task_app, "failed", &mode);
                                let hide_app = task_app.clone();
                                tauri::async_runtime::spawn(async move {
                                    tokio::time::sleep(Duration::from_secs(3)).await;
                                    if hide_app
                                        .state::<win_floating::FloatingStore>()
                                        .lock()
                                        .is_ok_and(|state| state.stage == "failed")
                                    {
                                        let _ = win_floating::set(&hide_app, "idle", &mode);
                                        crate::tray::set_stage(&hide_app, "idle");
                                        let _ = hide_app.emit("voice-stage", "idle");
                                    }
                                });
                            }
                            processing.store(false, Ordering::Relaxed);
                            task_phase.finish();
                        }));
                    }
                    Action::Cancel => {
                        if let Some(trace) = active_trace.take() {
                            trace.finish("cancelled");
                        }
                        machine.reset();
                        if processing.load(Ordering::SeqCst) {
                            if let Some(task) = processing_task.take() {
                                task.abort();
                                let cancelled_app = app.clone();
                                let cancelled_processing = processing.clone();
                                tauri::async_runtime::spawn(async move {
                                    let _ = task.await;
                                    crate::tray::set_stage(&cancelled_app, "idle");
                                    let _ = win_floating::set(&cancelled_app, "idle", "dictate");
                                    let _ = cancelled_app.emit("voice-stage", "idle");
                                    cancelled_processing.store(false, Ordering::SeqCst);
                                });
                            } else {
                                processing.store(false, Ordering::SeqCst);
                            }
                        }
                        if let Ok(mut recorder) = app.state::<AppRecorder>().lock() {
                            recorder.cancel();
                        }
                        crate::restore_output(&app);
                        active_mode = None;
                        active_target = None;
                        let _ = app.emit("voice-stage", "idle");
                        crate::tray::set_stage(&app, "idle");
                        let _ = win_floating::set(&app, "idle", "dictate");
                    }
                }
            }
        });
        Ok(Self {
            tap: Some(tap),
            worker: Some(worker),
        })
    }
}

impl Drop for WinHotkeyController {
    fn drop(&mut self) {
        self.tap.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

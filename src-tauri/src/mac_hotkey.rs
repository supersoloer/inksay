use crate::hotkey::{Action, HotkeyMachine, Key, ProcessingPhase, TriggerMode};
use crate::{
    finish_voice, mac_focus, mac_paste, recording_start, AppRecorder, AppSelection, AppSettings,
    AppStore,
};
use core_foundation::runloop::{
    kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunInMode,
};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

pub fn has_permission() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

pub fn request_permission() {
    if !has_permission() {
        unsafe { CGRequestListenEventAccess() };
    }
}

pub struct RawKeyEvent {
    pub key: Key,
    pub down: bool,
}

pub struct MacHotkeyTap {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MacHotkeyTap {
    pub fn start(trigger: &str) -> Result<(Self, mpsc::Receiver<RawKeyEvent>), String> {
        if !has_permission() {
            return Err("input monitoring permission is not granted".into());
        }
        let (key_code, trigger_flag) = match trigger {
            "rightOption" => (61, CGEventFlags::CGEventFlagAlternate),
            "leftOption" => (58, CGEventFlags::CGEventFlagAlternate),
            "function" => (63, CGEventFlags::CGEventFlagSecondaryFn),
            _ => return Err(format!("unknown hotkey trigger: {trigger}")),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (events, receiver) = mpsc::channel();
        let (ready, startup) = mpsc::sync_channel(1);
        let stop_for_thread = stop.clone();
        let thread = std::thread::spawn(move || {
            let disabled = Arc::new(AtomicBool::new(false));
            let disabled_for_callback = disabled.clone();
            let tap = CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![
                    CGEventType::FlagsChanged,
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                ],
                move |_, event_type, event| {
                    if matches!(
                        event_type,
                        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
                    ) {
                        disabled_for_callback.store(true, Ordering::Relaxed);
                        return CallbackResult::Keep;
                    }
                    let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                    let mapped = match event_type {
                        CGEventType::FlagsChanged if code == key_code => Some(RawKeyEvent {
                            key: Key::Trigger,
                            down: event.get_flags().contains(trigger_flag),
                        }),
                        CGEventType::FlagsChanged if code == 56 || code == 60 => {
                            Some(RawKeyEvent {
                                key: Key::Shift,
                                down: event.get_flags().contains(CGEventFlags::CGEventFlagShift),
                            })
                        }
                        CGEventType::KeyDown if code == 49 => Some(RawKeyEvent {
                            key: Key::Space,
                            down: true,
                        }),
                        CGEventType::KeyDown if code == 53 => Some(RawKeyEvent {
                            key: Key::Escape,
                            down: true,
                        }),
                        CGEventType::KeyDown => Some(RawKeyEvent {
                            key: Key::Other,
                            down: true,
                        }),
                        _ => None,
                    };
                    if let Some(event) = mapped {
                        let _ = events.send(event);
                    }
                    CallbackResult::Keep
                },
            );
            let Ok(tap) = tap else {
                let _ = ready.send(Err("could not create keyboard event tap".to_owned()));
                return;
            };
            let Ok(source) = tap.mach_port().create_runloop_source(0) else {
                let _ = ready.send(Err("could not create event tap run loop source".to_owned()));
                return;
            };
            CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
            tap.enable();
            let _ = ready.send(Ok(()));
            while !stop_for_thread.load(Ordering::Relaxed) {
                unsafe {
                    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 0);
                }
                if disabled.swap(false, Ordering::Relaxed) {
                    tap.enable();
                }
            }
        });
        startup.recv().map_err(|error| error.to_string())??;
        Ok((
            Self {
                stop,
                thread: Some(thread),
            },
            receiver,
        ))
    }
}

impl Drop for MacHotkeyTap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct MacHotkeyController {
    tap: Option<MacHotkeyTap>,
    worker: Option<JoinHandle<()>>,
}

impl MacHotkeyController {
    pub fn start(app: tauri::AppHandle, trigger: &str, mode: TriggerMode) -> Result<Self, String> {
        let (tap, receiver) = MacHotkeyTap::start(trigger)?;
        let worker = std::thread::spawn(move || {
            let mut machine = HotkeyMachine::new(mode);
            let mut active_mode = None;
            let mut active_target_pid = None;
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
                        let target_pid = match mac_focus::frontmost_pid() {
                            Ok(pid) => pid,
                            Err(error) => {
                                machine.reset();
                                let _ = app.emit("voice-error", error);
                                crate::tray::set_stage(&app, "failed");
                                continue;
                            }
                        };
                        let result = recording_start(
                            app.clone(),
                            app.state::<AppRecorder>(),
                            app.state::<AppSettings>(),
                        );
                        if let Err(error) = result {
                            machine.reset();
                            let _ = app.emit("voice-error", error);
                            crate::tray::set_stage(&app, "failed");
                            continue;
                        }
                        active_mode = Some(match action {
                            Action::BeginDictate => match app.state::<AppSelection>().lock() {
                                Ok(selection) => selection.clone(),
                                Err(error) => {
                                    if let Ok(mut recorder) = app.state::<AppRecorder>().lock() {
                                        recorder.cancel();
                                    }
                                    crate::restore_output(&app);
                                    machine.reset();
                                    let _ = app.emit("voice-error", error.to_string());
                                    crate::tray::set_stage(&app, "failed");
                                    continue;
                                }
                            },
                            Action::BeginTranslate => "translate".to_owned(),
                            Action::BeginAsk => "ask".to_owned(),
                            _ => unreachable!(),
                        });
                        active_target_pid = Some(target_pid);
                        active_trace = app
                            .state::<AppRecorder>()
                            .lock()
                            .ok()
                            .and_then(|recorder| recorder.trace.clone());
                        if let (Some(trace), Some(mode)) = (&active_trace, &active_mode) {
                            trace.event(
                                "recording",
                                &format!("mode={mode} target_pid={target_pid}"),
                            );
                        }
                        let _ = app.emit("voice-stage", "recording");
                        crate::tray::set_stage(&app, "recording");
                        if let Some(mode) = active_mode.as_deref() {
                            if let Err(error) = crate::mac_floating::set(&app, "recording", mode) {
                                eprintln!("floating bar failed: {error}");
                            }
                        }
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
                        let target_pid = active_target_pid.take();
                        processing.store(true, Ordering::Relaxed);
                        phase.start();
                        let task_app = app.clone();
                        let processing = processing.clone();
                        let task_phase = phase.clone();
                        let trace = active_trace.clone();
                        let _ = app.emit("voice-stage", "transcribing");
                        crate::tray::set_stage(&app, "transcribing");
                        let _ = crate::mac_floating::set(&app, "transcribing", &mode);
                        processing_task = Some(tauri::async_runtime::spawn(async move {
                            let result = finish_voice(
                                task_app.clone(),
                                task_app.state::<AppRecorder>(),
                                task_app.state::<AppSettings>(),
                                task_app.state::<AppStore>(),
                                mode.to_owned(),
                                target_pid.map(|pid| pid as u32),
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
                                    let shown = target_pid
                                        .ok_or("找不到当前输入焦点".to_owned())
                                        .and_then(|pid| {
                                            crate::mac_ask::show(
                                                &task_app,
                                                crate::mac_ask::PendingAsk {
                                                    text: value.refined_text.clone(),
                                                    history_id: value.history_id.clone(),
                                                    trace: value.trace.clone(),
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
                                    if let Some(pid) = target_pid {
                                        let _ = task_app.emit("voice-stage", "injecting");
                                        crate::tray::set_stage(&task_app, "injecting");
                                        let _ =
                                            crate::mac_floating::set(&task_app, "injecting", &mode);
                                        if let Some(trace) = &trace {
                                            trace.event("insertion", "started");
                                        }
                                        let insertion_started = Instant::now();
                                        let strategy = task_app
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
                                        let direct = match strategy {
                                            Ok(true) => Ok(false),
                                            Ok(false) => mac_focus::insert_selected_text(
                                                &value.refined_text,
                                                pid,
                                            ),
                                            Err(error) => Err(error),
                                        };
                                        let insertion = match direct {
                                            Ok(true) => Ok(true),
                                            Ok(false) => {
                                                let text = value.refined_text.clone();
                                                tauri::async_runtime::spawn_blocking(move || {
                                                    mac_paste::paste_text(&text, pid)
                                                })
                                                .await
                                                .map_err(|error| error.to_string())
                                                .and_then(|result| result)
                                                .map(|_| true)
                                            }
                                            Err(error) => Err(error),
                                        };
                                        let insertion_error = match insertion {
                                            Ok(true) => {
                                                value.inserted = true;
                                                None
                                            }
                                            Ok(false) => Some("文本注入失败，可能是缺少辅助功能权限，或目标应用不接受程序化输入".to_owned()),
                                            Err(error) => Some(error),
                                        };
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
                                        let update = task_app
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
                                        if let Err(error) = update {
                                            succeeded = false;
                                            let _ = task_app.emit("voice-error", error);
                                        }
                                        if let Some(error) = insertion_error {
                                            succeeded = false;
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
                                                pid as u32,
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
                                let _ = crate::mac_floating::set(&task_app, "idle", &mode);
                            } else {
                                crate::tray::set_stage(&task_app, "failed");
                                let sounds =
                                    task_app
                                        .state::<AppSettings>()
                                        .lock()
                                        .is_ok_and(|settings| {
                                            settings
                                                .backend_values()
                                                .get("interactionSounds")
                                                .and_then(serde_json::Value::as_bool)
                                                .unwrap_or(true)
                                        });
                                if sounds {
                                    crate::mac_output::play_failure(&task_app);
                                }
                                let _ = crate::mac_floating::set(&task_app, "failed", &mode);
                                let hide_app = task_app.clone();
                                tauri::async_runtime::spawn(async move {
                                    tokio::time::sleep(Duration::from_secs(3)).await;
                                    if hide_app
                                        .state::<crate::mac_floating::FloatingStore>()
                                        .lock()
                                        .is_ok_and(|state| state.stage == "failed")
                                    {
                                        let _ = crate::mac_floating::set(&hide_app, "idle", &mode);
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
                                    let _ =
                                        crate::mac_floating::set(&cancelled_app, "idle", "dictate");
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
                        active_target_pid = None;
                        let _ = app.emit("voice-stage", "idle");
                        crate::tray::set_stage(&app, "idle");
                        let _ = crate::mac_floating::set(&app, "idle", "dictate");
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

impl Drop for MacHotkeyController {
    fn drop(&mut self) {
        self.tap.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

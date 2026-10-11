use serde::Serialize;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};

tauri_panel! {
    panel!(RecordingPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatingState {
    pub stage: String,
    mode: String,
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

pub fn prepare(app: &tauri::AppHandle) -> Result<(), String> {
    if app.get_webview_panel("floating").is_ok() {
        return Ok(());
    }
    use objc2_app_kit::NSWindowCollectionBehavior;

    let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::CanJoinAllApplications
        | NSWindowCollectionBehavior::FullScreenAuxiliary
        | NSWindowCollectionBehavior::Stationary;
    PanelBuilder::<_, RecordingPanel>::new(app, "floating")
        .url(tauri::WebviewUrl::App("index.html?view=floating".into()))
        .title("录音状态")
        .size(tauri::Size::Logical(tauri::LogicalSize::new(190.0, 40.0)))
        .resizable(false)
        .floating(true)
        .level(PanelLevel::ScreenSaver)
        .add_style_mask(StyleMask::empty().nonactivating_panel())
        .collection_behavior(CollectionBehavior::from_raw(behavior))
        .ignores_mouse_events(true)
        .hides_on_deactivate(false)
        .has_shadow(false)
        .transparent(true)
        .with_window(|window| {
            window
                .transparent(true)
                .decorations(false)
                .skip_taskbar(true)
                .focusable(false)
                .visible(false)
        })
        .build()
        .map_err(|error| error.to_string())?;
    app.get_webview_window("floating")
        .ok_or("recording panel webview is unavailable")?
        .with_webview(|webview| unsafe {
            use objc2_app_kit::NSColor;
            use objc2_web_kit::WKWebView;

            let view: &WKWebView = &*webview.inner().cast();
            view.setUnderPageBackgroundColor(Some(&NSColor::clearColor()));
        })
        .map_err(|error| error.to_string())?;
    crate::diagnostics::log(app, "floating", "native panel prepared");
    Ok(())
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
        if let Ok(panel) = app.get_webview_panel("floating") {
            let window = app
                .get_webview_window("floating")
                .ok_or("recording panel webview is unavailable")?;
            window
                .with_webview(move |_| panel.hide())
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    prepare(app)?;
    let panel = app
        .get_webview_panel("floating")
        .map_err(|error| format!("recording panel is unavailable: {error:?}"))?;
    let window = app
        .get_webview_window("floating")
        .ok_or("recording panel webview is unavailable")?;
    window
        .with_webview(|webview| unsafe {
            use objc2_app_kit::NSWindow;
            let native: &NSWindow = &*webview.ns_window().cast();
            position_below_active_application(native);
        })
        .map_err(|error| error.to_string())?;
    let _ = app.emit_to("floating", "floating-stage", state);
    window
        .with_webview(move |_| panel.show())
        .map_err(|error| error.to_string())
}

fn position_below_active_application(window: &objc2_app_kit::NSWindow) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    use objc2_foundation::NSPoint;

    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    let screen = active_application_screen(marker).or_else(|| NSScreen::mainScreen(marker));
    if let Some(screen) = screen {
        let visible = screen.visibleFrame();
        let width = window.frame().size.width;
        window.setFrameOrigin(NSPoint::new(
            visible.origin.x + (visible.size.width - width) / 2.0,
            visible.origin.y + 14.0,
        ));
    }
}

fn active_application_screen(
    marker: objc2::MainThreadMarker,
) -> Option<objc2::rc::Retained<objc2_app_kit::NSScreen>> {
    use core_foundation::array::CFArrayGetValueAtIndex;
    use core_foundation::base::{CFGetTypeID, CFTypeRef, TCFType};
    use core_foundation::dictionary::{
        CFDictionaryGetTypeID, CFDictionaryGetValue, CFDictionaryRef,
    };
    use core_foundation::number::{CFNumber, CFNumberRef};
    use core_foundation::string::CFString;
    use core_graphics::display::CGDisplay;
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowListOptionOnScreenOnly,
    };
    use objc2_app_kit::{NSScreen, NSWorkspace};

    let pid = NSWorkspace::sharedWorkspace()
        .frontmostApplication()?
        .processIdentifier();
    let windows = copy_window_info(kCGWindowListOptionOnScreenOnly, kCGNullWindowID)?;
    let owner_key = CFString::new("kCGWindowOwnerPID");
    let layer_key = CFString::new("kCGWindowLayer");
    let bounds_key = CFString::new("kCGWindowBounds");
    let x_key = CFString::new("X");
    let y_key = CFString::new("Y");
    let width_key = CFString::new("Width");
    let height_key = CFString::new("Height");
    let primary_height = CGDisplay::main().bounds().size.height;

    for index in 0..windows.len() {
        let info = unsafe { CFArrayGetValueAtIndex(windows.as_concrete_TypeRef(), index as isize) };
        if info.is_null()
            || unsafe { CFGetTypeID(info as CFTypeRef) } != unsafe { CFDictionaryGetTypeID() }
        {
            continue;
        }
        let info = info as CFDictionaryRef;
        let number = |info: CFDictionaryRef, key: &CFString| -> Option<f64> {
            let raw = unsafe { CFDictionaryGetValue(info, key.as_concrete_TypeRef() as _) };
            if raw.is_null() || unsafe { CFGetTypeID(raw as CFTypeRef) } != CFNumber::type_id() {
                return None;
            }
            unsafe { CFNumber::wrap_under_get_rule(raw as CFNumberRef) }.to_f64()
        };
        if number(info, &owner_key).map(|value| value as i32) != Some(pid)
            || number(info, &layer_key) != Some(0.0)
        {
            continue;
        }
        let raw_bounds =
            unsafe { CFDictionaryGetValue(info, bounds_key.as_concrete_TypeRef() as _) };
        if raw_bounds.is_null()
            || unsafe { CFGetTypeID(raw_bounds as CFTypeRef) } != unsafe { CFDictionaryGetTypeID() }
        {
            continue;
        }
        let info = raw_bounds as CFDictionaryRef;
        let (Some(x), Some(y), Some(width), Some(height)) = (
            number(info, &x_key),
            number(info, &y_key),
            number(info, &width_key),
            number(info, &height_key),
        ) else {
            continue;
        };
        if width <= 120.0 || height <= 80.0 {
            continue;
        }
        let center_x = x + width / 2.0;
        let center_y = primary_height - y - height / 2.0;
        for screen in NSScreen::screens(marker) {
            let frame = screen.frame();
            if center_x >= frame.origin.x
                && center_x < frame.origin.x + frame.size.width
                && center_y >= frame.origin.y
                && center_y < frame.origin.y + frame.size.height
            {
                return Some(screen.clone());
            }
        }
        break;
    }
    None
}

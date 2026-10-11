use serde::Serialize;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
#[cfg(target_os = "macos")]
use tauri::menu::{MenuItemKind, PredefinedMenuItem, WINDOW_SUBMENU_ID};
use tauri::tray::TrayIconBuilder;
#[cfg(target_os = "macos")]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::Manager;
use tauri_plugin_clipboard_manager::ClipboardExt;

const TRAY_ID: &str = "inksay-tray";

#[derive(Clone)]
pub struct TrayState {
    stage: String,
    last_result: Option<String>,
}

impl Default for TrayState {
    fn default() -> Self {
        Self {
            stage: "idle".into(),
            last_result: None,
        }
    }
}

pub type AppTrayState = Mutex<TrayState>;

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayPanelSnapshot {
    stage: String,
    last_result: Option<String>,
    selected_mode: String,
    hotkey_active: bool,
    trigger: String,
    missing_permissions: Vec<&'static str>,
}

#[cfg(target_os = "macos")]
fn show_panel(app: &tauri::AppHandle, rect: tauri::Rect) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window("tray-panel") {
        window
    } else {
        let window = tauri::WebviewWindowBuilder::new(
            app,
            "tray-panel",
            tauri::WebviewUrl::App("index.html?view=tray-panel".into()),
        )
        .title("InkSay")
        .inner_size(300.0, 390.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
        window
            .with_webview(|webview| unsafe {
                use objc2_app_kit::{NSColor, NSFloatingWindowLevel, NSWindow};
                use objc2_web_kit::WKWebView;

                let native: &NSWindow = &*webview.ns_window().cast();
                let view: &WKWebView = &*webview.inner().cast();
                let clear = NSColor::clearColor();
                native.setOpaque(false);
                native.setBackgroundColor(Some(&clear));
                native.setHidesOnDeactivate(false);
                native.setLevel(NSFloatingWindowLevel);
                view.setUnderPageBackgroundColor(Some(&clear));
            })
            .map_err(|error| error.to_string())?;
        window
    };
    if window.is_visible().map_err(|error| error.to_string())? {
        return window.hide().map_err(|error| error.to_string());
    }
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let icon = rect.position.to_physical::<i32>(scale);
    let icon_size = rect.size.to_physical::<u32>(scale);
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let center_x = icon.x as f64 + icon_size.width as f64 / 2.0;
    let center_y = icon.y as f64 + icon_size.height as f64 / 2.0;
    let mut x = (center_x - size.width as f64 / 2.0) as i32;
    let mut y = icon.y + icon_size.height as i32 + 6;
    if let Ok(monitors) = app.available_monitors() {
        if let Some(monitor) = monitors.into_iter().find(|monitor| {
            let origin = monitor.position();
            let extent = monitor.size();
            center_x >= origin.x as f64
                && center_x < (origin.x + extent.width as i32) as f64
                && center_y >= origin.y as f64
                && center_y < (origin.y + extent.height as i32) as f64
        }) {
            let origin = monitor.position();
            let extent = monitor.size();
            let left = origin.x + 8;
            let right = (origin.x + extent.width as i32 - size.width as i32 - 8).max(left);
            x = x.clamp(left, right);
            y = y.min(origin.y + extent.height as i32 - size.height as i32 - 8);
        }
    }
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tauri::command]
pub fn tray_snapshot(app: tauri::AppHandle) -> Result<TrayPanelSnapshot, String> {
    let state = app
        .state::<AppTrayState>()
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let selected_mode = app
        .state::<crate::AppSelection>()
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let hotkey_active = app
        .state::<crate::AppHotkey>()
        .lock()
        .map_err(|error| error.to_string())?
        .is_some();
    let trigger = app
        .state::<crate::AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .snapshot()
        .values
        .get("hotkeyTrigger")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("rightOption")
        .to_owned();
    #[cfg(target_os = "macos")]
    let missing_permissions = {
        let status = crate::read_permissions()?;
        let mut missing = Vec::new();
        if status.microphone != "authorized" {
            missing.push("麦克风");
        }
        if !status.accessibility {
            missing.push("辅助功能");
        }
        if !status.input_monitoring {
            missing.push("输入监控");
        }
        missing
    };
    #[cfg(not(target_os = "macos"))]
    let missing_permissions = Vec::new();
    Ok(TrayPanelSnapshot {
        stage: state.stage,
        last_result: state.last_result,
        selected_mode,
        hotkey_active,
        trigger,
        missing_permissions,
    })
}

pub fn install(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    install_app_menu(app)?;
    let menu = build_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::include_image!("icons/tray.png"))
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(!cfg!(target_os = "macos"))
        .on_tray_icon_event(|tray, event| {
            #[cfg(target_os = "macos")]
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(error) = show_panel(tray.app_handle(), rect) {
                    eprintln!("tray panel failed: {error}");
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (tray, event);
        })
        .on_menu_event(|app, event| {
            crate::diagnostics::log(app, "tray", event.id.as_ref());
            #[cfg(target_os = "windows")]
            if matches!(event.id.as_ref(), "history" | "dictionary" | "settings") {
                let app = app.clone();
                let target = event.id.as_ref().to_owned();
                // WebView2 creation must not block a Windows menu callback.
                std::thread::spawn(move || {
                    let result = if target == "settings" {
                        crate::show_settings_window(app.clone(), None)
                    } else {
                        crate::open_page_window(&app, &target)
                    };
                    if let Err(error) = result { crate::diagnostics::log(&app, "window", &format!("open failed: {error}")); }
                });
                return;
            }
            match event.id.as_ref() {
                "mode-dictate" => select_mode(app, "dictate"),
                "mode-translate" => select_mode(app, "translate"),
                "mode-ask" => select_mode(app, "ask"),
                "recent-copy" => {
                    if let Ok(state) = app.state::<AppTrayState>().lock() {
                        if let Some(text) = &state.last_result {
                            let _ = app.clipboard().write_text(text);
                        }
                    }
                }
                "main" => {
                    let _ = show_main(app);
                }
                "history" => {
                    let _ = crate::open_page_window(app, "history");
                }
                "dictionary" => {
                    let _ = crate::open_page_window(app, "dictionary");
                }
                "permissions" => {
                    let _ = crate::show_settings_window(app.clone(), Some("permission".into()));
                }
                "settings" => {
                    let _ = crate::show_settings_window(app.clone(), None);
                }
                "restart-hotkey" => {
                    #[cfg(any(target_os = "macos", target_os = "windows"))]
                    {
                        let _ = crate::hotkey_stop(app.clone());
                        let _ = crate::hotkey_start(app.clone());
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_app_menu(app: &tauri::AppHandle) -> Result<(), String> {
    let menu = Menu::default(app).map_err(|error| error.to_string())?;
    let app_menu = match menu
        .items()
        .map_err(|error| error.to_string())?
        .into_iter()
        .next()
    {
        Some(MenuItemKind::Submenu(menu)) => menu,
        _ => return Err("application menu is unavailable".into()),
    };
    let settings = MenuItem::with_id(app, "app-settings", "设置…", true, Some("Cmd+,"))
        .map_err(|error| error.to_string())?;
    app_menu
        .insert(&settings, 2)
        .map_err(|error| error.to_string())?;

    let window_menu = match menu.get(WINDOW_SUBMENU_ID) {
        Some(MenuItemKind::Submenu(menu)) => menu,
        _ => return Err("window menu is unavailable".into()),
    };
    window_menu
        .append(&PredefinedMenuItem::separator(app).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    for (id, title, shortcut) in [
        ("app-main", "主窗口", "Cmd+0"),
        ("app-history", "历史记录", "Cmd+Shift+Y"),
        ("app-dictionary", "词典", "Cmd+Shift+D"),
    ] {
        let item = MenuItem::with_id(app, id, title, true, Some(shortcut))
            .map_err(|error| error.to_string())?;
        window_menu
            .append(&item)
            .map_err(|error| error.to_string())?;
    }
    app.set_menu(menu).map_err(|error| error.to_string())?;
    app.on_menu_event(|app, event| match event.id.as_ref() {
        "app-settings" => {
            let _ = crate::show_settings_window(app.clone(), None);
        }
        "app-main" => {
            let _ = show_main(app);
        }
        "app-history" => {
            let _ = crate::open_page_window(app, "history");
        }
        "app-dictionary" => {
            let _ = crate::open_page_window(app, "dictionary");
        }
        _ => {}
    });
    Ok(())
}

pub fn set_stage(app: &tauri::AppHandle, stage: &str) {
    if let Ok(mut state) = app.state::<AppTrayState>().lock() {
        state.stage = stage.to_owned();
    }
    let _ = refresh(app);
}

pub fn set_result(app: &tauri::AppHandle, text: &str) {
    if let Ok(mut state) = app.state::<AppTrayState>().lock() {
        state.last_result = (!text.is_empty()).then(|| text.to_owned());
    }
    let _ = refresh(app);
}

pub fn refresh(app: &tauri::AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            match build_menu(&handle) {
                Ok(menu) => {
                    if let Err(error) = tray.set_menu(Some(menu)) {
                        eprintln!("tray refresh failed: {error}");
                    }
                }
                Err(error) => eprintln!("tray menu failed: {error}"),
            }
        }
    })
    .map_err(|error| error.to_string())
}

fn build_menu(app: &tauri::AppHandle) -> Result<Menu<tauri::Wry>, String> {
    let state = app
        .state::<AppTrayState>()
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let selected = app
        .state::<crate::AppSelection>()
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let config = app
        .state::<crate::AppSettings>()
        .lock()
        .map_err(|error| error.to_string())?
        .snapshot()
        .values;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let hotkey_active = app
        .state::<crate::AppHotkey>()
        .lock()
        .map_err(|error| error.to_string())?
        .is_some();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let hotkey_active = false;
    let trigger = match config
        .get("hotkeyTrigger")
        .and_then(serde_json::Value::as_str)
    {
        Some("rightControl") => "右 Ctrl",
        Some("leftControl") => "左 Ctrl",
        Some("function") => "Fn 键",
        Some("leftOption") => "左 Option",
        _ => "右 Option",
    };
    let status_text = if !hotkey_active {
        "热键未生效"
    } else {
        match state.stage.as_str() {
            "recording" => "正在听…",
            "transcribing" => "识别中…",
            "refining" => "整理中…",
            "injecting" => "写入中…",
            "failed" => "出错",
            _ => "就绪",
        }
    };
    let status = MenuItem::with_id(
        app,
        "status",
        format!("{status_text}    {trigger}"),
        false,
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let dictate = CheckMenuItem::with_id(
        app,
        "mode-dictate",
        "听写",
        true,
        selected == "dictate",
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let translate = CheckMenuItem::with_id(
        app,
        "mode-translate",
        "翻译",
        true,
        selected == "translate",
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let ask = CheckMenuItem::with_id(
        app,
        "mode-ask",
        "询问",
        true,
        selected == "ask",
        None::<&str>,
    )
    .map_err(|error| error.to_string())?;
    let mut builder = MenuBuilder::new(app).item(&status);
    #[cfg(target_os = "macos")]
    {
        if let Ok(permissions) = crate::read_permissions() {
            let mut missing = Vec::new();
            if permissions.microphone != "authorized" {
                missing.push("麦克风");
            }
            if !permissions.accessibility {
                missing.push("辅助功能");
            }
            if !permissions.input_monitoring {
                missing.push("输入监控");
            }
            if !missing.is_empty() {
                let text = MenuItem::with_id(
                    app,
                    "missing",
                    format!("缺少权限：{}", missing.join("、")),
                    false,
                    None::<&str>,
                )
                .map_err(|error| error.to_string())?;
                builder = builder.item(&text).text("permissions", "去授权");
            }
        }
    }
    builder = builder
        .separator()
        .item(&dictate)
        .item(&translate)
        .item(&ask);
    if let Some(result) = state.last_result {
        let single_line = result.replace(['\n', '\r'], " ");
        let preview: String = single_line.chars().take(80).collect();
        let preview = if single_line.chars().count() > 80 {
            format!("{preview}…")
        } else {
            preview
        };
        let label = MenuItem::with_id(app, "recent-label", "最近一次", false, None::<&str>)
            .map_err(|error| error.to_string())?;
        let text = MenuItem::with_id(app, "recent-preview", preview, false, None::<&str>)
            .map_err(|error| error.to_string())?;
        builder = builder
            .separator()
            .item(&label)
            .item(&text)
            .text("recent-copy", "复制");
    }
    builder = builder
        .separator()
        .text("main", "打开主窗口")
        .text("history", "历史记录")
        .text("dictionary", "词典")
        .text("settings", "设置…");
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let restart = if hotkey_active {
            "重启热键监听"
        } else {
            "启动热键监听"
        };
        builder = builder.text("restart-hotkey", restart);
    }
    builder
        .separator()
        .text("quit", "退出 InkSay")
        .build()
        .map_err(|error| error.to_string())
}

fn select_mode(app: &tauri::AppHandle, mode: &str) {
    let _ = crate::select_mode(
        app.clone(),
        app.state::<crate::AppSelection>(),
        mode.to_owned(),
        None,
    );
}

pub fn show_main(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

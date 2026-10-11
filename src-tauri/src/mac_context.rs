use crate::context::{sanitized_page_url, FocusedContext};
use accessibility_sys::{
    kAXValueTypeCFRange, AXUIElementCopyAttributeValue, AXUIElementCreateApplication,
    AXUIElementCreateSystemWide, AXUIElementGetPid, AXUIElementGetTypeID,
    AXUIElementSetAttributeValue, AXValueGetType, AXValueGetValue,
};
use core_foundation::array::{CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex};
use core_foundation::base::{CFGetTypeID, CFRange, CFRelease, CFRetain, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use core_foundation::url::{CFURLGetString, CFURLRef, CFURL};
use objc2_app_kit::NSWorkspace;
use std::collections::HashSet;
use std::ptr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static AWAKENED: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();

pub fn read(expected_pid: i32) -> Option<FocusedContext> {
    let running = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = running.processIdentifier();
    if expected_pid != pid {
        return None;
    }
    let mut context = FocusedContext {
        app_name: running.localizedName().map(|name| name.to_string()),
        bundle_id: running.bundleIdentifier().map(|id| id.to_string()),
        ..Default::default()
    };
    context.content_format = inferred_format(context.bundle_id.as_deref(), None);
    if !crate::mac_focus::has_permission() {
        return Some(context);
    }

    let app = unsafe { AXUIElementCreateApplication(pid) } as CFTypeRef;
    let system = unsafe { AXUIElementCreateSystemWide() } as CFTypeRef;
    if app.is_null() || system.is_null() {
        if !app.is_null() {
            unsafe { CFRelease(app) }
        }
        if !system.is_null() {
            unsafe { CFRelease(system) }
        }
        return Some(context);
    }
    let mut focused = focused_element(system, app, pid);
    if focused.is_none() && pid != std::process::id() as i32 {
        awaken(app, pid);
        focused = focused_element(system, app, pid);
    }
    if focused.is_some_and(|element| {
        copy_string(element, "AXRole").as_deref() == Some("AXSecureTextField")
    }) {
        if let Some(element) = focused {
            unsafe { CFRelease(element) }
        }
        unsafe {
            CFRelease(app);
            CFRelease(system);
        }
        return Some(context);
    }

    let window = focused
        .and_then(|element| copy_element(element, "AXFocusedWindow"))
        .or_else(|| copy_element(app, "AXFocusedWindow"))
        .or_else(|| first_window(app));
    if let Some(window) = window {
        context.window_title = cleaned(copy_string(window, "AXTitle"));
        unsafe { CFRelease(window) };
    }
    if let Some(element) = focused {
        context.selected_text = cleaned(copy_string(element, "AXSelectedText"));
        if let Some(range) = selected_range(element) {
            if let Some(value) = copy_string(element, "AXValue") {
                let utf16: Vec<u16> = value.encode_utf16().collect();
                let cursor = (range.location.max(0) as usize).min(utf16.len());
                let selection = (range.length.max(0) as usize).min(utf16.len() - cursor);
                context.text_before_cursor = cleaned(Some(String::from_utf16_lossy(
                    &utf16[cursor.saturating_sub(600)..cursor],
                )));
                let after = cursor + selection;
                context.text_after_cursor = cleaned(Some(String::from_utf16_lossy(
                    &utf16[after..(after + 600).min(utf16.len())],
                )));
            }
        }
        context.page_url = web_page_url(element).and_then(|url| sanitized_page_url(&url));
        context.content_format =
            inferred_format(context.bundle_id.as_deref(), context.page_url.as_deref());
        unsafe { CFRelease(element) };
    }
    unsafe {
        CFRelease(app);
        CFRelease(system);
    }
    Some(context)
}

fn focused_element(system: CFTypeRef, app: CFTypeRef, pid: i32) -> Option<CFTypeRef> {
    if let Some(element) = copy_element(system, "AXFocusedUIElement") {
        let mut actual = 0;
        if unsafe { AXUIElementGetPid(element as _, &mut actual) } == 0 && actual == pid {
            return Some(element);
        }
        unsafe { CFRelease(element) };
    }
    copy_element(app, "AXFocusedUIElement")
}

fn awaken(app: CFTypeRef, pid: i32) {
    let cache = AWAKENED.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut seen) = cache.lock() else { return };
    if !seen.insert(pid) {
        return;
    }
    drop(seen);
    let true_value = CFBoolean::true_value();
    for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
        let attribute = CFString::new(name);
        let status = unsafe {
            AXUIElementSetAttributeValue(
                app as _,
                attribute.as_concrete_TypeRef(),
                true_value.as_CFTypeRef(),
            )
        };
        if status == 0 {
            std::thread::sleep(Duration::from_millis(120));
            break;
        }
    }
}

fn copy_attribute(element: CFTypeRef, name: &str) -> Option<CFTypeRef> {
    let attribute = CFString::new(name);
    let mut value = ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(element as _, attribute.as_concrete_TypeRef(), &mut value)
    };
    (status == 0 && !value.is_null()).then_some(value)
}

fn copy_element(element: CFTypeRef, name: &str) -> Option<CFTypeRef> {
    let value = copy_attribute(element, name)?;
    if unsafe { CFGetTypeID(value) } == unsafe { AXUIElementGetTypeID() } {
        Some(value)
    } else {
        unsafe { CFRelease(value) };
        None
    }
}

fn copy_string(element: CFTypeRef, name: &str) -> Option<String> {
    let value = copy_attribute(element, name)?;
    unsafe { crate::mac_ax_text::from_created(value) }
}

fn first_window(app: CFTypeRef) -> Option<CFTypeRef> {
    let windows = copy_attribute(app, "AXWindows")?;
    if unsafe { CFGetTypeID(windows) } != unsafe { CFArrayGetTypeID() } {
        unsafe { CFRelease(windows) };
        return None;
    }
    let first = if unsafe { CFArrayGetCount(windows as _) } > 0 {
        (unsafe { CFArrayGetValueAtIndex(windows as _, 0) }) as CFTypeRef
    } else {
        ptr::null()
    };
    let result =
        if !first.is_null() && unsafe { CFGetTypeID(first) } == unsafe { AXUIElementGetTypeID() } {
            Some(unsafe { CFRetain(first) })
        } else {
            None
        };
    unsafe { CFRelease(windows) };
    result
}

fn selected_range(element: CFTypeRef) -> Option<CFRange> {
    let value = copy_attribute(element, "AXSelectedTextRange")?;
    let range = if unsafe { AXValueGetType(value as _) } == kAXValueTypeCFRange {
        let mut range = CFRange::init(0, 0);
        unsafe { AXValueGetValue(value as _, kAXValueTypeCFRange, &mut range as *mut _ as _) }
            .then_some(range)
    } else {
        None
    };
    unsafe { CFRelease(value) };
    range
}

fn web_page_url(focused: CFTypeRef) -> Option<String> {
    let mut node = unsafe { CFRetain(focused) };
    for _ in 0..30 {
        if copy_string(node, "AXRole").as_deref() == Some("AXWebArea") {
            let result = copy_attribute(node, "AXURL").and_then(|raw| {
                if unsafe { CFGetTypeID(raw) } != CFURL::type_id() {
                    unsafe { CFRelease(raw) };
                    return None;
                }
                let url = unsafe { CFURL::wrap_under_create_rule(raw as CFURLRef) };
                let string = unsafe { CFURLGetString(url.as_concrete_TypeRef()) };
                (!string.is_null())
                    .then(|| unsafe { CFString::wrap_under_get_rule(string) }.to_string())
            });
            unsafe { CFRelease(node) };
            return result;
        }
        let parent = copy_element(node, "AXParent");
        unsafe { CFRelease(node) };
        let Some(parent) = parent else { return None };
        node = parent;
    }
    unsafe { CFRelease(node) };
    None
}

fn cleaned(value: Option<String>) -> Option<String> {
    let text: String = value?
        .chars()
        .filter(|ch| {
            *ch == '\n'
                || *ch == '\t'
                || (!ch.is_control() && *ch != '\u{feff}' && *ch != '\u{200b}')
        })
        .collect();
    (!text.is_empty()).then_some(text)
}

fn inferred_format(bundle_id: Option<&str>, url: Option<&str>) -> &'static str {
    if matches!(
        bundle_id,
        Some(
            "com.microsoft.VSCode"
                | "com.todesktop.230313mzl4w4u92"
                | "md.obsidian"
                | "com.openai.codex"
        )
    ) {
        "Markdown"
    } else if matches!(
        bundle_id,
        Some("com.apple.mail" | "com.apple.iWork.Pages" | "notion.id")
    ) || url
        .is_some_and(|url| url.contains("docs.google.com") || url.contains("notion.so"))
    {
        "富文本"
    } else {
        "纯文本"
    }
}

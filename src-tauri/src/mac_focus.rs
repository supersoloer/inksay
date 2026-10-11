use accessibility_sys::{
    kAXTrustedCheckOptionPrompt, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
    AXUIElementCopyAttributeValue, AXUIElementCreateApplication, AXUIElementCreateSystemWide,
    AXUIElementGetPid, AXUIElementGetTypeID, AXUIElementIsAttributeSettable,
    AXUIElementSetAttributeValue,
};
use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use std::ptr;

pub fn frontmost_pid() -> Result<i32, String> {
    use objc2_app_kit::NSWorkspace;

    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
        .filter(|pid| *pid > 0)
        .ok_or("找不到当前前台应用".into())
}

pub fn activate_pid(pid: i32) -> Result<(), String> {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .ok_or("目标应用已退出")?;
    if !app.activateWithOptions(NSApplicationActivationOptions::empty()) {
        return Err("无法恢复目标应用焦点".into());
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    if frontmost_pid()? != pid {
        return Err("处理期间输入焦点已切换到其他应用".into());
    }
    Ok(())
}

pub fn has_permission() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn request_permission() {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) };
}

pub fn insert_selected_text(text: &str, expected_pid: i32) -> Result<bool, String> {
    if text.is_empty() {
        return Ok(true);
    }
    if frontmost_pid()? != expected_pid {
        return Err("focused application changed during processing".into());
    }
    let element = match focused_element(expected_pid) {
        Ok(element) => element,
        Err(_) => return Ok(false),
    };
    if frontmost_pid()? != expected_pid {
        unsafe { CFRelease(element) };
        return Err("focused application changed during processing".into());
    }
    let selected = CFString::new("AXSelectedText");
    let mut settable = 0;
    let status = unsafe {
        AXUIElementIsAttributeSettable(element as _, selected.as_concrete_TypeRef(), &mut settable)
    };
    if status != 0 || settable == 0 {
        unsafe { CFRelease(element) };
        return Ok(false);
    }
    let value = CFString::new(text);
    let status = unsafe {
        AXUIElementSetAttributeValue(
            element as _,
            selected.as_concrete_TypeRef(),
            value.as_CFTypeRef(),
        )
    };
    if status != 0 {
        unsafe { CFRelease(element) };
        return Ok(false);
    }
    let attribute = CFString::new("AXValue");
    let mut current: CFTypeRef = ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(element as _, attribute.as_concrete_TypeRef(), &mut current)
    };
    unsafe { CFRelease(element) };
    if status != 0 || current.is_null() {
        return Ok(false);
    }
    Ok(unsafe { crate::mac_ax_text::from_created(current) }
        .is_some_and(|value| value.contains(text)))
}

fn focused_element(expected_pid: i32) -> Result<CFTypeRef, String> {
    if !has_permission() {
        return Err("accessibility permission is not granted".into());
    }
    let system = unsafe { AXUIElementCreateSystemWide() };
    if system.is_null() {
        return Err("could not create system-wide accessibility element".into());
    }
    let attribute = CFString::new("AXFocusedUIElement");
    let mut element: CFTypeRef = ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(system, attribute.as_concrete_TypeRef(), &mut element)
    };
    unsafe { CFRelease(system as CFTypeRef) };
    if status == 0 && !element.is_null() {
        let mut actual_pid = 0;
        if unsafe { AXUIElementGetPid(element as _, &mut actual_pid) } == 0
            && actual_pid == expected_pid
            && unsafe { CFGetTypeID(element) == AXUIElementGetTypeID() }
        {
            return Ok(element);
        }
        unsafe { CFRelease(element) };
    }
    let app = unsafe { AXUIElementCreateApplication(expected_pid) };
    if app.is_null() {
        return Err("could not create target accessibility element".into());
    }
    element = ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(app, attribute.as_concrete_TypeRef(), &mut element)
    };
    unsafe { CFRelease(app as CFTypeRef) };
    if status != 0 || element.is_null() {
        return Err(format!("could not read focused element: {status:?}"));
    }
    if unsafe { CFGetTypeID(element) != AXUIElementGetTypeID() } {
        unsafe { CFRelease(element) };
        return Err("focused object is not an accessibility element".into());
    }
    Ok(element)
}

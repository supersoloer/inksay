use core_foundation::attributed_string::{
    CFAttributedString, CFAttributedStringGetString, CFAttributedStringRef,
};
use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};

/// Takes ownership of a value returned by AXUIElementCopyAttributeValue.
pub unsafe fn from_created(value: CFTypeRef) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let type_id = unsafe { CFGetTypeID(value) };
    if type_id == CFString::type_id() {
        return Some(unsafe { CFString::wrap_under_create_rule(value as CFStringRef) }.to_string());
    }
    if type_id == CFAttributedString::type_id() {
        let attributed =
            unsafe { CFAttributedString::wrap_under_create_rule(value as CFAttributedStringRef) };
        let raw = unsafe { CFAttributedStringGetString(attributed.as_concrete_TypeRef()) };
        return (!raw.is_null()).then(|| unsafe { CFString::wrap_under_get_rule(raw) }.to_string());
    }
    unsafe { CFRelease(value) };
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::base::CFRetain;

    #[test]
    fn reads_plain_and_attributed_ax_text() {
        let plain = CFString::new("示例文字");
        let plain_copy = unsafe { CFRetain(plain.as_CFTypeRef()) };
        assert_eq!(
            unsafe { from_created(plain_copy) }.as_deref(),
            Some("示例文字")
        );

        let rich = CFAttributedString::new(&plain);
        let rich_copy = unsafe { CFRetain(rich.as_CFTypeRef()) };
        assert_eq!(
            unsafe { from_created(rich_copy) }.as_deref(),
            Some("示例文字")
        );
    }
}

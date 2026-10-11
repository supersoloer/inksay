use crate::mac_focus;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, KeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardType, NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSString};
use std::time::Duration;

type PasteboardContents = Vec<Vec<(Retained<NSPasteboardType>, Retained<NSData>)>>;

pub fn paste_text(text: &str, expected_pid: i32) -> Result<(), String> {
    if mac_focus::frontmost_pid()? != expected_pid {
        return Err("处理期间输入焦点已切换到其他应用".into());
    }
    let pasteboard = NSPasteboard::generalPasteboard();
    let original_change_count = pasteboard.changeCount();
    let contents = snapshot(&pasteboard);
    if mac_focus::frontmost_pid()? != expected_pid {
        return Err("处理期间输入焦点已切换到其他应用".into());
    }
    if pasteboard.changeCount() != original_change_count {
        return Err("文本注入失败，可能是缺少辅助功能权限，或目标应用不接受程序化输入".into());
    }
    pasteboard.clearContents();
    if !pasteboard.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString }) {
        restore(&pasteboard, contents)?;
        return Err("文本注入失败，可能是缺少辅助功能权限，或目标应用不接受程序化输入".into());
    }
    let temporary_change_count = pasteboard.changeCount();
    let result = match mac_focus::frontmost_pid() {
        Ok(pid) if pid == expected_pid => send_command_v(),
        Ok(_) => Err("处理期间输入焦点已切换到其他应用".into()),
        Err(error) => Err(error),
    };
    std::thread::sleep(Duration::from_millis(350));
    if pasteboard.changeCount() == temporary_change_count {
        restore(&pasteboard, contents)?;
    }
    result
}

fn snapshot(pasteboard: &NSPasteboard) -> PasteboardContents {
    let Some(items) = pasteboard.pasteboardItems() else {
        return Vec::new();
    };
    let mut contents = Vec::new();
    for index in 0..items.count() {
        let item = items.objectAtIndex(index);
        let mut saved = Vec::new();
        for item_type in item.types() {
            // Lazy clipboard providers may advertise a format without supplying data.
            // Preserve readable formats; an unavailable representation cannot be restored.
            if let Some(data) = item.dataForType(&item_type) {
                saved.push((item_type, data));
            }
        }
        if !saved.is_empty() {
            contents.push(saved);
        }
    }
    contents
}

fn restore(pasteboard: &NSPasteboard, contents: PasteboardContents) -> Result<(), String> {
    pasteboard.clearContents();
    if contents.is_empty() {
        return Ok(());
    }
    let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = contents
        .into_iter()
        .map(|item_contents| -> Result<_, String> {
            let item = NSPasteboardItem::new();
            for (item_type, data) in item_contents {
                if !item.setData_forType(&data, &item_type) {
                    return Err("could not restore clipboard item data".into());
                }
            }
            Ok(ProtocolObject::from_retained(item))
        })
        .collect::<Result<_, _>>()?;
    let objects = NSArray::from_retained_slice(&objects);
    if !pasteboard.writeObjects(&objects) {
        return Err("could not restore clipboard contents".into());
    }
    Ok(())
}

fn send_command_v() -> Result<(), String> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| "could not create keyboard event source")?;
    let down = CGEvent::new_keyboard_event(source.clone(), KeyCode::ANSI_V, true)
        .map_err(|_| "could not create paste key event")?;
    let up = CGEvent::new_keyboard_event(source, KeyCode::ANSI_V, false)
        .map_err(|_| "could not create paste key event")?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);
    up.post(CGEventTapLocation::HID);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::{define_class, msg_send, ClassType};
    use objc2_app_kit::NSPasteboardItemDataProvider;
    use objc2_foundation::{NSObject, NSObjectProtocol};

    define_class!(
        #[unsafe(super(NSObject))]
        struct UnavailableClipboardProvider;
        unsafe impl NSObjectProtocol for UnavailableClipboardProvider {}
        unsafe impl NSPasteboardItemDataProvider for UnavailableClipboardProvider {
            #[unsafe(method(pasteboard:item:provideDataForType:))]
            fn provide(
                &self,
                _board: Option<&NSPasteboard>,
                _item: &NSPasteboardItem,
                _kind: &NSPasteboardType,
            ) {
            }
        }
    );

    #[test]
    fn unavailable_format_does_not_discard_readable_formats() {
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();
        let item = NSPasteboardItem::new();
        let provider: Retained<UnavailableClipboardProvider> =
            unsafe { msg_send![UnavailableClipboardProvider::class(), new] };
        let missing = NSString::from_str("public.tiff");
        assert!(item.setDataProvider_forTypes(
            ProtocolObject::from_ref(&*provider),
            &NSArray::from_retained_slice(&[missing])
        ));
        assert!(
            item.setString_forType(&NSString::from_str("preserved"), unsafe {
                NSPasteboardTypeString
            })
        );
        assert!(
            pasteboard.writeObjects(&NSArray::from_slice(&[ProtocolObject::<
                dyn NSPasteboardWriting,
            >::from_ref(&*item)]))
        );
        let saved = snapshot(&pasteboard);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].len(), 1);
        restore(&pasteboard, saved).unwrap();
        assert_eq!(
            pasteboard
                .stringForType(unsafe { NSPasteboardTypeString })
                .unwrap()
                .to_string(),
            "preserved"
        );
    }

    #[test]
    fn clipboard_snapshot_restores_every_item_type() {
        let pasteboard = NSPasteboard::pasteboardWithUniqueName();
        let original = NSPasteboardItem::new();
        let custom_type = NSString::from_str("com.supersoloer.inksay.test.binary");
        assert!(
            original.setString_forType(&NSString::from_str("before"), unsafe {
                NSPasteboardTypeString
            })
        );
        assert!(original.setData_forType(&NSData::with_bytes(&[1, 2, 3]), &custom_type));
        let objects = [ProtocolObject::<dyn NSPasteboardWriting>::from_ref(
            &*original,
        )];
        assert!(pasteboard.writeObjects(&NSArray::from_slice(&objects)));

        let saved = snapshot(&pasteboard);
        pasteboard.clearContents();
        restore(&pasteboard, saved).unwrap();

        let restored = pasteboard.pasteboardItems().unwrap().objectAtIndex(0);
        assert_eq!(
            restored
                .stringForType(unsafe { NSPasteboardTypeString })
                .unwrap()
                .to_string(),
            "before"
        );
        assert_eq!(
            restored.dataForType(&custom_type).unwrap().to_vec(),
            vec![1, 2, 3]
        );
    }
}

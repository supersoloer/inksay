use crate::win_paste;
#[cfg(test)]
use crate::win_focus;
#[cfg(test)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
};

pub fn insert_text(
    app: &tauri::AppHandle,
    text: &str,
    expected_pid: u32,
    _paste_only: bool,
) -> Result<(), String> {
    // Batched Unicode SendInput was accepted but corrupted text in Windows 11 ARM Notepad.
    win_paste::paste_text(app, text, expected_pid)
}

#[cfg(test)]
fn try_unicode(text: &str, expected_pid: u32) -> Result<bool, String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    for (index, chunk) in units.chunks(100).enumerate() {
        if win_focus::focused_pid()? != expected_pid {
            return Err("focused application changed during processing".into());
        }
        let mut events = Vec::with_capacity(chunk.len() * 2);
        for unit in chunk {
            events.push(unicode_key(*unit, false));
            events.push(unicode_key(*unit, true));
        }
        let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
        if index == 0 && sent == 0 {
            return Ok(false);
        }
        if sent != events.len() as u32 {
            return Err("Windows blocked text input".into());
        }
    }
    Ok(true)
}

#[cfg(test)]
fn unicode_key(unit: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wScan: unit,
                dwFlags: if up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                ..Default::default()
            },
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "Requires a focused disposable editor and INKSAY_TEST_TARGET_PID"]
    fn unicode_input_into_external_editor() {
        let pid = std::env::var("INKSAY_TEST_TARGET_PID")
            .expect("set the PID of the disposable foreground editor")
            .parse::<u32>().unwrap();
        assert!(super::try_unicode("Inksay Windows 验收：中文 English 123。", pid).unwrap());
    }
}

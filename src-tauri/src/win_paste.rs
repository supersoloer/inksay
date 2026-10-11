use crate::win_focus;
use std::time::Duration;
use tauri::Manager;
use windows::Win32::Foundation::{
    GetLastError, GlobalFree, SetLastError, HANDLE, HWND, WIN32_ERROR,
};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, EmptyClipboard, GetClipboardSequenceNumber,
    OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    OleFlushClipboard, OleGetClipboard, OleInitialize, OleSetClipboard, OleUninitialize,
    CF_UNICODETEXT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CONTROL, VK_V,
};

pub fn paste_text(app: &tauri::AppHandle, text: &str, expected_pid: u32) -> Result<(), String> {
    let owner = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?
        .hwnd()
        .map_err(|error| error.to_string())?
        .0 as isize;
    let text = text.to_owned();
    std::thread::spawn(move || {
        unsafe { OleInitialize(None) }.map_err(|error| error.to_string())?;
        let result = paste_on_sta(owner, &text, expected_pid);
        unsafe { OleUninitialize() };
        result
    })
    .join()
    .map_err(|_| "paste worker failed".to_owned())?
}

fn paste_on_sta(owner: isize, text: &str, expected_pid: u32) -> Result<(), String> {
    if win_focus::focused_pid()? != expected_pid {
        return Err("focused application changed during processing".into());
    }
    let source_sequence = unsafe { GetClipboardSequenceNumber() };
    unsafe { SetLastError(WIN32_ERROR(0)) };
    let count = unsafe { CountClipboardFormats() };
    if count == 0 && unsafe { GetLastError() }.0 != 0 {
        return Err(windows::core::Error::from_win32().to_string());
    }
    let previous = if count == 0 {
        None
    } else {
        Some(unsafe { OleGetClipboard() }.map_err(|error| error.to_string())?)
    };
    unsafe { OpenClipboard(Some(HWND(owner as _))) }.map_err(|error| error.to_string())?;
    if unsafe { GetClipboardSequenceNumber() } != source_sequence {
        unsafe { CloseClipboard() }.map_err(|error| error.to_string())?;
        return Err("Windows blocked paste input".into());
    }
    let write = put_text(text);
    let closed = unsafe { CloseClipboard() }.map_err(|error| error.to_string());
    if let Err(error) = write.and(closed) {
        restore(previous)?;
        return Err(error);
    }

    let temporary_sequence = unsafe { GetClipboardSequenceNumber() };
    let sent = send_ctrl_v(expected_pid);
    std::thread::sleep(Duration::from_millis(350));
    if unsafe { GetClipboardSequenceNumber() } == temporary_sequence {
        restore(previous)?;
    }
    sent
}

fn put_text(text: &str) -> Result<(), String> {
    unsafe { EmptyClipboard() }.map_err(|error| error.to_string())?;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let memory =
        unsafe { GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) }.map_err(|error| error.to_string())?;
    let pointer = unsafe { GlobalLock(memory) };
    if pointer.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err("could not lock clipboard memory".into());
    }
    unsafe { std::ptr::copy_nonoverlapping(wide.as_ptr(), pointer.cast::<u16>(), wide.len()) };
    let _ = unsafe { GlobalUnlock(memory) };
    if let Err(error) = unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))) }
    {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(error.to_string());
    }
    Ok(())
}

fn restore(previous: Option<IDataObject>) -> Result<(), String> {
    if let Some(previous) = previous {
        unsafe { OleSetClipboard(&previous) }.map_err(|error| error.to_string())?;
        unsafe { OleFlushClipboard() }.map_err(|error| error.to_string())
    } else {
        unsafe { OpenClipboard(None) }.map_err(|error| error.to_string())?;
        let emptied = unsafe { EmptyClipboard() }.map_err(|error| error.to_string());
        let closed = unsafe { CloseClipboard() }.map_err(|error| error.to_string());
        emptied.and(closed)
    }
}

fn send_ctrl_v(expected_pid: u32) -> Result<(), String> {
    if win_focus::focused_pid()? != expected_pid {
        return Err("focused application changed during processing".into());
    }
    let key = |code: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: code,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                ..Default::default()
            },
        },
    };
    let events = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };
    if sent != events.len() as u32 {
        let release = [key(VK_V, true), key(VK_CONTROL, true)];
        let _ = unsafe { SendInput(&release, std::mem::size_of::<INPUT>() as i32) };
        return Err("Windows blocked paste input".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "Requires a focused disposable editor and INKSAY_TEST_TARGET_PID"]
    fn clipboard_input_into_external_editor() {
        let pid = std::env::var("INKSAY_TEST_TARGET_PID").unwrap().parse::<u32>().unwrap();
        unsafe { OleInitialize(None) }.unwrap();
        let owner = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
        let result = super::paste_on_sta(owner, "Inksay Windows 验收：中文 English 123。", pid);
        unsafe { OleUninitialize() };
        result.unwrap();
    }
    use super::{OleInitialize, OleUninitialize};
}

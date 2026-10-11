use crate::context::FocusedContext;
use crate::win_focus;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, TextPatternRangeEndpoint_End,
    TextPatternRangeEndpoint_Start, TextUnit_Character, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

pub fn read(expected_pid: u32) -> Option<FocusedContext> {
    let (handle, pid) = win_focus::focused_window().ok()?;
    if expected_pid != pid {
        return None;
    }
    let mut context = FocusedContext {
        app_name: process_name(pid),
        ..Default::default()
    };
    if context.app_name.as_deref() == Some("Code") {
        context.content_format = "Markdown";
    }
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return Some(context);
    }
    let result = read_automation(handle, pid, &mut context);
    unsafe { CoUninitialize() };
    if result.is_err() {
        return Some(context);
    }
    Some(context)
}

fn read_automation(handle: isize, pid: u32, context: &mut FocusedContext) -> Result<(), String> {
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| error.to_string())?;
    let focused = unsafe { automation.GetFocusedElement() }.map_err(|error| error.to_string())?;
    if unsafe { focused.CurrentProcessId() }.map_err(|error| error.to_string())? as u32 != pid {
        return Err("focused application changed during processing".into());
    }
    if unsafe { focused.CurrentIsPassword() }
        .map_err(|error| error.to_string())?
        .as_bool()
    {
        return Ok(());
    }
    context.window_title = window_title(handle);
    let pattern =
        unsafe { focused.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) };
    let Ok(pattern) = pattern else { return Ok(()) };
    let selected = unsafe { pattern.GetSelection() }.map_err(|error| error.to_string())?;
    if unsafe { selected.Length() }.map_err(|error| error.to_string())? == 0 {
        return Ok(());
    }
    let range = unsafe { selected.GetElement(0) }.map_err(|error| error.to_string())?;
    context.selected_text = clean(
        unsafe { range.GetText(400) }
            .ok()
            .map(|text| text.to_string()),
    );
    if let Ok(before) = unsafe { range.Clone() } {
        let _ = unsafe {
            before.MoveEndpointByRange(
                TextPatternRangeEndpoint_End,
                &range,
                TextPatternRangeEndpoint_Start,
            )
        };
        let _ = unsafe {
            before.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -600)
        };
        context.text_before_cursor = clean(
            unsafe { before.GetText(600) }
                .ok()
                .map(|text| text.to_string()),
        );
    }
    if let Ok(after) = unsafe { range.Clone() } {
        let _ = unsafe {
            after.MoveEndpointByRange(
                TextPatternRangeEndpoint_Start,
                &range,
                TextPatternRangeEndpoint_End,
            )
        };
        let _ = unsafe {
            after.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, 600)
        };
        context.text_after_cursor = clean(
            unsafe { after.GetText(600) }
                .ok()
                .map(|text| text.to_string()),
        );
    }
    Ok(())
}

fn window_title(handle: isize) -> Option<String> {
    let window = HWND(handle as _);
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return None;
    }
    let mut buffer = vec![0_u16; length as usize + 1];
    let read = unsafe { GetWindowTextW(window, &mut buffer) };
    (read > 0).then(|| String::from_utf16_lossy(&buffer[..read as usize]))
}

fn process_name(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = vec![0_u16; 32768];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
}

fn clean(value: Option<String>) -> Option<String> {
    let text: String = value?
        .chars()
        .filter(|ch| *ch == '\n' || *ch == '\t' || (!ch.is_control() && *ch != '\u{feff}'))
        .collect();
    (!text.is_empty()).then_some(text)
}

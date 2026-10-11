use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

pub fn focused_pid() -> Result<u32, String> {
    Ok(focused_window()?.1)
}

pub fn focused_window() -> Result<(isize, u32), String> {
    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return Err("no foreground window".into());
    }
    let mut pid = 0_u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 {
        return Err("could not read foreground process".into());
    }
    Ok((window.0 as isize, pid))
}

pub fn activate_window(handle: isize, expected_pid: u32) -> Result<(), String> {
    let window = HWND(handle as _);
    if !unsafe { SetForegroundWindow(window) }.as_bool() {
        return Err("could not restore target window focus".into());
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    if focused_window()? != (handle, expected_pid) {
        return Err("target window focus changed during processing".into());
    }
    Ok(())
}

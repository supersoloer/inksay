use crate::persistence::write_transient as write_json;
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;

#[repr(C)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: u32,
        data: *const c_void,
    ) -> i32;
}

const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const SCOPE_OUTPUT: u32 = u32::from_be_bytes(*b"outp");
const DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"dOut");
const DEVICE_MUTE: u32 = u32::from_be_bytes(*b"mute");

#[derive(Serialize, Deserialize)]
struct MuteState {
    device: u32,
    muted: u32,
}

pub struct OutputMute {
    lock_path: PathBuf,
}

impl OutputMute {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            lock_path: directory.join("mute-state.json"),
        }
    }

    pub fn mute(&self) -> Result<bool, String> {
        if self.lock_path.exists() {
            self.restore()?;
        }
        let Some(device) = default_output_device() else {
            return Ok(false);
        };
        let Some(original) = muted(device) else {
            return Ok(false);
        };
        if original != 0 {
            return Ok(false);
        }
        write_json(&self.lock_path, &MuteState { device, muted: 0 })?;
        if set_muted(device, 1) {
            Ok(true)
        } else {
            if muted(device) == Some(0) {
                let _ = fs::remove_file(&self.lock_path);
            }
            Err("could not mute output device".into())
        }
    }

    pub fn restore(&self) -> Result<(), String> {
        let state = match fs::read(&self.lock_path) {
            Ok(data) => {
                serde_json::from_slice::<MuteState>(&data).map_err(|error| error.to_string())?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        if !set_muted(state.device, state.muted) {
            return Err("could not restore output device mute state".into());
        }
        fs::remove_file(&self.lock_path).map_err(|error| error.to_string())
    }
}

fn default_output_device() -> Option<u32> {
    let address = PropertyAddress {
        selector: DEFAULT_OUTPUT,
        scope: SCOPE_GLOBAL,
        element: 0,
    };
    let mut device = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            1,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut device as *mut _ as _,
        )
    };
    (status == 0 && device != 0).then_some(device)
}

fn muted(device: u32) -> Option<u32> {
    let address = PropertyAddress {
        selector: DEVICE_MUTE,
        scope: SCOPE_OUTPUT,
        element: 0,
    };
    let mut value = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut value as *mut _ as _,
        )
    };
    (status == 0).then_some(value)
}

fn set_muted(device: u32, value: u32) -> bool {
    let address = PropertyAddress {
        selector: DEVICE_MUTE,
        scope: SCOPE_OUTPUT,
        element: 0,
    };
    unsafe {
        AudioObjectSetPropertyData(
            device,
            &address,
            0,
            std::ptr::null(),
            std::mem::size_of::<u32>() as u32,
            &value as *const _ as _,
        ) == 0
    }
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    fn NSBeep();
}

pub fn play_start(app: &tauri::AppHandle) {
    play_named(app, "Tink");
}

pub fn play_stop(app: &tauri::AppHandle) {
    play_named(app, "Pop");
}

pub fn play_failure(app: &tauri::AppHandle) {
    let _ = app.run_on_main_thread(|| unsafe { NSBeep() });
}

fn play_named(app: &tauri::AppHandle, name: &'static str) {
    let _ = app.run_on_main_thread(move || {
        let name = objc2_foundation::NSString::from_str(name);
        if let Some(sound) = objc2_app_kit::NSSound::soundNamed(&name) {
            sound.play();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_restore_keeps_recovery_lock() {
        let directory = std::env::temp_dir().join(format!("inksay-mute-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let output = OutputMute::new(directory.clone());
        write_json(
            &output.lock_path,
            &MuteState {
                device: 0,
                muted: 0,
            },
        )
        .unwrap();
        assert!(output.restore().is_err());
        assert!(output.lock_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}

use crate::persistence::write_transient as write_json;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONINFORMATION, MB_OK};

#[derive(Serialize, Deserialize)]
struct MuteState {
    device: String,
    muted: bool,
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
        self.restore()?;
        let lock_path = self.lock_path.clone();
        on_com_thread(move || {
            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                    .map_err(|error| error.to_string())?;
            let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) }
                .map_err(|error| error.to_string())?;
            let volume: IAudioEndpointVolume =
                unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|error| error.to_string())?;
            if unsafe { volume.GetMute() }
                .map_err(|error| error.to_string())?
                .as_bool()
            {
                return Ok(false);
            }
            let id = unsafe { device.GetId() }.map_err(|error| error.to_string())?;
            let name = unsafe { id.to_string() }.map_err(|error| error.to_string());
            unsafe { CoTaskMemFree(Some(id.0.cast())) };
            let state = MuteState {
                device: name?,
                muted: false,
            };
            write_json(&lock_path, &state)?;
            if let Err(error) = unsafe { volume.SetMute(true, std::ptr::null()) } {
                if unsafe { volume.GetMute() }.is_ok_and(|muted| !muted.as_bool()) {
                    let _ = fs::remove_file(&lock_path);
                }
                return Err(error.to_string());
            }
            Ok(true)
        })
    }

    pub fn restore(&self) -> Result<(), String> {
        let state: MuteState = match fs::read(&self.lock_path) {
            Ok(data) => serde_json::from_slice(&data).map_err(|error| error.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        let lock_path = self.lock_path.clone();
        on_com_thread(move || {
            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                    .map_err(|error| error.to_string())?;
            let wide: Vec<u16> = state
                .device
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let device: IMMDevice = unsafe { enumerator.GetDevice(PCWSTR(wide.as_ptr())) }
                .map_err(|error| error.to_string())?;
            let volume: IAudioEndpointVolume =
                unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|error| error.to_string())?;
            unsafe { volume.SetMute(state.muted, std::ptr::null()) }
                .map_err(|error| error.to_string())?;
            fs::remove_file(lock_path).map_err(|error| error.to_string())
        })
    }
}

fn on_com_thread<T: Send + 'static>(
    action: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    std::thread::spawn(move || {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| error.to_string())?;
        let result = action();
        unsafe { CoUninitialize() };
        result
    })
    .join()
    .map_err(|_| "output mute worker failed".to_owned())?
}

pub fn play_start() {
    let _ = unsafe { MessageBeep(MB_ICONINFORMATION) };
}

pub fn play_stop() {
    let _ = unsafe { MessageBeep(MB_OK) };
}

pub fn play_failure() {
    let _ = unsafe { MessageBeep(MB_ICONERROR) };
}

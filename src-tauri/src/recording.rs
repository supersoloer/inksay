use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const MAX_RECORDING_SECONDS: usize = 600;
pub const EMPTY_RECORDING: &str = "recording contains no audio";

pub struct Recorder {
    active: Option<ActiveRecording>,
    pub trace: Option<crate::diagnostics::SessionTrace>,
}

#[derive(Serialize)]
pub struct AudioInputDevice {
    pub id: String,
    pub name: String,
}

pub fn input_devices() -> Result<Vec<AudioInputDevice>, String> {
    let mut devices: Vec<AudioInputDevice> = cpal::default_host()
        .input_devices()
        .map_err(|error| error.to_string())?
        .filter_map(|device| {
            Some(AudioInputDevice {
                id: device.id().ok()?.1,
                name: device.description().ok()?.name().to_owned(),
            })
        })
        .collect();
    devices.sort_by_key(|device| device.name.to_lowercase());
    Ok(devices)
}

struct ActiveRecording {
    stream: Stream,
    samples: Arc<Mutex<Vec<i16>>>,
    overflowed: Arc<AtomicBool>,
    input_failed: Arc<AtomicBool>,
    level: Arc<std::sync::atomic::AtomicU32>,
    sample_rate: u32,
    channels: u16,
    started: Instant,
    last_loud: Instant,
    silence_seconds: f64,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            active: None,
            trace: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn start(&mut self, preferred_device_id: &str, silence_seconds: f64) -> Result<(), String> {
        if self.active.is_some() {
            return Err("recording already active".into());
        }
        let host = cpal::default_host();
        let device = if preferred_device_id.is_empty() {
            host.default_input_device().ok_or("no input device")?
        } else {
            host.input_devices()
                .map_err(|error| error.to_string())?
                .find(|device| device.id().is_ok_and(|id| id.1 == preferred_device_id))
                .ok_or("selected input device is unavailable")?
        };
        let input = device
            .default_input_config()
            .map_err(|error| error.to_string())?;
        let sample_rate = input.sample_rate();
        let channels = input.channels();
        let format = input.sample_format();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let overflowed = Arc::new(AtomicBool::new(false));
        let input_failed = Arc::new(AtomicBool::new(false));
        let level = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let max_samples = sample_rate as usize * channels as usize * MAX_RECORDING_SECONDS;
        let stream = build_stream(
            &device,
            &input.into(),
            format,
            samples.clone(),
            overflowed.clone(),
            input_failed.clone(),
            level.clone(),
            max_samples,
        )?;
        stream.play().map_err(|error| error.to_string())?;
        let started = Instant::now();
        self.active = Some(ActiveRecording {
            stream,
            samples,
            overflowed,
            input_failed,
            level,
            sample_rate,
            channels,
            started,
            last_loud: started,
            silence_seconds,
        });
        Ok(())
    }

    pub fn poll(&mut self) -> Option<(f32, bool)> {
        let active = self.active.as_mut()?;
        let now = Instant::now();
        let level = f32::from_bits(active.level.load(Ordering::Relaxed));
        if level >= 0.02 {
            active.last_loud = now;
        }
        let elapsed = now.duration_since(active.started).as_secs_f64();
        let quiet = quiet_duration(now, active.started, active.last_loud).as_secs_f64();
        let auto_stop = elapsed >= MAX_RECORDING_SECONDS as f64
            || (active.silence_seconds > 0.0 && elapsed >= 2.0 && quiet >= active.silence_seconds)
            || active.input_failed.load(Ordering::Relaxed)
            || active.overflowed.load(Ordering::Relaxed);
        Some((level, auto_stop))
    }

    pub fn set_silence_seconds(&mut self, seconds: f64) {
        if let Some(active) = self.active.as_mut() {
            active.silence_seconds = seconds;
        }
    }

    pub fn stop(&mut self, path: &Path) -> Result<(), String> {
        let active = self.active.take().ok_or("no active recording")?;
        drop(active.stream);
        if active.input_failed.load(Ordering::Relaxed) {
            return Err("audio input stream failed".into());
        }
        if active.overflowed.load(Ordering::Relaxed) {
            return Err("recording exceeded ten minutes".into());
        }
        let samples = active.samples.lock().map_err(|error| error.to_string())?;
        if samples.is_empty() {
            return Err(EMPTY_RECORDING.into());
        }
        write_wav(path, &samples, active.sample_rate, active.channels)
    }

    pub fn cancel(&mut self) {
        if let Some(trace) = self.trace.take() {
            trace.finish("cancelled");
        }
        self.active = None;
    }
}

fn quiet_duration(now: Instant, started: Instant, last_loud: Instant) -> std::time::Duration {
    now.saturating_duration_since(last_loud.max(started + std::time::Duration::from_secs(2)))
}

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    samples: Arc<Mutex<Vec<i16>>>,
    overflowed: Arc<AtomicBool>,
    input_failed: Arc<AtomicBool>,
    level: Arc<std::sync::atomic::AtomicU32>,
    max_samples: usize,
) -> Result<Stream, String> {
    let on_error = move |error| {
        input_failed.store(true, Ordering::Relaxed);
        eprintln!("audio input error: {error}");
    };
    match format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                append_samples(
                    &samples,
                    &overflowed,
                    &level,
                    max_samples,
                    data.iter()
                        .map(|value| (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
                )
            },
            on_error,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| {
                append_samples(
                    &samples,
                    &overflowed,
                    &level,
                    max_samples,
                    data.iter().copied(),
                )
            },
            on_error,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _| {
                append_samples(
                    &samples,
                    &overflowed,
                    &level,
                    max_samples,
                    data.iter().map(|value| (*value ^ 0x8000) as i16),
                )
            },
            on_error,
            None,
        ),
        _ => return Err(format!("unsupported input sample format: {format:?}")),
    }
    .map_err(|error| error.to_string())
}

fn append_samples(
    samples: &Mutex<Vec<i16>>,
    overflowed: &AtomicBool,
    level: &std::sync::atomic::AtomicU32,
    max_samples: usize,
    data: impl Iterator<Item = i16>,
) {
    if let Ok(mut buffer) = samples.lock() {
        let mut energy = 0.0_f64;
        let mut count = 0_u64;
        for sample in data {
            if buffer.len() >= max_samples {
                overflowed.store(true, Ordering::Relaxed);
                break;
            }
            buffer.push(sample);
            let normalized = sample as f64 / i16::MAX as f64;
            energy += normalized * normalized;
            count += 1;
        }
        if count > 0 {
            let rms = (energy / count as f64).sqrt().max(1e-7);
            let normalized = ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0) as f32;
            level.store(normalized.to_bits(), Ordering::Relaxed);
        }
    }
}

fn write_wav(path: &Path, samples: &[i16], sample_rate: u32, channels: u16) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|error| error.to_string())?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|error| error.to_string())?;
    }
    writer.finalize().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_preserves_device_format() {
        let path =
            std::env::temp_dir().join(format!("inksay-recording-{}.wav", uuid::Uuid::new_v4()));
        write_wav(&path, &[0, 100, -100, 0], 48_000, 2).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 48_000);
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.duration(), 2);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn input_level_tracks_real_sample_energy() {
        let samples = Mutex::new(Vec::new());
        let overflowed = AtomicBool::new(false);
        let level = std::sync::atomic::AtomicU32::new(0);
        append_samples(&samples, &overflowed, &level, 8, [0_i16; 4].into_iter());
        assert_eq!(f32::from_bits(level.load(Ordering::Relaxed)), 0.0);
        append_samples(
            &samples,
            &overflowed,
            &level,
            8,
            [16_000_i16; 4].into_iter(),
        );
        assert!(f32::from_bits(level.load(Ordering::Relaxed)) > 0.02);
    }

    #[test]
    fn silence_timer_starts_after_two_second_grace() {
        let started = Instant::now();
        assert_eq!(
            quiet_duration(
                started + std::time::Duration::from_secs(2),
                started,
                started
            )
            .as_secs_f64(),
            0.0
        );
        assert_eq!(
            quiet_duration(
                started + std::time::Duration::from_secs(3),
                started,
                started
            )
            .as_secs_f64(),
            1.0
        );
        let later_speech = started + std::time::Duration::from_secs(4);
        assert_eq!(
            quiet_duration(
                started + std::time::Duration::from_secs(5),
                started,
                later_speech
            )
            .as_secs_f64(),
            1.0
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_smoke {
    #[test]
    #[ignore = "Opens the default microphone for two seconds; requires INKSAY_CAPTURE_SMOKE=1"]
    fn capture_default_microphone() {
        assert_eq!(std::env::var("INKSAY_CAPTURE_SMOKE").as_deref(), Ok("1"));
        let path = std::env::temp_dir().join(format!("inksay-capture-{}.wav", uuid::Uuid::new_v4()));
        let mut recorder = super::Recorder::new();
        recorder.start("", 0.0).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        recorder.stop(&path).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        let duration = reader.duration() as f64 / reader.spec().sample_rate as f64;
        drop(reader);
        std::fs::remove_file(path).unwrap();
        assert!(duration > 0.5, "microphone produced no usable audio frames");
        println!("Captured {duration:.2}s of audio frames");
    }
}

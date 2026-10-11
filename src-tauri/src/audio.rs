use rubato::audioadapter::Adapter;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::path::Path;

const TARGET_RATE: u32 = 16_000;

pub fn normalize_wav(input: &Path, output: &Path) -> Result<(), String> {
    let mut reader = hound::WavReader::open(input).map_err(|error| error.to_string())?;
    let spec = reader.spec();
    if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
        return Err("expected 16-bit PCM recording".into());
    }
    let channels = usize::from(spec.channels);
    if channels == 0 {
        return Err("recording has no channels".into());
    }
    let source: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    let mono: Vec<f32> = source
        .chunks_exact(channels)
        .map(|frame| {
            frame.iter().map(|sample| *sample as f32).sum::<f32>()
                / (channels as f32 * i16::MAX as f32)
        })
        .collect();
    if mono.is_empty() {
        return Err("recording contains no audio".into());
    }
    let normalized = if spec.sample_rate == TARGET_RATE {
        mono
    } else {
        let input =
            InterleavedSlice::new(&mono, 1, mono.len()).map_err(|error| error.to_string())?;
        let mut resampler = Fft::<f32>::new(
            spec.sample_rate as usize,
            TARGET_RATE as usize,
            1024,
            1,
            FixedSync::Both,
        )
        .map_err(|error| error.to_string())?;
        let output = resampler
            .process_all(&input, mono.len(), None)
            .map_err(|error| error.to_string())?;
        (0..output.frames())
            .map(|frame| {
                output
                    .read_sample(0, frame)
                    .ok_or("resampler output is incomplete".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output, spec).map_err(|error| error.to_string())?;
    for sample in normalized {
        let sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(sample)
            .map_err(|error| error.to_string())?;
    }
    writer.finalize().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_48k_becomes_mono_16k_without_changing_duration() {
        let directory = std::env::temp_dir().join(format!("inksay-audio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.wav");
        let target = directory.join("target.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).unwrap();
        for _ in 0..48_000 {
            writer.write_sample(1000_i16).unwrap();
            writer.write_sample(3000_i16).unwrap();
        }
        writer.finalize().unwrap();
        normalize_wav(&source, &target).unwrap();
        let reader = hound::WavReader::open(&target).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.duration(), 16_000);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn stereo_44k1_noninteger_duration_keeps_frame_count() {
        let directory =
            std::env::temp_dir().join(format!("inksay-audio-44k-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.wav");
        let target = directory.join("target.wav");
        let frames = 44_100 / 3 + 17;
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&source, spec).unwrap();
        for index in 0..frames {
            writer.write_sample((index % 1000) as i16).unwrap();
            writer.write_sample((index % 500) as i16).unwrap();
        }
        writer.finalize().unwrap();
        normalize_wav(&source, &target).unwrap();
        let reader = hound::WavReader::open(&target).unwrap();
        let expected = (frames as f64 * 16_000.0 / 44_100.0).round() as i64;
        assert!((reader.duration() as i64 - expected).abs() <= 2);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

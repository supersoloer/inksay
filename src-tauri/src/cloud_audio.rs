use flacenc::component::BitRepr;
use flacenc::error::Verify;
use std::io::Cursor;

pub const INLINE_LIMIT: usize = 10_000_000;
const RATE: usize = 16_000;
const MAX_SAMPLES: usize = RATE * 300;
// Even incompressible PCM fits the inline budget at this size, including FLAC framing.
const SIZE_SAFE_SAMPLES: usize = 3_500_000;

pub struct AudioChunk {
    pub bytes: Vec<u8>,
    pub format: &'static str,
    pub sample_rate: u32,
    pub samples: usize,
}

pub fn inline_size(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4 + 22
}

/// Input is the application's normalized mono, 16 kHz PCM16 WAV; originals are never modified.
pub fn prepare(audio: Vec<u8>) -> Result<Vec<AudioChunk>, String> {
    let mut reader =
        hound::WavReader::new(Cursor::new(&audio)).map_err(|error| error.to_string())?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != RATE as u32
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("无效的 WAV 录音".into());
    }
    let count = reader.duration() as usize;
    if count == 0 {
        return Err("未检测到语音".into());
    }
    if count <= MAX_SAMPLES && inline_size(audio.len()) <= INLINE_LIMIT {
        return Ok(vec![AudioChunk {
            bytes: audio,
            format: "wav",
            sample_rate: spec.sample_rate,
            samples: count,
        }]);
    }
    let samples = reader
        .samples::<i16>()
        .map(|sample| sample.map(i32::from))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < samples.len() {
        let mut end = boundary(&samples, start, MAX_SAMPLES);
        let mut bytes = encode_flac(&samples[start..end])?;
        if inline_size(bytes.len()) > INLINE_LIMIT {
            end = boundary(&samples, start, SIZE_SAFE_SAMPLES);
            bytes = encode_flac(&samples[start..end])?;
        }
        if inline_size(bytes.len()) > INLINE_LIMIT {
            return Err("编码后的录音超过 10 MB，请缩短录音".into());
        }
        chunks.push(AudioChunk {
            bytes,
            format: "flac",
            sample_rate: spec.sample_rate,
            samples: end - start,
        });
        start = end;
    }
    Ok(chunks)
}

// Prefer a quiet 20 ms window near the end; contiguous ranges avoid lost or duplicated samples.
fn boundary(samples: &[i32], start: usize, limit: usize) -> usize {
    let end = (start + limit).min(samples.len());
    if end == samples.len() {
        return end;
    }
    let window = RATE / 50;
    let search = end - RATE * 5;
    (search..=end - window)
        .step_by(window)
        .min_by_key(|&offset| {
            samples[offset..offset + window]
                .iter()
                .map(|&sample| i64::from(sample).pow(2))
                .sum::<i64>()
        })
        .map(|offset| offset + window / 2)
        .unwrap_or(end)
}

fn encode_flac(samples: &[i32]) -> Result<Vec<u8>, String> {
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|error| format!("FLAC configuration: {error:?}"))?;
    let source = flacenc::source::MemSource::from_samples(samples, 1, 16, RATE);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|error| error.to_string())?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|error| error.to_string())?;
    Ok(sink.as_slice().to_vec())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn wav(samples: &[i16]) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut output,
            hound::WavSpec {
                channels: 1,
                sample_rate: RATE as u32,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for &sample in samples {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        output.into_inner()
    }

    fn decode(chunks: &[AudioChunk]) -> Vec<i32> {
        chunks
            .iter()
            .flat_map(|chunk| {
                assert!(inline_size(chunk.bytes.len()) <= INLINE_LIMIT);
                assert!(chunk.samples <= MAX_SAMPLES);
                let mut reader = claxon::FlacReader::new(Cursor::new(&chunk.bytes)).unwrap();
                assert_eq!(reader.streaminfo().sample_rate, RATE as u32);
                assert_eq!(reader.streaminfo().channels, 1);
                let samples = reader.samples().collect::<Result<Vec<_>, _>>().unwrap();
                assert_eq!(samples.len(), chunk.samples);
                samples
            })
            .collect()
    }

    #[test]
    fn recording_236_9_seconds_fits_without_losing_samples() {
        let samples: Vec<i16> = (0..3_790_400)
            .map(|i| ((i % 160) as i16 - 80) * 200)
            .collect();
        let audio = wav(&samples);
        assert!(inline_size(audio.len()) > INLINE_LIMIT);
        let chunks = prepare(audio).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].format, "flac");
        assert_eq!(
            decode(&chunks),
            samples.iter().copied().map(i32::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn long_incompressible_recording_is_split_without_gaps_or_overlap() {
        let mut seed = 123u32;
        let samples: Vec<i16> = (0..RATE * 600)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as i16
            })
            .collect();
        let chunks = prepare(wav(&samples)).unwrap();
        assert!(chunks.len() >= 3);
        assert_eq!(
            decode(&chunks),
            samples.iter().copied().map(i32::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn short_wav_is_preserved_and_invalid_audio_is_rejected() {
        let audio = wav(&[0; 1600]);
        let chunks = prepare(audio.clone()).unwrap();
        assert_eq!(chunks[0].bytes, audio);
        assert_eq!(chunks[0].format, "wav");
        assert!(prepare(vec![0; 20]).is_err());
        assert!(prepare(wav(&[])).is_err());
    }
}

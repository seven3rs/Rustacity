//! WAV decoding into the interleaved `f32` layout the rest of the app uses.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::IoError;

/// A decoded audio file.
///
/// Samples are interleaved and normalized to `[-1.0, 1.0]`. `mono` is the
/// channel mixdown used by the waveform, so the view never has to know how
/// many channels the file has.
#[derive(Clone)]
pub struct AudioFile {
    pub samples: Arc<[f32]>,
    pub mono: Arc<[f32]>,
    pub channels: u16,
    pub sample_rate: u32,
}

impl AudioFile {
    /// Number of frames (samples per channel).
    pub fn frames(&self) -> usize {
        self.mono.len()
    }

    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }

        self.frames() as f64 / f64::from(self.sample_rate)
    }

    /// Converts a position in seconds to a frame index, clamping to the file.
    pub fn seconds_to_frame(&self, seconds: f64) -> usize {
        let frame = (seconds.max(0.0) * f64::from(self.sample_rate)).round();

        frame.min(self.frames() as f64) as usize
    }

    /// Converts a frame index to a position in seconds.
    pub fn frame_to_seconds(&self, frame: usize) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }

        frame as f64 / f64::from(self.sample_rate)
    }
}

impl fmt::Debug for AudioFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioFile")
            .field("frames", &self.frames())
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

/// Decode a PCM or IEEE-float WAV file.
///
/// M1 accepts 8/16/24/32-bit integer PCM and 32-bit float, which covers what
/// Audacity's default WAV export and most recorders produce.
pub fn decode_wav(path: impl AsRef<Path>) -> Result<AudioFile, IoError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();

    if spec.channels == 0 {
        return Err(IoError::Unsupported(String::from(
            "WAV declares zero channels",
        )));
    }

    if spec.sample_rate == 0 {
        return Err(IoError::Unsupported(String::from(
            "WAV declares a zero sample rate",
        )));
    }

    let mut samples = Vec::new();

    match spec.sample_format {
        hound::SampleFormat::Float => {
            if spec.bits_per_sample != 32 {
                return Err(IoError::Unsupported(format!(
                    "float WAV with {} bits per sample",
                    spec.bits_per_sample
                )));
            }

            for sample in reader.samples::<f32>() {
                samples.push(sample?);
            }
        }
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;

            if !(8..=32).contains(&bits) {
                return Err(IoError::Unsupported(format!(
                    "integer WAV with {bits} bits per sample"
                )));
            }

            let scale = 1.0 / (1u64 << (bits - 1)) as f32;

            for sample in reader.samples::<i32>() {
                samples.push(sample? as f32 * scale);
            }
        }
    }

    let channels = usize::from(spec.channels);
    let frames = samples.len() / channels;
    samples.truncate(frames * channels);

    if frames == 0 {
        return Err(IoError::Unsupported(String::from(
            "WAV contains no audio frames",
        )));
    }

    let mono = mixdown(&samples, channels);

    Ok(AudioFile {
        samples: samples.into(),
        mono: mono.into(),
        channels: spec.channels,
        sample_rate: spec.sample_rate,
    })
}

fn mixdown(samples: &[f32], channels: usize) -> Vec<f32> {
    let scale = 1.0 / channels as f32;

    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * scale)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_wav_path(label: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "rustacity-io-{}-{stamp}-{label}.wav",
            std::process::id()
        ))
    }

    #[test]
    fn decodes_16_bit_stereo_and_mixes_down() {
        let path = temp_wav_path("stereo16");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        {
            let mut writer = hound::WavWriter::create(&path, spec).expect("create WAV");
            writer.write_sample(16_384i16).expect("left");
            writer.write_sample(-16_384i16).expect("right");
            writer.write_sample(0i16).expect("left");
            writer.write_sample(0i16).expect("right");
            writer.finalize().expect("finish WAV");
        }

        let file = decode_wav(&path).expect("decode WAV");

        assert_eq!(file.channels, 2);
        assert_eq!(file.sample_rate, 8_000);
        assert_eq!(file.frames(), 2);
        assert!((file.samples[0] - 0.5).abs() < 1e-6);
        assert!((file.samples[1] + 0.5).abs() < 1e-6);
        assert!(file.mono[0].abs() < 1e-6);
        assert!(file.mono[1].abs() < 1e-6);
        assert!((file.duration_seconds() - 0.000_25).abs() < 1e-9);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn decodes_24_bit_samples() {
        let path = temp_wav_path("pcm24");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };

        {
            let mut writer = hound::WavWriter::create(&path, spec).expect("create WAV");
            writer.write_sample(-8_388_608i32).expect("minimum");
            writer.write_sample(8_388_607i32).expect("maximum");
            writer.finalize().expect("finish WAV");
        }

        let file = decode_wav(&path).expect("decode WAV");

        assert!((file.samples[0] + 1.0).abs() < 1e-6);
        assert!((file.samples[1] - 0.999_999_9).abs() < 1e-6);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn decodes_8_bit_and_float_samples() {
        let int_path = temp_wav_path("pcm8");
        let int_spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 8,
            sample_format: hound::SampleFormat::Int,
        };

        {
            let mut writer = hound::WavWriter::create(&int_path, int_spec).expect("create WAV");
            writer.write_sample(-128i8).expect("minimum");
            writer.write_sample(127i8).expect("maximum");
            writer.finalize().expect("finish WAV");
        }

        let int_file = decode_wav(&int_path).expect("decode 8-bit WAV");
        assert!((int_file.samples[0] + 1.0).abs() < 1e-6);
        assert!((int_file.samples[1] - 127.0 / 128.0).abs() < 1e-6);

        let float_path = temp_wav_path("float32");
        let float_spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        {
            let mut writer = hound::WavWriter::create(&float_path, float_spec).expect("create WAV");
            writer.write_sample(-0.5f32).expect("negative");
            writer.write_sample(0.5f32).expect("positive");
            writer.finalize().expect("finish WAV");
        }

        let float_file = decode_wav(&float_path).expect("decode float WAV");
        assert_eq!(float_file.samples.as_ref(), &[-0.5f32, 0.5][..]);

        let _ = std::fs::remove_file(int_path);
        let _ = std::fs::remove_file(float_path);
    }

    #[test]
    fn rejects_an_empty_wav() {
        let path = temp_wav_path("empty");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        {
            let writer = hound::WavWriter::create(&path, spec).expect("create WAV");
            writer.finalize().expect("finish WAV");
        }

        assert!(matches!(decode_wav(&path), Err(IoError::Unsupported(_))));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn conversion_helpers_clamp_to_the_file() {
        let file = AudioFile {
            samples: vec![0.0, 0.0, 0.0].into(),
            mono: vec![0.0, 0.0, 0.0].into(),
            channels: 1,
            sample_rate: 1_000,
        };

        assert_eq!(file.seconds_to_frame(-1.0), 0);
        assert_eq!(file.seconds_to_frame(0.002), 2);
        assert_eq!(file.seconds_to_frame(10.0), 3);
        assert!((file.frame_to_seconds(2) - 0.002).abs() < 1e-9);
    }
}

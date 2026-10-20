//! `cpal` playback with a shared position that the UI can poll.

use std::fmt;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{AudioFile, IoError};

struct PlaybackState {
    samples: Arc<[f32]>,
    channels: usize,
    sample_rate: u32,
    position: usize,
    playing: bool,
    volume: f32,
}

impl PlaybackState {
    fn total_frames(&self) -> usize {
        self.samples.len().checked_div(self.channels).unwrap_or(0)
    }
}

/// A playing stream plus the state the callback reads.
///
/// The stream is intentionally kept in the struct: dropping it stops
/// playback. The UI reads the position through the mutex and never touches
/// the callback directly.
pub struct AudioPlayer {
    _stream: cpal::Stream,
    state: Arc<Mutex<PlaybackState>>,
}

impl AudioPlayer {
    /// Open the default output device and prepare the file for it.
    ///
    /// The file is converted to the device channel count and sample rate once,
    /// so the audio callback only has to copy frames.
    pub fn new(file: &AudioFile) -> Result<Self, IoError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| IoError::Device(String::from("no default output device")))?;

        let supported = device
            .default_output_config()
            .map_err(|error| IoError::Device(error.to_string()))?;

        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate();
        let config = supported.config();
        let channels = usize::from(config.channels);

        if channels == 0 {
            return Err(IoError::Device(String::from(
                "output device reports zero channels",
            )));
        }

        let samples = prepare_samples(
            &file.samples,
            usize::from(file.channels),
            channels,
            file.sample_rate,
            sample_rate,
        );

        if samples.is_empty() {
            return Err(IoError::Unsupported(String::from(
                "WAV contains no audio frames",
            )));
        }

        let state = Arc::new(Mutex::new(PlaybackState {
            samples: samples.into(),
            channels,
            sample_rate,
            position: 0,
            playing: false,
            volume: 1.0,
        }));

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, config, state.clone(), |sample| sample)?
            }
            cpal::SampleFormat::I8 => {
                build_stream::<i8>(&device, config, state.clone(), |sample| {
                    (sample.clamp(-1.0, 1.0) * i8::MAX as f32) as i8
                })?
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, config, state.clone(), |sample| {
                    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                })?
            }
            cpal::SampleFormat::I32 => {
                build_stream::<i32>(&device, config, state.clone(), |sample| {
                    (sample.clamp(-1.0, 1.0) * i32::MAX as f32) as i32
                })?
            }
            cpal::SampleFormat::I64 => {
                build_stream::<i64>(&device, config, state.clone(), |sample| {
                    (sample.clamp(-1.0, 1.0) * i64::MAX as f32) as i64
                })?
            }
            cpal::SampleFormat::U8 => {
                build_stream::<u8>(&device, config, state.clone(), |sample| {
                    ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u8::MAX as f32) as u8
                })?
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, config, state.clone(), |sample| {
                    ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16
                })?
            }
            cpal::SampleFormat::U32 => {
                build_stream::<u32>(&device, config, state.clone(), |sample| {
                    ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u32::MAX as f32) as u32
                })?
            }
            cpal::SampleFormat::U64 => {
                build_stream::<u64>(&device, config, state.clone(), |sample| {
                    ((sample.clamp(-1.0, 1.0) * 0.5 + 0.5) * u64::MAX as f32) as u64
                })?
            }
            cpal::SampleFormat::F64 => {
                build_stream::<f64>(&device, config, state.clone(), f64::from)?
            }
            other => {
                return Err(IoError::Unsupported(format!(
                    "output sample format {other:?}"
                )));
            }
        };

        stream
            .play()
            .map_err(|error| IoError::Stream(error.to_string()))?;

        Ok(Self {
            _stream: stream,
            state,
        })
    }

    pub fn play(&self) {
        let mut state = lock(&self.state);

        if state.position >= state.total_frames() {
            state.position = 0;
        }

        state.playing = true;
    }

    pub fn pause(&self) {
        lock(&self.state).playing = false;
    }

    pub fn stop(&self) {
        let mut state = lock(&self.state);
        state.playing = false;
        state.position = 0;
    }

    pub fn seek_seconds(&self, seconds: f64) {
        let mut state = lock(&self.state);
        let rate = f64::from(state.sample_rate);
        let frame = (seconds.max(0.0) * rate).round() as usize;

        state.position = frame.min(state.total_frames());
    }

    pub fn position_seconds(&self) -> f64 {
        let state = lock(&self.state);

        state.position as f64 / f64::from(state.sample_rate)
    }

    pub fn duration_seconds(&self) -> f64 {
        let state = lock(&self.state);

        state.total_frames() as f64 / f64::from(state.sample_rate)
    }

    pub fn is_playing(&self) -> bool {
        lock(&self.state).playing
    }

    pub fn set_volume(&self, volume: f32) {
        lock(&self.state).volume = volume.clamp(0.0, 1.0);
    }
}

impl fmt::Debug for AudioPlayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioPlayer")
            .field("position", &self.position_seconds())
            .field("playing", &self.is_playing())
            .finish_non_exhaustive()
    }
}

fn lock(state: &Arc<Mutex<PlaybackState>>) -> std::sync::MutexGuard<'_, PlaybackState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    state: Arc<Mutex<PlaybackState>>,
    convert: fn(f32) -> T,
) -> Result<cpal::Stream, IoError>
where
    T: cpal::SizedSample + Copy + 'static,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| fill(data, &state, convert),
            |error| eprintln!("audio output error: {error}"),
            None,
        )
        .map_err(|error| IoError::Stream(error.to_string()))
}

fn fill<T>(data: &mut [T], state: &Mutex<PlaybackState>, convert: fn(f32) -> T)
where
    T: Copy,
{
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    if !state.playing || state.channels == 0 {
        data.fill(convert(0.0));
        return;
    }

    let channels = state.channels;
    let total_frames = state.total_frames();
    let frames = data.len() / channels;

    for frame in 0..frames {
        if state.position >= total_frames {
            state.playing = false;
            data[frame * channels..].fill(convert(0.0));
            return;
        }

        for channel in 0..channels {
            let sample = state.samples[state.position * channels + channel] * state.volume;
            data[frame * channels + channel] = convert(sample);
        }

        state.position += 1;
    }

    let used = frames * channels;
    data[used..].fill(convert(0.0));
}

fn prepare_samples(
    samples: &[f32],
    input_channels: usize,
    output_channels: usize,
    input_rate: u32,
    output_rate: u32,
) -> Vec<f32> {
    if input_channels == 0 || output_channels == 0 || samples.is_empty() {
        return Vec::new();
    }

    let mut adapted = Vec::with_capacity((samples.len() / input_channels) * output_channels);

    for frame in samples.chunks_exact(input_channels) {
        if output_channels == 1 && input_channels > 1 {
            adapted.push(frame.iter().sum::<f32>() / input_channels as f32);
        } else if input_channels == 1 {
            adapted.extend(std::iter::repeat_n(frame[0], output_channels));
        } else {
            for channel in 0..output_channels {
                adapted.push(frame[channel.min(input_channels - 1)]);
            }
        }
    }

    if input_rate == output_rate || input_rate == 0 || output_rate == 0 {
        adapted
    } else {
        resample_linear(&adapted, output_channels, input_rate, output_rate)
    }
}

fn resample_linear(
    samples: &[f32],
    channels: usize,
    input_rate: u32,
    output_rate: u32,
) -> Vec<f32> {
    if channels == 0 || input_rate == 0 || output_rate == 0 {
        return Vec::new();
    }

    let input_frames = samples.len() / channels;

    if input_frames == 0 {
        return Vec::new();
    }

    let ratio = f64::from(input_rate) / f64::from(output_rate);
    let output_frames = ((input_frames as f64) * f64::from(output_rate) / f64::from(input_rate))
        .round()
        .max(1.0) as usize;
    let mut output = Vec::with_capacity(output_frames * channels);

    for frame in 0..output_frames {
        let position = frame as f64 * ratio;
        let index = (position.floor() as usize).min(input_frames - 1);
        let fraction = (position - position.floor()) as f32;
        let next = (index + 1).min(input_frames - 1);

        for channel in 0..channels {
            let a = samples[index * channels + channel];
            let b = samples[next * channels + channel];
            output.push(a + (b - a) * fraction);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_duplicated_to_every_output_channel() {
        let samples = prepare_samples(&[0.25, -0.25], 1, 2, 48_000, 48_000);
        assert_eq!(samples, vec![0.25, 0.25, -0.25, -0.25]);
    }

    #[test]
    fn stereo_is_mixed_down_for_a_mono_device() {
        let samples = prepare_samples(&[1.0, 0.0, -1.0, 0.0], 2, 1, 48_000, 48_000);
        assert_eq!(samples, vec![0.5, -0.5]);
    }

    #[test]
    #[ignore = "requires an output device"]
    fn opens_the_default_output_device() {
        let rate = 48_000;
        let samples: Vec<f32> = (0..rate / 4)
            .map(|frame| (frame as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.05)
            .collect();
        let file = AudioFile {
            samples: samples.clone().into(),
            mono: samples.into(),
            channels: 1,
            sample_rate: rate,
        };
        let player = AudioPlayer::new(&file).expect("default output device");

        player.play();
        player.seek_seconds(0.1);
        assert!(player.is_playing());
        assert!((player.position_seconds() - 0.1).abs() < 0.01);
        player.stop();
    }

    #[test]
    fn resampling_preserves_the_number_of_channels() {
        let samples = prepare_samples(&[0.0, 1.0], 1, 1, 2, 4);
        assert_eq!(samples.len(), 4);
        assert!((samples[0] - 0.0).abs() < 1e-6);
        assert!((samples[1] - 0.5).abs() < 1e-6);
        assert!((samples[2] - 1.0).abs() < 1e-6);
        assert!((samples[3] - 1.0).abs() < 1e-6);
    }
}

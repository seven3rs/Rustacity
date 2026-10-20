//! Audio devices and file formats for the Rustacity application.
//!
//! The UI never talks to a codec or an audio backend directly. M1 exposes the
//! smallest useful surface: decode a WAV into interleaved `f32`, build a peak
//! cache for the waveform, and play the result through `cpal`. FLAC/MP3 and
//! recording arrive in later milestones.

mod audio_file;
mod peaks;
mod player;

pub use audio_file::{decode_wav, AudioFile};
pub use peaks::{Peak, PeakCache};
pub use player::AudioPlayer;

/// The first format the M1 vertical slice reads.
pub const M1_FIRST_FORMAT: &str = "WAV";

/// Everything the audio layer can fail at.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// The WAV container or one of its samples is invalid.
    #[error("could not read WAV: {0}")]
    Wav(#[from] hound::Error),

    /// The file uses a sample layout this milestone does not decode yet.
    #[error("unsupported audio format: {0}")]
    Unsupported(String),

    /// No output device, or no usable default configuration.
    #[error("audio device error: {0}")]
    Device(String),

    /// The stream could not be created or started.
    #[error("audio stream error: {0}")]
    Stream(String),
}

// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Audio file formats for the Rustacity backend.
//!
//! The C++ frontend asks this crate to decode a WAV into an engine
//! [`AudioBuffer`].  The buffer is then exposed to the editor through the
//! backend's C ABI; no codec or sample store lives in the frontend any more.

mod audio_file;
mod peaks;

pub use audio_file::decode_wav;
pub use peaks::{Peak, PeakCache};
pub use rustacity_engine::AudioBuffer;

/// The first format the backend reads.
pub const FIRST_FORMAT: &str = "WAV";

/// Everything the audio layer can fail at.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// The WAV container or one of its samples is invalid.
    #[error("could not read WAV: {0}")]
    Wav(#[from] hound::Error),

    /// The file uses a sample layout this milestone does not decode yet.
    #[error("unsupported audio format: {0}")]
    Unsupported(String),
}

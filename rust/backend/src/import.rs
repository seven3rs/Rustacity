// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! WAV import: one Rust [`Sequence`] per channel.
//!
//! The C++ `PCMImportFileHandle` asks this module for a WAV; the decoded
//! interleaved buffer is split into mono sequences here, so the C++ side never
//! touches libsndfile or any sample storage for the Rust path.

use rustacity_io::{decode_wav, IoError};

use crate::Sequence;

/// A decoded WAV waiting for the C++ importer to take its channel sequences.
#[derive(Debug)]
pub struct Import {
    channels: Vec<Option<Sequence>>,
    sample_rate: u32,
}

impl Import {
    /// Decode `path` and split it into mono sequences.
    pub fn from_wav(path: impl AsRef<std::path::Path>) -> Result<Self, IoError> {
        let file = decode_wav(path)?;
        let channels = usize::from(file.channels());
        let frames = file.frames();
        let samples = file.samples();
        let sample_rate = file.sample_rate();
        let mut output = Vec::with_capacity(channels);

        for channel in 0..channels {
            let mono: Vec<f32> = (0..frames)
                .map(|frame| samples[frame * channels + channel])
                .collect();
            let sequence = Sequence::from_samples(sample_rate, mono)
                .map_err(|error| IoError::Unsupported(error.to_string()))?;
            output.push(Some(sequence));
        }

        Ok(Self {
            channels: output,
            sample_rate,
        })
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Transfer ownership of one channel's sequence to the caller.
    pub fn take(&mut self, channel: usize) -> Option<Sequence> {
        self.channels.get_mut(channel).and_then(Option::take)
    }
}

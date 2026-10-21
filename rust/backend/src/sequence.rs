// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! One mono audio buffer and the edit operations Audacity's C++ `Sequence`
//! performs on it.
//!
//! The C++ `Sequence` class keeps its public interface; when a clip comes from
//! the Rust WAV importer, its sample storage is one of these objects.  Every
//! edit builds a fresh [`AudioBuffer`] (the engine's copy-on-write shape), so
//! an undo snapshot that cloned the C++ Sequence keeps the old samples alive.

use std::sync::{Mutex, MutexGuard};

use rustacity_engine::{AudioBuffer, AudioBufferError, SampleRange};
use rustacity_io::PeakCache;

#[derive(Debug)]
struct SequenceData {
    buffer: AudioBuffer,
    // Built on the first waveform draw after an edit, so an append or a
    // delete does not pay for a full peak pyramid it may never need.
    peaks: Option<PeakCache>,
}

impl SequenceData {
    fn new(buffer: AudioBuffer) -> Self {
        Self {
            buffer,
            peaks: None,
        }
    }

    fn replace(&mut self, buffer: AudioBuffer) {
        self.peaks = None;
        self.buffer = buffer;
    }

    fn peaks(&mut self) -> &PeakCache {
        self.peaks
            .get_or_insert_with(|| PeakCache::new(&self.buffer))
    }
}

/// A mono, `f32` sample buffer with peak data for the waveform view.
#[derive(Debug)]
pub struct Sequence {
    data: Mutex<SequenceData>,
}

impl Sequence {
    /// Build an empty sequence at `sample_rate`.
    pub fn new(sample_rate: u32) -> Result<Self, AudioBufferError> {
        let buffer = AudioBuffer::silent(0, 1, sample_rate)?;
        Ok(Self {
            data: Mutex::new(SequenceData::new(buffer)),
        })
    }

    /// Build a sequence from mono `f32` samples.
    pub fn from_samples(sample_rate: u32, samples: Vec<f32>) -> Result<Self, AudioBufferError> {
        let buffer = AudioBuffer::new(samples, 1, sample_rate)?;
        Ok(Self {
            data: Mutex::new(SequenceData::new(buffer)),
        })
    }

    fn from_buffer(buffer: AudioBuffer) -> Self {
        Self {
            data: Mutex::new(SequenceData::new(buffer)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, SequenceData> {
        self.data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn sample_rate(&self) -> u32 {
        self.lock().buffer.sample_rate()
    }

    pub fn frames(&self) -> u64 {
        self.lock().buffer.frames() as u64
    }

    /// Copy up to `len` frames into `out`, returning the number written.
    pub fn read(&self, start: u64, len: usize, out: &mut [f32]) -> usize {
        let data = self.lock();
        let frames = data.buffer.frames();
        let start = usize::try_from(start).unwrap_or(usize::MAX).min(frames);
        let available = frames.saturating_sub(start).min(len).min(out.len());

        out[..available].copy_from_slice(&data.buffer.samples()[start..start + available]);
        available
    }

    /// Replace `input.len()` frames at `start`, growing the buffer if needed.
    pub fn write(&self, start: u64, input: &[f32]) {
        if input.is_empty() {
            return;
        }

        let mut data = self.lock();
        let channels = usize::from(data.buffer.channels());
        let old = data.buffer.samples();
        let mut samples = Vec::with_capacity(
            old.len().max(
                usize::try_from(start)
                    .unwrap_or(usize::MAX)
                    .saturating_add(input.len())
                    * channels,
            ),
        );
        samples.extend_from_slice(old);

        let start = usize::try_from(start).unwrap_or(usize::MAX);
        let end = start.saturating_add(input.len());
        if samples.len() < end * channels {
            samples.resize(end * channels, 0.0);
        }
        for (frame, sample) in input.iter().enumerate() {
            let at = (start + frame) * channels;
            samples[at] = *sample;
        }

        let rate = data.buffer.sample_rate();
        let buffer =
            AudioBuffer::new(samples, 1, rate).expect("a valid sequence stays valid after a write");
        data.replace(buffer);
    }

    /// Append `input` to the end of the sequence.
    pub fn append(&self, input: &[f32]) {
        if input.is_empty() {
            return;
        }

        let mut data = self.lock();
        let rate = data.buffer.sample_rate();
        let old = data.buffer.samples();
        let mut samples = Vec::with_capacity(old.len() + input.len());
        samples.extend_from_slice(old);
        samples.extend_from_slice(input);
        let buffer = AudioBuffer::new(samples, 1, rate).expect("appending leaves a valid sequence");
        data.replace(buffer);
    }

    /// Insert `input` frames at `start`.
    pub fn insert(&self, start: u64, input: &[f32]) {
        if input.is_empty() {
            return;
        }

        let mut data = self.lock();
        let rate = data.buffer.sample_rate();
        let inserted =
            AudioBuffer::new(input.to_vec(), 1, rate).expect("inserted samples have a valid shape");
        let buffer = data.buffer.insert_frames(start, &inserted);
        data.replace(buffer);
    }

    /// Remove `len` frames starting at `start`.
    pub fn delete(&self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        let mut data = self.lock();
        let range = SampleRange::new(start, start.saturating_add(len));
        let buffer = data.buffer.remove_frames(range);
        data.replace(buffer);
    }

    /// Insert `len` silent frames at `start`.
    pub fn insert_silence(&self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        let mut data = self.lock();
        let rate = data.buffer.sample_rate();
        let silent =
            AudioBuffer::silent(len as usize, 1, rate).expect("a silent buffer has a valid shape");
        let buffer = data.buffer.insert_frames(start, &silent);
        data.replace(buffer);
    }

    /// Zero the samples in `start..start + len`.
    pub fn set_silence(&self, start: u64, len: u64) {
        if len == 0 {
            return;
        }

        let mut data = self.lock();
        let rate = data.buffer.sample_rate();
        let channels = usize::from(data.buffer.channels());
        let mut samples = data.buffer.samples().to_vec();
        let start = usize::try_from(start).unwrap_or(usize::MAX);
        let end = start.saturating_add(len as usize);
        let end = end.min(samples.len() / channels);
        for frame in start.min(end)..end {
            for channel in 0..channels {
                samples[frame * channels + channel] = 0.0;
            }
        }
        let buffer = AudioBuffer::new(samples, 1, rate).expect("zeroing leaves a valid sequence");
        data.replace(buffer);
    }

    /// A deep copy of `start..start + len`.
    pub fn slice(&self, start: u64, len: u64) -> Self {
        let data = self.lock();
        let range = SampleRange::new(start, start.saturating_add(len));
        Self::from_buffer(data.buffer.slice_frames(range))
    }

    /// Min/max over `start..start + len`; `None` for an empty range.
    pub fn min_max(&self, start: u64, len: u64) -> (f32, f32) {
        let data = self.lock();
        let frames = data.buffer.frames();
        let start = usize::try_from(start).unwrap_or(usize::MAX).min(frames);
        let end = start.saturating_add(len as usize).min(frames);

        if start >= end {
            return (0.0, 0.0);
        }

        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for &sample in &data.buffer.samples()[start..end] {
            min = min.min(sample);
            max = max.max(sample);
        }

        (min, max)
    }

    /// RMS over `start..start + len`; zero for an empty range.
    pub fn rms(&self, start: u64, len: u64) -> f32 {
        let data = self.lock();
        let frames = data.buffer.frames();
        let start = usize::try_from(start).unwrap_or(usize::MAX).min(frames);
        let end = start.saturating_add(len as usize).min(frames);

        if start >= end {
            return 0.0;
        }

        let mut sum = 0.0_f64;
        for &sample in &data.buffer.samples()[start..end] {
            sum += f64::from(sample) * f64::from(sample);
        }

        (sum / (end - start) as f64).sqrt() as f32
    }

    /// Fill `where` and one min/max/rms/bl column per pixel for the visible
    /// time range.  `where` must have `width + 1` entries.
    #[allow(clippy::too_many_arguments)] // mirrors the C ABI's column arrays
    pub fn wave_display(
        &self,
        t0: f64,
        pixels_per_second: f64,
        width: usize,
        where_out: &mut [u64],
        min: &mut [f32],
        max: &mut [f32],
        rms: &mut [f32],
        bl: &mut [i32],
    ) {
        if width == 0 || where_out.len() < width + 1 || !pixels_per_second.is_finite() {
            return;
        }

        let mut data = self.lock();
        let rate = f64::from(data.buffer.sample_rate());
        let frames = data.buffer.frames() as u64;
        let samples_per_pixel = rate / pixels_per_second.max(f64::MIN_POSITIVE);
        let w0 = 0.5 + t0 * rate;

        for (column, slot) in where_out.iter_mut().take(width + 1).enumerate() {
            let value = (w0 + column as f64 * samples_per_pixel).floor();
            *slot = value.max(0.0).min(frames as f64) as u64;
        }

        let end_seconds = t0 + width as f64 / pixels_per_second.max(f64::MIN_POSITIVE);
        let peaks = data.peaks().peaks(t0, end_seconds, width);
        let samples = data.buffer.samples();

        for column in 0..width {
            let peak = peaks.get(column).copied().unwrap_or_default();
            let start = where_out[column] as usize;
            let end = where_out[column + 1] as usize;

            if start < end && end <= samples.len() {
                if end - start <= 512 {
                    let mut lo = f32::INFINITY;
                    let mut hi = f32::NEG_INFINITY;
                    let mut sum = 0.0_f64;
                    for &sample in &samples[start..end] {
                        lo = lo.min(sample);
                        hi = hi.max(sample);
                        sum += f64::from(sample) * f64::from(sample);
                    }
                    min[column] = lo;
                    max[column] = hi;
                    rms[column] = (sum / (end - start) as f64).sqrt() as f32;
                } else {
                    min[column] = peak.min;
                    max[column] = peak.max;
                    rms[column] = ((peak.min * peak.min + peak.max * peak.max) * 0.5).sqrt();
                }
            } else {
                min[column] = 0.0;
                max[column] = 0.0;
                rms[column] = 0.0;
            }

            bl[column] = 1;
        }
    }

    /// Fill one min/max/rms/bl column per pixel using the caller's `where`
    /// array (the fisheye path).
    pub fn wave_display_where(
        &self,
        where_in: &[u64],
        width: usize,
        min: &mut [f32],
        max: &mut [f32],
        rms: &mut [f32],
        bl: &mut [i32],
    ) {
        if width == 0 || where_in.len() < width + 1 {
            return;
        }

        let mut data = self.lock();
        let frames = data.buffer.frames();
        let rate = f64::from(data.buffer.sample_rate());

        for column in 0..width {
            let start = usize::try_from(where_in[column])
                .unwrap_or(usize::MAX)
                .min(frames);
            let end = usize::try_from(where_in[column + 1])
                .unwrap_or(usize::MAX)
                .min(frames);
            let end = end.max(start);

            if start < end {
                if end - start <= 512 {
                    let mut lo = f32::INFINITY;
                    let mut hi = f32::NEG_INFINITY;
                    let mut sum = 0.0_f64;
                    for &sample in &data.buffer.samples()[start..end] {
                        lo = lo.min(sample);
                        hi = hi.max(sample);
                        sum += f64::from(sample) * f64::from(sample);
                    }
                    min[column] = lo;
                    max[column] = hi;
                    rms[column] = (sum / (end - start) as f64).sqrt() as f32;
                } else {
                    // A wide column goes through the peak pyramid rather than
                    // re-reading every sample on each repaint.
                    let peak = data
                        .peaks()
                        .peaks(start as f64 / rate, end as f64 / rate, 1)[0];
                    min[column] = peak.min;
                    max[column] = peak.max;
                    rms[column] = ((peak.min * peak.min + peak.max * peak.max) * 0.5).sqrt();
                }
            } else {
                min[column] = 0.0;
                max[column] = 0.0;
                rms[column] = 0.0;
            }

            bl[column] = 1;
        }
    }
}

impl Clone for Sequence {
    fn clone(&self) -> Self {
        let data = self.lock();
        Self::from_buffer(data.buffer.clone())
    }
}

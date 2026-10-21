// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Core state for the Rustacity editor: audio buffers, tracks, the project,
//! the transport, the edit operations and the undo history.
//!
//! The engine owns the samples. `rustacity-io` decodes into [`AudioBuffer`];
//! `rustacity-backend` exposes one buffer at a time to the C++ `Sequence`,
//! which keeps the wxWidgets editor in charge of the project and track
//! objects for now. Keeping edits here means a later `.aup` reader and the
//! frontend can share one model, and the edit tests run without a window or
//! an audio device.

#![deny(missing_debug_implementations)]

use std::fmt;
use std::sync::Arc;

/// A sample range, in frames, half-open: `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SampleRange {
    pub start: u64,
    pub end: u64,
}

impl SampleRange {
    pub const fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    pub const fn len(self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.end <= self.start
    }

    /// Clamp both ends into `start..=end` and keep the range ordered.
    pub fn clamp(self, start: u64, end: u64) -> Self {
        let start = self.start.clamp(start, end);
        let end = self.end.clamp(start, end);

        Self { start, end }
    }

    /// Build an ordered range from two drag endpoints.
    pub const fn between(a: u64, b: u64) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }
}

/// Why an [`AudioBuffer`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBufferError {
    ZeroChannels,
    ZeroSampleRate,
}

impl fmt::Display for AudioBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroChannels => f.write_str("an audio buffer needs at least one channel"),
            Self::ZeroSampleRate => f.write_str("an audio buffer needs a sample rate"),
        }
    }
}

impl std::error::Error for AudioBufferError {}

/// Interleaved `f32` samples plus the mono mixdown the waveform reads.
///
/// The samples live behind an [`Arc`] so cloning a track for undo is cheap.
/// Every edit builds a new buffer; old snapshots keep the old samples alive
/// until they fall out of the history.
#[derive(Clone, PartialEq)]
pub struct AudioBuffer {
    samples: Arc<[f32]>,
    mono: Arc<[f32]>,
    channels: u16,
    sample_rate: u32,
}

impl AudioBuffer {
    /// Build a buffer from interleaved samples.
    ///
    /// A trailing partial frame is dropped. Empty audio is allowed; a zero
    /// channel count or sample rate is not.
    pub fn new(
        mut samples: Vec<f32>,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self, AudioBufferError> {
        if channels == 0 {
            return Err(AudioBufferError::ZeroChannels);
        }

        if sample_rate == 0 {
            return Err(AudioBufferError::ZeroSampleRate);
        }

        let channels_usize = usize::from(channels);
        let frames = samples.len() / channels_usize;
        samples.truncate(frames * channels_usize);

        let mono = mixdown(&samples, channels_usize);

        Ok(Self {
            samples: samples.into(),
            mono: mono.into(),
            channels,
            sample_rate,
        })
    }

    /// Build a silent buffer with a valid shape.
    pub fn silent(
        frames: usize,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self, AudioBufferError> {
        Self::new(
            vec![0.0; frames * usize::from(channels)],
            channels,
            sample_rate,
        )
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frames(&self) -> usize {
        self.mono.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames() == 0
    }

    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }

        self.frames() as f64 / f64::from(self.sample_rate)
    }

    /// Convert a position in seconds to a frame, clamped to the buffer.
    pub fn seconds_to_frame(&self, seconds: f64) -> usize {
        let frame = (seconds.max(0.0) * f64::from(self.sample_rate)).round();

        frame.min(self.frames() as f64) as usize
    }

    pub fn frame_to_seconds(&self, frame: usize) -> f64 {
        frame as f64 / f64::from(self.sample_rate)
    }

    /// The interleaved samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// The channel mixdown used for the waveform.
    pub fn mono(&self) -> &[f32] {
        &self.mono
    }

    pub fn mono_shared(&self) -> Arc<[f32]> {
        self.mono.clone()
    }

    /// Copy a frame range, clamped to the buffer.
    pub fn slice_frames(&self, range: SampleRange) -> Self {
        let range = range.clamp(0, self.frames() as u64);
        let channels = usize::from(self.channels);
        let start = range.start as usize * channels;
        let end = range.end as usize * channels;

        Self::new(
            self.samples[start..end].to_vec(),
            self.channels,
            self.sample_rate,
        )
        .expect("a slice of a valid buffer is valid")
    }

    /// Return a copy with a frame range removed.
    pub fn remove_frames(&self, range: SampleRange) -> Self {
        let frames = self.frames();
        let range = range.clamp(0, frames as u64);

        if range.is_empty() {
            return self.clone();
        }

        let channels = usize::from(self.channels);
        let start = range.start as usize * channels;
        let end = range.end as usize * channels;
        let mut samples = Vec::with_capacity(self.samples.len() - (end - start));
        samples.extend_from_slice(&self.samples[..start]);
        samples.extend_from_slice(&self.samples[end..]);

        Self::new(samples, self.channels, self.sample_rate)
            .expect("removing frames from a valid buffer is valid")
    }

    /// Return a copy with `other` inserted at `frame`.
    ///
    /// `other` is resampled and channel-adapted first, so a mono clipboard
    /// can be pasted into a stereo track.
    pub fn insert_frames(&self, frame: u64, other: &Self) -> Self {
        let channels = usize::from(self.channels);
        let at = (frame.min(self.frames() as u64) as usize) * channels;
        let adapted = other
            .resample_to(self.sample_rate)
            .adapt_channels(self.channels);
        let mut samples = Vec::with_capacity(self.samples.len() + adapted.samples.len());
        samples.extend_from_slice(&self.samples[..at]);
        samples.extend_from_slice(adapted.samples());
        samples.extend_from_slice(&self.samples[at..]);

        Self::new(samples, self.channels, self.sample_rate)
            .expect("inserting into a valid buffer is valid")
    }

    /// Linear resampling. Good enough for an imported file until the DSP
    /// resampler replaces it; edit positions stay in project frames.
    pub fn resample_to(&self, sample_rate: u32) -> Self {
        if sample_rate == 0 || sample_rate == self.sample_rate {
            return self.clone();
        }

        let channels = usize::from(self.channels);
        let input_frames = self.frames();
        let output_frames = ((input_frames as f64 * f64::from(sample_rate)
            / f64::from(self.sample_rate))
        .round() as usize)
            .max(1);

        if input_frames == 0 {
            return Self::new(Vec::new(), self.channels, sample_rate)
                .expect("a resampled empty buffer is valid");
        }

        let ratio = f64::from(self.sample_rate) / f64::from(sample_rate);
        let mut output = Vec::with_capacity(output_frames * channels);

        for frame in 0..output_frames {
            let position = frame as f64 * ratio;
            let index = (position.floor() as usize).min(input_frames - 1);
            let fraction = (position - position.floor()) as f32;
            let next = (index + 1).min(input_frames - 1);

            for channel in 0..channels {
                let a = self.samples[index * channels + channel];
                let b = self.samples[next * channels + channel];
                output.push(a + (b - a) * fraction);
            }
        }

        Self::new(output, self.channels, sample_rate).expect("a resampled buffer is valid")
    }

    /// Convert mono to N channels or N channels to mono; otherwise copy or
    /// truncate channels.
    pub fn adapt_channels(&self, channels: u16) -> Self {
        if channels == 0 || channels == self.channels {
            return self.clone();
        }

        let input_channels = usize::from(self.channels);
        let output_channels = usize::from(channels);
        let mut output = Vec::with_capacity(self.frames() * output_channels);

        for frame in self.samples.chunks_exact(input_channels) {
            if output_channels == 1 && input_channels > 1 {
                output.push(frame.iter().sum::<f32>() / input_channels as f32);
            } else if input_channels == 1 {
                output.extend(std::iter::repeat_n(frame[0], output_channels));
            } else {
                for channel in 0..output_channels {
                    output.push(frame[channel.min(input_channels - 1)]);
                }
            }
        }

        Self::new(output, channels, self.sample_rate).expect("a channel-adapted buffer is valid")
    }
}

impl fmt::Debug for AudioBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioBuffer")
            .field("frames", &self.frames())
            .field("channels", &self.channels)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

fn mixdown(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }

    let scale = 1.0 / channels as f32;

    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * scale)
        .collect()
}

/// One audio track: the buffer, its mixer settings and a view revision.
///
/// `revision` changes whenever the buffer is replaced, so a later frontend
/// can keep a peak cache per track and rebuild only the tracks that changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub name: String,
    pub muted: bool,
    pub solo: bool,
    pub gain: f32,
    buffer: AudioBuffer,
    revision: u64,
}

impl Track {
    pub fn new(name: impl Into<String>, buffer: AudioBuffer) -> Self {
        Self {
            name: name.into(),
            muted: false,
            solo: false,
            gain: 1.0,
            buffer,
            revision: 1,
        }
    }

    pub fn buffer(&self) -> &AudioBuffer {
        &self.buffer
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn channels(&self) -> u16 {
        self.buffer.channels()
    }

    pub fn sample_rate(&self) -> u32 {
        self.buffer.sample_rate()
    }

    pub fn frames(&self) -> usize {
        self.buffer.frames()
    }

    pub fn duration_seconds(&self) -> f64 {
        self.buffer.duration_seconds()
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(0.0, 4.0);
    }

    fn replace_buffer(&mut self, buffer: AudioBuffer) {
        self.buffer = buffer;
        self.revision = self.revision.wrapping_add(1);
    }

    fn resample_to(&mut self, sample_rate: u32) {
        if self.sample_rate() != sample_rate {
            let buffer = self.buffer.resample_to(sample_rate);
            self.replace_buffer(buffer);
        }
    }
}

/// The transport's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    #[default]
    Stopped,
    Playing,
    Recording,
}

impl Transport {
    pub const fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }
}

/// The project: tracks, the active track, the selection and the transport.
///
/// Every track is kept at the project sample rate. The first track sets it;
/// later imports are resampled when they are added.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub tracks: Vec<Track>,
    pub active_track: Option<usize>,
    pub selection: SampleRange,
    pub transport: Transport,
    sample_rate: u32,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            active_track: None,
            selection: SampleRange::default(),
            transport: Transport::Stopped,
            sample_rate: 0,
        }
    }
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Append a track and make it active if the project was empty.
    ///
    /// A track whose rate differs from the project is resampled, so every
    /// frame index in the project means the same instant.
    pub fn add_track(&mut self, mut track: Track) -> usize {
        if self.sample_rate == 0 {
            self.sample_rate = track.sample_rate();
        } else {
            track.resample_to(self.sample_rate);
        }

        let index = self.tracks.len();
        self.tracks.push(track);
        self.active_track.get_or_insert(index);
        index
    }

    pub fn active_track(&self) -> Option<&Track> {
        self.active_track.and_then(|index| self.tracks.get(index))
    }

    pub fn active_track_mut(&mut self) -> Option<&mut Track> {
        self.active_track
            .and_then(|index| self.tracks.get_mut(index))
    }

    pub fn set_active_track(&mut self, index: usize) -> bool {
        if index < self.tracks.len() {
            self.active_track = Some(index);
            self.selection = SampleRange::default();
            true
        } else {
            false
        }
    }

    pub fn duration_frames(&self) -> u64 {
        self.tracks
            .iter()
            .map(|track| track.frames() as u64)
            .max()
            .unwrap_or(0)
    }

    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }

        self.duration_frames() as f64 / f64::from(self.sample_rate)
    }

    pub fn selection_seconds(&self) -> Option<(f64, f64)> {
        if self.sample_rate == 0 || self.selection.is_empty() {
            return None;
        }

        let rate = f64::from(self.sample_rate);
        Some((
            self.selection.start as f64 / rate,
            self.selection.end as f64 / rate,
        ))
    }

    pub fn set_selection(&mut self, range: SampleRange) -> SampleRange {
        let end = self.active_track().map_or(0, |track| track.frames() as u64);
        self.selection = range.clamp(0, end);
        self.selection
    }

    pub fn clear_selection(&mut self) {
        self.selection = SampleRange::default();
    }

    pub fn select_all_active(&mut self) {
        let end = self.active_track().map_or(0, |track| track.frames() as u64);
        self.selection = SampleRange::new(0, end);
    }

    /// Copy the selected frames without changing the project.
    pub fn copy_selection(&self) -> Option<AudioBuffer> {
        let track = self.active_track()?;
        let range = self.selection.clamp(0, track.frames() as u64);

        if range.is_empty() {
            return None;
        }

        Some(track.buffer().slice_frames(range))
    }

    /// Remove and return the selected frames. The selection collapses to the
    /// cut point, as it does in Audacity.
    pub fn cut_selection(&mut self) -> Option<AudioBuffer> {
        let (range, buffer) = {
            let track = self.active_track()?;
            let range = self.selection.clamp(0, track.frames() as u64);

            if range.is_empty() {
                return None;
            }

            (range, track.buffer().slice_frames(range))
        };

        let track = self.active_track_mut()?;
        let edited = track.buffer().remove_frames(range);
        track.replace_buffer(edited);
        self.selection = SampleRange::new(range.start, range.start);

        Some(buffer)
    }

    pub fn delete_selection(&mut self) -> bool {
        self.cut_selection().is_some()
    }

    /// Insert a buffer at `frame` in the active track and select the result.
    pub fn paste_at(&mut self, frame: u64, clipboard: &AudioBuffer) -> Option<SampleRange> {
        let project_rate = self.sample_rate;
        let track = self.active_track_mut()?;
        let at = frame.min(track.frames() as u64);
        let pasted_frames = clipboard
            .resample_to(project_rate)
            .adapt_channels(track.channels())
            .frames() as u64;
        let edited = track.buffer().insert_frames(at, clipboard);
        track.replace_buffer(edited);
        self.selection = SampleRange::new(at, at + pasted_frames);

        Some(self.selection)
    }

    pub fn toggle_mute(&mut self, index: usize) -> bool {
        if let Some(track) = self.tracks.get_mut(index) {
            track.muted = !track.muted;
            true
        } else {
            false
        }
    }

    pub fn toggle_solo(&mut self, index: usize) -> bool {
        if let Some(track) = self.tracks.get_mut(index) {
            track.solo = !track.solo;
            true
        } else {
            false
        }
    }

    pub fn set_gain(&mut self, index: usize, gain: f32) -> bool {
        if let Some(track) = self.tracks.get_mut(index) {
            track.set_gain(gain);
            true
        } else {
            false
        }
    }

    /// Mix every audible track to stereo at the project rate.
    ///
    /// A muted track never contributes. If any track is soloed, only soloed
    /// tracks contribute. Gain is applied before summing; clipping is left to
    /// the output device.
    pub fn mixdown(&self) -> Option<AudioBuffer> {
        if self.tracks.is_empty() || self.sample_rate == 0 {
            return None;
        }

        let frames = self.duration_frames() as usize;
        let any_solo = self.tracks.iter().any(|track| track.solo);
        let mut mix = vec![0.0f32; frames * 2];

        for track in &self.tracks {
            if track.muted || (any_solo && !track.solo) {
                continue;
            }

            let gain = track.gain.clamp(0.0, 4.0);

            if gain == 0.0 {
                continue;
            }

            let buffer = track.buffer();
            let channels = usize::from(buffer.channels());
            let samples = buffer.samples();
            let track_frames = buffer.frames().min(frames);

            for frame in 0..track_frames {
                let left = samples[frame * channels];
                let right = if channels > 1 {
                    samples[frame * channels + 1]
                } else {
                    left
                };

                mix[frame * 2] += left * gain;
                mix[frame * 2 + 1] += right * gain;
            }
        }

        AudioBuffer::new(mix, 2, self.sample_rate).ok()
    }

    pub fn play(&mut self) {
        self.transport = Transport::Playing;
    }

    pub fn stop(&mut self) {
        self.transport = Transport::Stopped;
    }

    pub fn record(&mut self) {
        self.transport = Transport::Recording;
    }
}

/// Snapshot undo/redo over the project.
///
/// Snapshots share the audio buffers, so the cost of a snapshot is the track
/// metadata, not a copy of the samples. The transport is intentionally not
/// restored: undoing an edit while playing must not stop the transport.
#[derive(Debug)]
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(64)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
        }
    }

    pub fn record(&mut self, project: &Project) {
        if self.limit == 0 {
            return;
        }

        self.undo.push(project.clone());

        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }

        self.redo.clear();
    }

    pub fn undo(&mut self, project: &mut Project) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };

        let transport = project.transport;
        self.redo.push(project.clone());
        *project = previous;
        project.transport = transport;

        true
    }

    pub fn redo(&mut self, project: &mut Project) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };

        let transport = project.transport;
        self.undo.push(project.clone());
        *project = next;
        project.transport = transport;

        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(samples: Vec<f32>, rate: u32) -> AudioBuffer {
        AudioBuffer::new(samples, 1, rate).expect("valid mono buffer")
    }

    fn project_with_track(samples: Vec<f32>) -> Project {
        let mut project = Project::new();
        project.add_track(Track::new("Audio 1", mono(samples, 1_000)));
        project
    }

    #[test]
    fn a_range_is_half_open() {
        let range = SampleRange::new(10, 25);
        assert_eq!(range.len(), 15);
        assert!(!range.is_empty());
    }

    #[test]
    fn an_empty_range_has_no_length() {
        assert_eq!(SampleRange::new(7, 7).len(), 0);
        assert!(SampleRange::new(7, 7).is_empty());
    }

    #[test]
    fn a_range_clamps_and_orders_its_ends() {
        assert_eq!(
            SampleRange::new(2, 20).clamp(0, 10),
            SampleRange::new(2, 10)
        );
        assert_eq!(
            SampleRange::new(20, 30).clamp(0, 10),
            SampleRange::new(10, 10)
        );
        assert_eq!(SampleRange::between(9, 3), SampleRange::new(3, 9));
    }

    #[test]
    fn transport_tracks_the_project() {
        let mut project = Project::new();
        assert_eq!(project.transport, Transport::Stopped);

        project.play();
        assert!(project.transport.is_playing());

        project.stop();
        assert_eq!(project.transport, Transport::Stopped);

        project.record();
        assert_eq!(project.transport, Transport::Recording);
    }

    #[test]
    fn a_buffer_mixes_down_and_reports_its_shape() {
        let buffer =
            AudioBuffer::new(vec![1.0, 0.0, -0.5, -0.5], 2, 48_000).expect("stereo buffer");

        assert_eq!(buffer.frames(), 2);
        assert_eq!(buffer.channels(), 2);
        assert_eq!(buffer.sample_rate(), 48_000);
        assert_eq!(buffer.mono(), &[0.5, -0.5]);
        assert!((buffer.duration_seconds() - 2.0 / 48_000.0).abs() < 1e-12);
    }

    #[test]
    fn a_buffer_rejects_invalid_shapes() {
        assert_eq!(
            AudioBuffer::new(vec![0.0], 0, 48_000),
            Err(AudioBufferError::ZeroChannels)
        );
        assert_eq!(
            AudioBuffer::new(vec![0.0], 1, 0),
            Err(AudioBufferError::ZeroSampleRate)
        );
    }

    #[test]
    fn slicing_removing_and_inserting_keep_frames_aligned() {
        let buffer = mono(vec![0.0, 1.0, 2.0, 3.0], 1_000);
        let slice = buffer.slice_frames(SampleRange::new(1, 3));
        assert_eq!(slice.samples(), &[1.0, 2.0]);

        let removed = buffer.remove_frames(SampleRange::new(1, 3));
        assert_eq!(removed.samples(), &[0.0, 3.0]);

        let inserted = removed.insert_frames(1, &mono(vec![8.0, 9.0], 1_000));
        assert_eq!(inserted.samples(), &[0.0, 8.0, 9.0, 3.0]);
    }

    #[test]
    fn resampling_preserves_the_audio_shape() {
        let buffer = mono(vec![0.0, 1.0], 2);
        let resampled = buffer.resample_to(4);

        assert_eq!(resampled.frames(), 4);
        assert_eq!(resampled.sample_rate(), 4);
        assert!((resampled.samples()[0] - 0.0).abs() < 1e-6);
        assert!((resampled.samples()[1] - 0.5).abs() < 1e-6);
        assert!((resampled.samples()[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pasting_adapts_a_mono_clipboard_to_stereo() {
        let stereo = AudioBuffer::new(vec![0.0, 0.0], 2, 1_000).expect("stereo");
        let inserted = stereo.insert_frames(0, &mono(vec![0.5], 1_000));
        assert_eq!(inserted.channels(), 2);
        assert_eq!(inserted.samples(), &[0.5, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn the_first_track_sets_the_project_rate() {
        let mut project = Project::new();
        let first = project.add_track(Track::new("first", mono(vec![0.0; 4], 1_000)));
        let second = project.add_track(Track::new("second", mono(vec![0.0; 4], 2_000)));

        assert_eq!(first, 0);
        assert_eq!(second, 1);
        assert_eq!(project.sample_rate(), 1_000);
        assert_eq!(project.tracks[1].sample_rate(), 1_000);
        assert_eq!(project.tracks[1].frames(), 2);
        assert_eq!(project.active_track, Some(0));
    }

    #[test]
    fn the_active_track_can_change() {
        let mut project = project_with_track(vec![0.0]);
        project.add_track(Track::new("Audio 2", mono(vec![0.0], 1_000)));

        assert!(project.set_active_track(1));
        assert_eq!(project.active_track(), Some(&project.tracks[1]));
        assert!(!project.set_active_track(5));
    }

    #[test]
    fn cut_copy_and_paste_round_trip_a_selection() {
        let mut project = project_with_track(vec![0.0, 1.0, 2.0, 3.0, 4.0]);
        project.set_selection(SampleRange::new(1, 3));

        let copied = project.copy_selection().expect("copy");
        assert_eq!(copied.samples(), &[1.0, 2.0]);

        let cut = project.cut_selection().expect("cut");
        assert_eq!(cut.samples(), &[1.0, 2.0]);
        assert_eq!(project.tracks[0].buffer().samples(), &[0.0, 3.0, 4.0]);
        assert_eq!(project.selection, SampleRange::new(1, 1));

        let pasted = project.paste_at(1, &cut).expect("paste");
        assert_eq!(pasted, SampleRange::new(1, 3));
        assert_eq!(
            project.tracks[0].buffer().samples(),
            &[0.0, 1.0, 2.0, 3.0, 4.0]
        );
    }

    #[test]
    fn an_empty_selection_does_not_edit() {
        let mut project = project_with_track(vec![0.0, 1.0]);
        assert!(project.copy_selection().is_none());
        assert!(project.cut_selection().is_none());
        assert!(!project.delete_selection());
    }

    #[test]
    fn mixing_applies_mute_solo_and_gain() {
        let mut project = Project::new();
        let first = project.add_track(Track::new("first", mono(vec![1.0, 1.0], 1_000)));
        project.add_track(Track::new("second", mono(vec![0.5, 0.5], 1_000)));
        project.set_gain(first, 0.5);

        let mix = project.mixdown().expect("mix").samples().to_vec();
        assert_eq!(mix, vec![1.0, 1.0, 1.0, 1.0]);

        project.toggle_mute(1);
        let mix = project.mixdown().expect("mix").samples().to_vec();
        assert_eq!(mix, vec![0.5, 0.5, 0.5, 0.5]);

        project.toggle_mute(1);
        project.toggle_solo(1);
        let mix = project.mixdown().expect("mix").samples().to_vec();
        assert_eq!(mix, vec![0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn history_undoes_and_redoes_an_edit() {
        let mut project = project_with_track(vec![0.0, 1.0, 2.0]);
        let mut history = History::new(8);
        project.set_selection(SampleRange::new(0, 2));

        history.record(&project);
        project.cut_selection().expect("cut");
        assert_eq!(project.tracks[0].buffer().samples(), &[2.0]);

        assert!(history.undo(&mut project));
        assert_eq!(project.tracks[0].buffer().samples(), &[0.0, 1.0, 2.0]);
        assert_eq!(project.selection, SampleRange::new(0, 2));

        assert!(history.redo(&mut project));
        assert_eq!(project.tracks[0].buffer().samples(), &[2.0]);
        assert!(!history.can_redo());
    }

    #[test]
    fn history_keeps_transport_state_out_of_snapshots() {
        let mut project = project_with_track(vec![0.0, 1.0]);
        let mut history = History::new(8);
        history.record(&project);
        project.play();

        assert!(history.undo(&mut project));
        assert_eq!(project.transport, Transport::Playing);
    }
}

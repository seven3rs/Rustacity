//! Core state for the Rustacity editor: the project, the tracks, the
//! transport and the edit operations the UI drives.
//!
//! M0 is deliberately small. It fixes the vocabulary that the iced shell and
//! the future `.aup` reader share, and it keeps the audio machinery out of
//! the view layer. The signal path itself lands in M1.

#![deny(missing_debug_implementations)]

/// A sample range, in samples, half-open: `start..end`.
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
}

/// One audio track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub name: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub muted: bool,
    pub solo: bool,
}

impl Track {
    pub fn new(name: impl Into<String>, channels: u16, sample_rate: u32) -> Self {
        Self {
            name: name.into(),
            channels,
            sample_rate,
            muted: false,
            solo: false,
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

/// The project: the tracks and the current selection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Project {
    pub tracks: Vec<Track>,
    pub selection: SampleRange,
    pub transport: Transport,
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_track(&mut self, track: Track) {
        self.tracks.push(track);
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_track_keeps_its_shape() {
        let track = Track::new("Audio 1", 2, 48_000);
        assert_eq!(track.name, "Audio 1");
        assert_eq!(track.channels, 2);
        assert_eq!(track.sample_rate, 48_000);
        assert!(!track.muted);
        assert!(!track.solo);
    }
}

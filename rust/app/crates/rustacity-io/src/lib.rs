//! Audio devices and file formats for the Rustacity application.
//!
//! M0 leaves this crate empty on purpose: the boundary is fixed first, and
//! `cpal` (playback and recording) plus `symphonia` (decoding) land in M1.
//! The UI calls this crate, never a codec directly, so the backend can be
//! swapped without touching the shell.

/// The first format the M1 vertical slice will read.
pub const M1_FIRST_FORMAT: &str = "WAV";

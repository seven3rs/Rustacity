//! Project persistence: the `.aup` XML and its blockfiles.
//!
//! M0 is a placeholder. The reader and writer land in M4 and must round-trip
//! real Audacity 2.4.1 projects before the C++ tree can be retired, so the
//! crate owns the format from the start instead of letting the view layer
//! parse XML.

/// The project file extension the M4 reader and writer own.
pub const PROJECT_EXTENSION: &str = "aup";

// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Rust-owned audio storage for the Rustacity C++ frontend.
//!
//! The wxWidgets editor keeps its project and track objects, but the samples
//! and the sample-editing operations live here.  [`Sequence`] is the object a
//! C++ `Sequence` delegates to: one mono buffer per clip channel, with the
//! same append/delete/insert/silence/peak operations Audacity's C++ Sequence
//! exposed.  [`Import`] turns a WAV file into one Sequence per channel.
//!
//! The C ABI is in [`ffi`] and the hand-written header is
//! `include/rustacity_backend.h`.
//!
//! Contributor: Sato <seven3rs@gmail.com>

#![deny(missing_debug_implementations)]

pub mod ffi;
mod import;
mod sequence;

pub use import::Import;
pub use sequence::Sequence;

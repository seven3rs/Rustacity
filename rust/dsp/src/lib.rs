// Rustacity: A Digital Audio Editor
//
// Copyright (C) 2026 Sadik Pranto
//
// SPDX-License-Identifier: GPL-2.0-or-later

//! DSP core of Rustacity.
//!
//! Pure-math audio DSP ported from the original Audacity C++ sources,
//! exposed to the application through the C ABI declared in
//! `include/rustacity_dsp.h`.  Zero runtime dependencies.
//!
//! Contributor: Sadik Pranto <pranto.smss.dev@gmail.com>

pub mod bass_treble;
pub mod biquad;
pub mod distortion;
pub mod ebur128;
pub mod fft;
pub mod phaser;
pub mod wahwah;

mod ffi;

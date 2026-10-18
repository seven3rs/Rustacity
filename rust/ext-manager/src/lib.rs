// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension manager: the extensions directory, and what is in it.
//!
//! An extension is a directory holding `extension.toml` and `extension.wasm`.
//! Installed means that directory is in the extensions root;
//! [turned off](model::set_enabled) means it is in a `disabled` subdirectory of
//! that root instead, so the state of every extension is visible in a file
//! manager and nothing has to be kept in step.
//!
//! This crate is the back end: reading the directory, moving an extension
//! between the two slots, and [asking the host](verify) whether one still
//! loads.  It has no GUI dependency, so it builds, runs and tests like any
//! other host tool, and the window that drives it - `rust/ext-manager-gui` -
//! links it and calls it directly.  `rustacity-ext-manager`, the program in
//! this crate, is the same work without a window: it prints the listings.

pub mod model;
pub mod open;
pub mod paths;
pub mod verify;

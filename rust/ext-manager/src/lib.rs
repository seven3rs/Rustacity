// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension manager: the extensions directory, and the page that drives it.
//!
//! An extension is a directory holding `extension.toml` and `extension.wasm`.
//! Installed means that directory is in the extensions root;
//! [turned off](model::set_enabled) means it is in a `disabled` subdirectory of
//! that root instead, so the state of every extension is visible in a file
//! manager and nothing has to be kept in step.
//!
//! This crate is the whole back end, and it has no GUI dependency of its own.
//! It serves [the built interface](server) - a WebAssembly program in
//! `rust/ext-manager-ui` - at `/`, and the JSON API that interface calls under
//! `/api/`.  Anything the interface can do can be done by hand with `curl`
//! against the same server, which is what `tests/server.rs` does.
//!
//! The reason for that split: a browser cannot read or write a directory, and
//! every native GUI toolkit is a dependency this project does not otherwise
//! have.  The directory work is therefore ordinary Rust here, where the tests
//! can see it, and the interface is the only part that is WebAssembly.

pub mod model;
pub mod open;
pub mod paths;
pub mod server;
pub mod verify;

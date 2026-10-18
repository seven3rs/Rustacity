// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension API of Rustacity: version 1.
//!
//! This crate holds the parts of the extension engine that are shared and that
//! do not need a WebAssembly runtime: the ABI names and limits ([`abi`]), the
//! manifest and its validation ([`manifest`]), and the error type every entry
//! point returns ([`ExtError`]).
//!
//! It is deliberately free of `wasmtime`, so that tools which only read or write
//! manifests — a packager, a lint, an extension's own build script — do not
//! carry a WebAssembly engine to do it.
//!
//! The authoritative description of the contract is `docs/extensions/API.md`.

pub mod abi;
pub mod error;
pub mod manifest;

pub use error::ExtError;
pub use manifest::{Manifest, Parameter, ParameterKind};

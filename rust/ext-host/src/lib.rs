// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! Sandboxed WebAssembly extensions for Rustacity.
//!
//! An extension is a directory holding `extension.toml` and `extension.wasm`;
//! `docs/extensions/API.md` is the contract they are written against.
//! [`Engine`] loads them, [`Extension`] is a loaded module with its manifest,
//! and [`Instance`] is one running copy of it with audio going through.
//!
//! The C++ application uses this crate through the C ABI in
//! `include/rustacity_ext.h`, which [`ffi`] implements.

mod engine;
pub mod ffi;
mod limits;
mod log;

pub use engine::{Engine, Extension, Instance};
pub use limits::{HostConfig, ProcessLimits};
pub use log::LogRecord;
pub use rustacity_ext_api::abi;
pub use rustacity_ext_api::{ExtError, Manifest, Parameter, ParameterKind};

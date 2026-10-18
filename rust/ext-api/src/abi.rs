// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! Names and limits of the module ABI.
//!
//! These strings are the contract with the `.wasm` module: the exported
//! functions it must provide, the one import it may use, and the constants that
//! decide how large a block may be and how long a call may take. Keeping them in
//! one place is what stops the host and the documentation from drifting apart.

/// The API version this host implements, and what `rx_api_version` must return.
pub const API_VERSION: i32 = 1;

/// The exported linear memory every module must have.
pub const EXPORT_MEMORY: &str = "memory";
/// Exported: `() -> i32`, the module's API version.
pub const EXPORT_API_VERSION: &str = "rx_api_version";
/// Exported: `(size: i32) -> i32`, allocates inside the module's memory.
pub const EXPORT_ALLOC: &str = "rx_alloc";
/// Exported: `(ptr: i32, size: i32)`, releases memory from [`EXPORT_ALLOC`].
pub const EXPORT_FREE: &str = "rx_free";
/// Exported: `(sample_rate: f32, max_block_frames: i32, channels: i32) -> i32`.
pub const EXPORT_INIT: &str = "rx_init";
/// Exported: `(index: i32, value: f32)`, parameters in manifest order.
pub const EXPORT_SET_PARAM: &str = "rx_set_param";
/// Exported: `(in_ptr: i32, out_ptr: i32, frames: i32) -> i32`, planar `f32`.
pub const EXPORT_PROCESS: &str = "rx_process";
/// Exported, optional: `()`, back to the state after `rx_init`.
pub const EXPORT_RESET: &str = "rx_reset";

/// The one host import an extension may use: `rustacity.rx_log`.
pub const IMPORT_MODULE: &str = "rustacity";
/// The field of [`IMPORT_MODULE`]: `(level: i32, ptr: i32, len: i32)`.
pub const IMPORT_LOG: &str = "rx_log";

/// The WASI preview 1 module of which every function is linked, hollow.
pub const WASI_MODULE: &str = "wasi_snapshot_preview1";

/// Largest block the host will hand to `rx_process`, in frames.
pub const MAX_BLOCK_FRAMES: usize = 65_536;

/// Default limit on a module's linear memory, in bytes (256 MiB).
pub const DEFAULT_MEMORY_LIMIT: usize = 256 * 1024 * 1024;
/// Default limit on tables a module may create.
pub const DEFAULT_TABLE_LIMIT: usize = 128;
/// Default limit on instances a module may create.
pub const DEFAULT_INSTANCE_LIMIT: usize = 64;
/// Default wall-clock budget for a single `rx_process` call, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;
/// How often the epoch ticker advances the engine clock, in milliseconds.
///
/// The deadline is expressed in ticks, so this is also the resolution of
/// [`DEFAULT_TIMEOUT_MS`]: a call may overrun by up to one tick.
pub const EPOCH_TICK_MS: u64 = 10;

/// Most log messages one `rx_process` call may produce.
pub const MAX_LOG_MESSAGES_PER_CALL: usize = 64;
/// Most bytes of log text one `rx_process` call may produce.
pub const MAX_LOG_BYTES_PER_CALL: usize = 8 * 1024;
/// Longest single log message kept; the rest is truncated.
pub const MAX_LOG_MESSAGE_BYTES: usize = 512;
/// Log level used for messages whose level is out of range.
pub const LOG_LEVEL_DEFAULT: i32 = 2;

/// Capability: the extension reads the input buffer.
pub const CAP_AUDIO_READ: &str = "audio.read";
/// Capability: the extension writes the output buffer.
pub const CAP_AUDIO_WRITE: &str = "audio.write";

/// Capabilities this host implements.
pub const CAPABILITIES: &[&str] = &[CAP_AUDIO_READ, CAP_AUDIO_WRITE];

/// Names reserved for a later API version, refused rather than ignored.
pub const CAPABILITIES_RESERVED: &[&str] = &["labels.write", "tracks.read"];

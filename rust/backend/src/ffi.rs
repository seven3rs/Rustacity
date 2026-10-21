// Rustacity: A Digital Audio Editor
//
// Copyright (C) 2026 Sato
//
// SPDX-License-Identifier: GPL-2.0-or-later

//! C ABI of the Rust audio backend.
//!
//! The declarations live in `include/rustacity_backend.h`.  Every entry point
//! validates its pointers, and [`guard`] turns a Rust panic into a harmless
//! return value instead of unwinding into the C++ frontend.  The C++ side
//! calls these functions through `src/RustBackend.{h,cpp}`.
//!
//! Contributor: Sato <seven3rs@gmail.com>

// The pointer contract is identical for every entry point and is stated once
// in the module docs and the C header; repeating it in twenty `# Safety`
// sections would make the ABI harder, not easier, to audit.
#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::slice;

use crate::{Import, Sequence};

/// Opaque handle for one mono sequence.
#[derive(Debug)]
pub struct RustSequence {
    inner: Sequence,
}

/// Opaque handle for a decoded WAV whose channel sequences can be taken.
#[derive(Debug)]
pub struct RustImport {
    inner: Import,
}

fn guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(_) => fallback,
    }
}

fn boxed_sequence(sequence: Sequence) -> *mut RustSequence {
    Box::into_raw(Box::new(RustSequence { inner: sequence }))
}

unsafe fn as_sequence<'a>(sequence: *const RustSequence) -> Option<&'a RustSequence> {
    sequence.as_ref()
}

unsafe fn as_import<'a>(import: *const RustImport) -> Option<&'a RustImport> {
    import.as_ref()
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_free(sequence: *mut RustSequence) {
    if !sequence.is_null() {
        drop(Box::from_raw(sequence));
    }
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_clone(
    sequence: *const RustSequence,
) -> *mut RustSequence {
    guard(std::ptr::null_mut(), || match as_sequence(sequence) {
        Some(sequence) => boxed_sequence(sequence.inner.clone()),
        None => std::ptr::null_mut(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_slice(
    sequence: *const RustSequence,
    start: u64,
    len: u64,
) -> *mut RustSequence {
    guard(std::ptr::null_mut(), || match as_sequence(sequence) {
        Some(sequence) => boxed_sequence(sequence.inner.slice(start, len)),
        None => std::ptr::null_mut(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_frames(sequence: *const RustSequence) -> u64 {
    guard(0, || {
        as_sequence(sequence).map_or(0, |sequence| sequence.inner.frames())
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_read_f32(
    sequence: *const RustSequence,
    start: u64,
    frames: usize,
    out: *mut f32,
) -> usize {
    guard(0, || {
        let Some(sequence) = as_sequence(sequence) else {
            return 0;
        };
        if out.is_null() || frames == 0 {
            return 0;
        }

        let out = slice::from_raw_parts_mut(out, frames);
        sequence.inner.read(start, frames, out)
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_write_f32(
    sequence: *mut RustSequence,
    start: u64,
    frames: usize,
    input: *const f32,
) -> i32 {
    guard(0, || {
        let Some(sequence) = as_sequence(sequence) else {
            return 0;
        };
        if input.is_null() || frames == 0 {
            return 0;
        }

        let input = slice::from_raw_parts(input, frames);
        sequence.inner.write(start, input);
        1
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_append_f32(
    sequence: *mut RustSequence,
    frames: usize,
    input: *const f32,
) -> i32 {
    guard(0, || {
        let Some(sequence) = as_sequence(sequence) else {
            return 0;
        };
        if input.is_null() || frames == 0 {
            return 0;
        }

        let input = slice::from_raw_parts(input, frames);
        sequence.inner.append(input);
        1
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_insert_f32(
    sequence: *mut RustSequence,
    start: u64,
    frames: usize,
    input: *const f32,
) -> i32 {
    guard(0, || {
        let Some(sequence) = as_sequence(sequence) else {
            return 0;
        };
        if input.is_null() || frames == 0 {
            return 0;
        }

        let input = slice::from_raw_parts(input, frames);
        sequence.inner.insert(start, input);
        1
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_delete(
    sequence: *mut RustSequence,
    start: u64,
    len: u64,
) -> i32 {
    guard(0, || match as_sequence(sequence) {
        Some(sequence) => {
            sequence.inner.delete(start, len);
            1
        }
        None => 0,
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_insert_silence(
    sequence: *mut RustSequence,
    start: u64,
    len: u64,
) -> i32 {
    guard(0, || match as_sequence(sequence) {
        Some(sequence) => {
            sequence.inner.insert_silence(start, len);
            1
        }
        None => 0,
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_set_silence(
    sequence: *mut RustSequence,
    start: u64,
    len: u64,
) -> i32 {
    guard(0, || match as_sequence(sequence) {
        Some(sequence) => {
            sequence.inner.set_silence(start, len);
            1
        }
        None => 0,
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_min_max(
    sequence: *const RustSequence,
    start: u64,
    len: u64,
    min: *mut f32,
    max: *mut f32,
) -> i32 {
    guard(0, || {
        let Some(sequence) = as_sequence(sequence) else {
            return 0;
        };
        if min.is_null() || max.is_null() {
            return 0;
        }

        let (lo, hi) = sequence.inner.min_max(start, len);
        *min = lo;
        *max = hi;
        1
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_rms(
    sequence: *const RustSequence,
    start: u64,
    len: u64,
) -> f32 {
    guard(0.0, || {
        as_sequence(sequence).map_or(0.0, |sequence| sequence.inner.rms(start, len))
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_wave_display(
    sequence: *const RustSequence,
    t0: f64,
    pixels_per_second: f64,
    width: usize,
    where_out: *mut u64,
    min: *mut f32,
    max: *mut f32,
    rms: *mut f32,
    bl: *mut i32,
) -> i32 {
    guard(0, || {
        let Some(sequence) = as_sequence(sequence) else {
            return 0;
        };
        if where_out.is_null() || min.is_null() || max.is_null() || rms.is_null() || bl.is_null() {
            return 0;
        }

        let where_out = slice::from_raw_parts_mut(where_out, width + 1);
        let min = slice::from_raw_parts_mut(min, width);
        let max = slice::from_raw_parts_mut(max, width);
        let rms = slice::from_raw_parts_mut(rms, width);
        let bl = slice::from_raw_parts_mut(bl, width);
        sequence
            .inner
            .wave_display(t0, pixels_per_second, width, where_out, min, max, rms, bl);
        1
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_sequence_wave_display_where(
    sequence: *const RustSequence,
    where_in: *const u64,
    width: usize,
    min: *mut f32,
    max: *mut f32,
    rms: *mut f32,
    bl: *mut i32,
) -> i32 {
    guard(0, || {
        let Some(sequence) = as_sequence(sequence) else {
            return 0;
        };
        if where_in.is_null() || min.is_null() || max.is_null() || rms.is_null() || bl.is_null() {
            return 0;
        }

        let where_in = slice::from_raw_parts(where_in, width + 1);
        let min = slice::from_raw_parts_mut(min, width);
        let max = slice::from_raw_parts_mut(max, width);
        let rms = slice::from_raw_parts_mut(rms, width);
        let bl = slice::from_raw_parts_mut(bl, width);
        sequence
            .inner
            .wave_display_where(where_in, width, min, max, rms, bl);
        1
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_wav_import_open(path: *const c_char) -> *mut RustImport {
    guard(std::ptr::null_mut(), || {
        if path.is_null() {
            return std::ptr::null_mut();
        }

        let Ok(path) = CStr::from_ptr(path).to_str() else {
            return std::ptr::null_mut();
        };

        match Import::from_wav(Path::new(path)) {
            Ok(import) => Box::into_raw(Box::new(RustImport { inner: import })),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_import_channel_count(import: *const RustImport) -> usize {
    guard(0, || {
        as_import(import).map_or(0, |import| import.inner.channel_count())
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_import_sample_rate(import: *const RustImport) -> u32 {
    guard(0, || {
        as_import(import).map_or(0, |import| import.inner.sample_rate())
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_import_take_sequence(
    import: *mut RustImport,
    channel: usize,
) -> *mut RustSequence {
    guard(std::ptr::null_mut(), || {
        let Some(import) = import.as_mut() else {
            return std::ptr::null_mut();
        };
        import
            .inner
            .take(channel)
            .map_or(std::ptr::null_mut(), boxed_sequence)
    })
}

#[no_mangle]
pub unsafe extern "C" fn rustacity_import_free(import: *mut RustImport) {
    if !import.is_null() {
        drop(Box::from_raw(import));
    }
}

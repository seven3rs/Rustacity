// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! A starter extension. Copy this directory, rename the crate in `Cargo.toml`,
//! write your own `extension.toml`, and fill in the TODOs below.
//!
//! `docs/extensions/WRITING_AN_EXTENSION.md` walks through it; the contract is
//! `docs/extensions/API.md`. `rust/extensions/gain-invert` is this template with
//! the TODOs done.

use std::alloc::{alloc, dealloc, Layout};

/// The alignment of a block from [`rx_alloc`]. Keep it, unless you also change
/// `BLOCK_ALIGN` in `rx_free`; the two must match.
const BLOCK_ALIGN: usize = 16;

/// The index of a parameter, in the order `extension.toml` declares them.
const PARAM_AMOUNT: i32 = 0;

/// Everything the extension remembers between calls.
struct State {
    channels: usize,
    max_block: usize,
    initialized: bool,
    amount: f32,
}

static mut STATE: State = State {
    channels: 0,
    max_block: 0,
    initialized: false,
    amount: 0.0,
};

fn state() -> &'static mut State {
    // SAFETY: the host never calls two exports at the same time, so this is the
    // only reference to STATE while it is alive.
    unsafe { &mut *std::ptr::addr_of_mut!(STATE) }
}

/// The API version this extension implements. Leave it at 1 unless you are
/// writing for a later version of the API.
#[no_mangle]
pub extern "C" fn rx_api_version() -> i32 {
    1
}

/// Returns `size` bytes of module memory, or zero. The host calls [`rx_free`]
/// with the same size. If you would rather let your language's allocator do the
/// work, `Vec::with_capacity(size).leak()` and `Vec::from_raw_parts` work too;
/// this is the plain version.
#[no_mangle]
pub extern "C" fn rx_alloc(size: i32) -> i32 {
    if size <= 0 {
        return 0;
    }
    let Ok(layout) = Layout::from_size_align(size as usize, BLOCK_ALIGN) else {
        return 0;
    };
    // SAFETY: the layout is valid and non-zero-sized.
    unsafe { alloc(layout) as i32 }
}

/// Releases a block from [`rx_alloc`].
#[no_mangle]
pub extern "C" fn rx_free(ptr: i32, size: i32) {
    if ptr == 0 || size <= 0 {
        return;
    }
    let Ok(layout) = Layout::from_size_align(size as usize, BLOCK_ALIGN) else {
        return;
    };
    // SAFETY: ptr came from rx_alloc with this size and alignment.
    unsafe { dealloc(ptr as *mut u8, layout) }
}

/// Called once before the first block. Return 0, or a non-zero code of your own
/// that the host will report to the user.
#[no_mangle]
pub extern "C" fn rx_init(sample_rate: f32, max_block_frames: i32, channels: i32) -> i32 {
    let _ = sample_rate; // TODO: resample, or tune a filter to it
    if channels <= 0 || max_block_frames <= 0 {
        return 1;
    }
    let state = state();
    state.channels = channels as usize;
    state.max_block = max_block_frames as usize;
    state.amount = 0.0;
    state.initialized = true;
    0
}

/// A parameter moved. Indices are the manifest's order; a bool arrives as 0.0
/// or 1.0 and a choice as its index.
#[no_mangle]
pub extern "C" fn rx_set_param(index: i32, value: f32) {
    let state = state();
    match index {
        PARAM_AMOUNT => state.amount = value,
        _ => {}
    }
}

/// Processes `frames` frames of planar `f32` audio. This one does nothing;
/// TODO: replace the body with your processing.
#[no_mangle]
pub extern "C" fn rx_process(in_ptr: i32, out_ptr: i32, frames: i32) -> i32 {
    let state = state();
    if !state.initialized {
        return 1;
    }
    if frames <= 0 || frames as usize > state.max_block {
        return 2;
    }
    if in_ptr == 0 || out_ptr == 0 {
        return 3;
    }
    let _ = state.amount;

    let frames = frames as usize;
    // SAFETY: the host allocated both buffers with channels * max_block * 4
    // bytes through rx_alloc, and frames <= max_block, so this stays inside.
    unsafe {
        let input = in_ptr as *const f32;
        let output = out_ptr as *mut f32;
        for channel in 0..state.channels {
            for index in 0..frames {
                let offset = channel * frames + index;
                // TODO: this is the part that matters. Right now: pass through.
                *output.add(offset) = *input.add(offset);
            }
        }
    }
    0
}

/// Optional. The host calls it when it seeks or restarts, and treats its
/// absence as a no-op. TODO: clear filters, delays, envelopes.
#[no_mangle]
pub extern "C" fn rx_reset() {
    let state = state();
    state.amount = 0.0;
}

// Logging, if you want it: the host provides one import, and rate-limits it.
//
// #[link(wasm_import_module = "rustacity")]
// extern "C" {
//     fn rx_log(level: i32, ptr: i32, len: i32);
// }
//
// fn log(message: &str) {
//     // SAFETY: message is a live &str; rx_log reads len bytes from it.
//     unsafe { rx_log(2, message.as_ptr() as i32, message.len() as i32) }
// }

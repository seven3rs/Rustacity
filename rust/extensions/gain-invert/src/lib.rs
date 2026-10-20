// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Gain / Invert: the first extension of the Rustacity extension engine, and
//! the one the host's tests measure everything else against.
//!
//! It implements the whole of API version 1 and nothing more: eight exports, no
//! host imports, no capabilities beyond audio.read and audio.write. Read it
//! together with `docs/extensions/API.md`; `rust/extensions/template` is the
//! same shape with the specific parts taken out.
//!
//! Two parameters, in the order `extension.toml` declares them:
//!
//! | index | id | kind | meaning |
//! | --- | --- | --- | --- |
//! | 0 | `gain_db` | float, -60..24 | gain in decibels, 0 dB is unity |
//! | 1 | `invert` | bool | flip the polarity |
//!
//! A parameter set before the first block takes effect immediately, so a fresh
//! run is an exact multiply. A parameter changed once processing has started is
//! slewed over [`SLEW_FRAMES`] frames, linearly, so that dragging a slider does
//! not click.

use std::alloc::{alloc, dealloc, Layout};

/// `gain_db`: index 0 in `extension.toml`.
const PARAM_GAIN_DB: i32 = 0;
/// `invert`: index 1 in `extension.toml`.
const PARAM_INVERT: i32 = 1;

/// Alignment of every block handed out by [`rx_alloc`]. `rx_free` is called
/// with the same size, and the layout has to match to be freed.
const BLOCK_ALIGN: usize = 16;

/// The range `extension.toml` declares for `gain_db`.
const GAIN_DB_MIN: f32 = -60.0;
const GAIN_DB_MAX: f32 = 24.0;

/// Frames a gain change takes to reach its target once processing has started.
const SLEW_FRAMES: f32 = 128.0;

/// Everything this extension remembers. One `Store` per instance means one
/// module instance, and the host never calls in concurrently, so a single
/// mutable static is the whole of the state.
struct State {
    sample_rate: f32,
    channels: usize,
    max_block: usize,
    initialized: bool,
    /// Set by the first `rx_process`, and what decides whether a parameter
    /// change snaps or slews.
    processing: bool,
    gain_db: f32,
    invert: bool,
    /// Gain as it is being applied right now, and where it is heading.
    current: f32,
    target: f32,
    /// Gain the current slew started from, and how many frames of it are left.
    from: f32,
    remaining: f32,
}

static mut STATE: State = State {
    sample_rate: 0.0,
    channels: 0,
    max_block: 0,
    initialized: false,
    processing: false,
    gain_db: 0.0,
    invert: false,
    current: 1.0,
    target: 1.0,
    from: 1.0,
    remaining: 0.0,
};

/// The one instance's state.
fn state() -> &'static mut State {
    // SAFETY: the module runs in a single-threaded store and the host
    // guarantees it never calls two exports at the same time, so there is no
    // other reference to STATE alive while this one is used.
    unsafe { &mut *std::ptr::addr_of_mut!(STATE) }
}

/// Turns decibels into the factor to multiply samples by.
fn gain_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Recomputes the gain the next sample should use, after `gain_db` or `invert`
/// moved.
fn retune(state: &mut State) {
    let clamped = state.gain_db.clamp(GAIN_DB_MIN, GAIN_DB_MAX);
    let linear = gain_to_linear(clamped);
    state.target = if state.invert { -linear } else { linear };
    if state.processing {
        state.from = state.current;
        state.remaining = SLEW_FRAMES;
    } else {
        state.current = state.target;
        state.from = state.target;
        state.remaining = 0.0;
    }
}

// ---------------------------------------------------------------------------
// The ABI. See docs/extensions/API.md section 3.
// ---------------------------------------------------------------------------

/// The API version this extension implements.
#[no_mangle]
pub extern "C" fn rx_api_version() -> i32 {
    1
}

/// Allocates `size` bytes in this module's memory and returns the pointer, or
/// zero if the request cannot be met. The host passes the same `size` to
/// [`rx_free`].
#[no_mangle]
pub extern "C" fn rx_alloc(size: i32) -> i32 {
    if size <= 0 {
        return 0;
    }
    let Ok(layout) = Layout::from_size_align(size as usize, BLOCK_ALIGN) else {
        return 0;
    };
    // SAFETY: the layout is a valid, non-zero-size layout.
    let block = unsafe { alloc(layout) };
    block as i32
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
    // SAFETY: ptr came from rx_alloc with this exact size and alignment, and
    // the host calls rx_free once per block, after the last use.
    unsafe { dealloc(ptr as *mut u8, layout) }
}

/// Prepares for a run. Returns 0 on success.
#[no_mangle]
pub extern "C" fn rx_init(sample_rate: f32, max_block_frames: i32, channels: i32) -> i32 {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return 1;
    }
    if channels <= 0 || max_block_frames <= 0 {
        return 2;
    }
    let state = state();
    state.sample_rate = sample_rate;
    state.channels = channels as usize;
    state.max_block = max_block_frames as usize;
    state.initialized = true;
    state.processing = false;
    state.gain_db = 0.0;
    state.invert = false;
    state.current = 1.0;
    state.target = 1.0;
    state.from = 1.0;
    state.remaining = 0.0;
    0
}

/// Sets parameter `index`, in manifest order. The host has already checked the
/// value against the manifest; this clamps as well, so that a host which did
/// not is still not dangerous.
#[no_mangle]
pub extern "C" fn rx_set_param(index: i32, value: f32) {
    let state = state();
    match index {
        PARAM_GAIN_DB => state.gain_db = value,
        PARAM_INVERT => state.invert = value != 0.0,
        _ => return,
    }
    retune(state);
}

/// Processes `frames` frames of planar `f32` audio. Returns 0 on success.
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
    let frames = frames as usize;
    state.processing = true;

    // SAFETY: the host allocated both buffers through rx_alloc with
    // channels * max_block * 4 bytes, and frames <= max_block, so every access
    // below stays inside them. The pointers are aligned for f32 by rx_alloc.
    unsafe {
        let input = in_ptr as *const f32;
        let output = out_ptr as *mut f32;
        for index in 0..frames {
            // One step per frame, not per sample: every channel of a frame is
            // at the same point of the slew, or the image shifts sideways while
            // the gain moves.
            if state.remaining > 0.0 {
                state.remaining -= 1.0;
                if state.remaining <= 0.0 {
                    state.remaining = 0.0;
                    state.current = state.target;
                } else {
                    state.current = state.from
                        + (state.target - state.from) * (1.0 - state.remaining / SLEW_FRAMES);
                }
            }
            let gain = state.current;
            for channel in 0..state.channels {
                let offset = channel * frames + index;
                *output.add(offset) = *input.add(offset) * gain;
            }
        }
    }
    0
}

/// Forgets the slew and the fact that processing started: the state a fresh
/// `rx_init` would leave behind.
#[no_mangle]
pub extern "C" fn rx_reset() {
    let state = state();
    state.processing = false;
    state.current = state.target;
    state.from = state.target;
    state.remaining = 0.0;
}

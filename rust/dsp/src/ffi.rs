// Rustacity: A Digital Audio Editor
//
// Copyright (C) 2026 Sadik Pranto
//
// SPDX-License-Identifier: GPL-2.0-or-later

//! C ABI of the DSP core.
//!
//! The declarations live in `include/rustacity_dsp.h`; every function
//! here mirrors one C++ entry point of the original implementation.
//! Pointers are validated before use and no panic may cross the FFI
//! boundary: each entry point runs under [`guard`], which turns a panic
//! into a harmless return value instead of unwinding into C++.
//!
//! Contributor: Sadik Pranto <pranto.smss.dev@gmail.com>

use core::ffi::c_int;
use core::slice;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::bass_treble::BassTreble;
use crate::biquad::{self, Biquad};
use crate::distortion::Distortion;
use crate::ebur128::{self, Ebur128};
use crate::fft;
use crate::phaser::Phaser;
use crate::wahwah::Wahwah;

/// `rustacity_biquad_t` is 9 doubles - the C++ side asserts that the
/// C++ `Biquad` has exactly the same size.
const _: () =
    assert!(core::mem::size_of::<Biquad>() == 9 * core::mem::size_of::<f64>());

/// Run `f`, returning `fallback` if it panics.
///
/// Every entry point below is wrapped in this guard, because the C++
/// caller cannot cope with an unwind and aborting the process would
/// cost the user whatever they had not saved.  The default panic hook
/// still reports where the panic happened, so the fault stays
/// diagnosable.
///
/// `AssertUnwindSafe` is needed because the closures capture the raw
/// pointers that came from C++, which are not `UnwindSafe`.  That is
/// sound here: a caught panic cannot leave anything observable to the
/// caller.
fn guard<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => fallback,
    }
}

/// Copy `sections` into the caller's buffer, at most `cap` entries,
/// and return how many were written.
fn write_sections(out: *mut Biquad, cap: usize, sections: Vec<Biquad>) -> c_int {
    if out.is_null() {
        return 0;
    }
    let n = sections.len().min(cap);
    for (i, section) in sections.into_iter().take(n).enumerate() {
        // SAFETY: `out` is non-null and has room for `n <= cap` entries
        // by contract of the caller.
        unsafe { out.add(i).write(section) };
    }
    n as c_int
}

#[no_mangle]
pub extern "C" fn rustacity_biquad_reset(bq: *mut Biquad) {
    guard((), || {
        if bq.is_null() {
            return;
        }
        // SAFETY: non-null, aligned and dereferenceable by contract.
        unsafe { (*bq).reset() };
    });
}

#[no_mangle]
pub extern "C" fn rustacity_biquad_process(
    bq: *mut Biquad,
    input: *const f32,
    output: *mut f32,
    num_samples: c_int,
) {
    guard((), || {
        if bq.is_null() || input.is_null() || output.is_null() || num_samples <= 0 {
            return;
        }
        // SAFETY: the caller guarantees valid pointers for `num_samples`
        // samples; input and output may alias (the loop reads each sample
        // before writing it), so raw pointers are used throughout.
        let bq = unsafe { &mut *bq };
        for i in 0..num_samples as isize {
            let x = unsafe { *input.offset(i) };
            let y = bq.process_one(x);
            unsafe { *output.offset(i) = y };
        }
    });
}

#[no_mangle]
pub extern "C" fn rustacity_biquad_design_butterworth(
    out: *mut Biquad,
    cap: usize,
    order: c_int,
    fn_hz: f64,
    fc: f64,
    subtype: c_int,
) -> c_int {
    guard(0, || {
        write_sections(
            out,
            cap,
            biquad::calc_butterworth_filter(order, fn_hz, fc, subtype),
        )
    })
}

#[no_mangle]
pub extern "C" fn rustacity_biquad_design_chebyshev1(
    out: *mut Biquad,
    cap: usize,
    order: c_int,
    fn_hz: f64,
    fc: f64,
    ripple: f64,
    subtype: c_int,
) -> c_int {
    guard(0, || {
        write_sections(
            out,
            cap,
            biquad::calc_chebyshev_type1_filter(order, fn_hz, fc, ripple, subtype),
        )
    })
}

#[no_mangle]
pub extern "C" fn rustacity_biquad_design_chebyshev2(
    out: *mut Biquad,
    cap: usize,
    order: c_int,
    fn_hz: f64,
    fc: f64,
    ripple: f64,
    subtype: c_int,
) -> c_int {
    guard(0, || {
        write_sections(
            out,
            cap,
            biquad::calc_chebyshev_type2_filter(order, fn_hz, fc, ripple, subtype),
        )
    })
}

#[no_mangle]
pub extern "C" fn rustacity_biquad_cheby_poly(order: c_int, norm_freq: f64) -> f64 {
    guard(0.0, || {
        if !(biquad::MIN_ORDER..=biquad::MAX_ORDER).contains(&order) {
            return 0.0;
        }
        biquad::cheby_poly(order, norm_freq)
    })
}

/// Create the shelf filters of EffectBassTreble for one channel, or null if
/// the arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_bass_treble_new(sample_rate: f32) -> *mut BassTreble {
    guard(core::ptr::null_mut(), || {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return core::ptr::null_mut();
        }
        Box::into_raw(Box::new(BassTreble::new(sample_rate)))
    })
}

#[no_mangle]
pub extern "C" fn rustacity_bass_treble_free(handle: *mut BassTreble) {
    guard((), || {
        if handle.is_null() {
            return;
        }
        // SAFETY: the pointer came from `rustacity_bass_treble_new` and is
        // consumed exactly once.
        unsafe { drop(Box::from_raw(handle)) };
    });
}

/// Filter `len` samples of one channel from `input` to `output`, which may
/// alias, with the effect's three parameters for this block; returns `len`,
/// or 0 if the arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_bass_treble_process(
    handle: *mut BassTreble,
    bass_db: f64,
    treble_db: f64,
    gain_db: f64,
    input: *const f32,
    output: *mut f32,
    len: usize,
) -> usize {
    guard(0, || {
        if handle.is_null() || input.is_null() || output.is_null() {
            return 0;
        }
        // SAFETY: non-null, aligned and dereferenceable by contract; input
        // and output may alias (each sample is read before it is written),
        // so raw pointers are used throughout.
        let handle = unsafe { &mut *handle };
        handle.set_parameters(bass_db, treble_db, gain_db);
        for i in 0..len {
            let x = unsafe { *input.add(i) };
            let y = handle.process_one(x);
            unsafe { *output.add(i) = y };
        }
        len
    })
}

/// Create the allpass filter of EffectPhaser for one channel, or null if the
/// arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_phaser_new(sample_rate: f32) -> *mut Phaser {
    guard(core::ptr::null_mut(), || {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return core::ptr::null_mut();
        }
        Box::into_raw(Box::new(Phaser::new(sample_rate)))
    })
}

#[no_mangle]
pub extern "C" fn rustacity_phaser_free(handle: *mut Phaser) {
    guard((), || {
        if handle.is_null() {
            return;
        }
        // SAFETY: the pointer came from `rustacity_phaser_new` and is
        // consumed exactly once.
        unsafe { drop(Box::from_raw(handle)) };
    });
}

/// Filter `len` samples of one channel from `input` to `output`, which may
/// alias, with the effect's settings for this block; returns `len`, or 0 if
/// the arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_phaser_process(
    handle: *mut Phaser,
    stages: c_int,
    dry_wet: c_int,
    freq: f64,
    phase_deg: f64,
    depth: c_int,
    feedback: c_int,
    out_gain_db: f64,
    input: *const f32,
    output: *mut f32,
    len: usize,
) -> usize {
    guard(0, || {
        if handle.is_null() || input.is_null() || output.is_null() {
            return 0;
        }
        let params = crate::phaser::Params {
            stages,
            dry_wet,
            freq,
            phase_deg,
            depth,
            feedback,
            out_gain_db,
        };
        // SAFETY: non-null, aligned and dereferenceable by contract; input
        // and output may alias (each sample is read before it is written).
        unsafe { (*handle).process_raw(params, input, output, len) };
        len
    })
}

/// Create the wah filter of EffectWahwah for one channel, or null if the
/// arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_wahwah_new(sample_rate: f32) -> *mut Wahwah {
    guard(core::ptr::null_mut(), || {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return core::ptr::null_mut();
        }
        Box::into_raw(Box::new(Wahwah::new(sample_rate)))
    })
}

#[no_mangle]
pub extern "C" fn rustacity_wahwah_free(handle: *mut Wahwah) {
    guard((), || {
        if handle.is_null() {
            return;
        }
        // SAFETY: the pointer came from `rustacity_wahwah_new` and is consumed
        // exactly once.
        unsafe { drop(Box::from_raw(handle)) };
    });
}

/// Filter `len` samples of one channel from `input` to `output`, which may
/// alias, with the effect's settings for this block; returns `len`, or 0 if
/// the arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_wahwah_process(
    handle: *mut Wahwah,
    freq: f64,
    phase_deg: f64,
    depth: c_int,
    res: f64,
    freq_ofs: c_int,
    out_gain_db: f64,
    input: *const f32,
    output: *mut f32,
    len: usize,
) -> usize {
    guard(0, || {
        if handle.is_null() || input.is_null() || output.is_null() {
            return 0;
        }
        let params = crate::wahwah::Params {
            freq,
            phase_deg,
            depth,
            res,
            freq_ofs,
            out_gain_db,
        };
        // SAFETY: non-null, aligned and dereferenceable by contract; input and
        // output may alias (each sample is read before it is written).
        unsafe { (*handle).process_raw(params, input, output, len) };
        len
    })
}

/// Create the waveshaper of EffectDistortion for one channel, or null if the
/// arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_distortion_new(sample_rate: f32) -> *mut Distortion {
    guard(core::ptr::null_mut(), || {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return core::ptr::null_mut();
        }
        Box::into_raw(Box::new(Distortion::new(sample_rate)))
    })
}

#[no_mangle]
pub extern "C" fn rustacity_distortion_free(handle: *mut Distortion) {
    guard((), || {
        if handle.is_null() {
            return;
        }
        // SAFETY: the pointer came from `rustacity_distortion_new` and is
        // consumed exactly once.
        unsafe { drop(Box::from_raw(handle)) };
    });
}

/// Shape `len` samples of one channel from `input` to `output`, which may
/// alias, with the effect's settings for this block; returns `len`, or 0 if
/// the arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_distortion_process(
    handle: *mut Distortion,
    table_choice: c_int,
    dc_block: c_int,
    threshold_db: f64,
    noise_floor: f64,
    param1: f64,
    param2: f64,
    repeats: c_int,
    input: *const f32,
    output: *mut f32,
    len: usize,
) -> usize {
    guard(0, || {
        if handle.is_null() || input.is_null() || output.is_null() {
            return 0;
        }
        let params = crate::distortion::Params {
            table_choice,
            dc_block: dc_block != 0,
            threshold_db,
            noise_floor,
            param1,
            param2,
            repeats,
        };
        // SAFETY: non-null, aligned and dereferenceable by contract; input and
        // output may alias (each sample is read before it is written).
        unsafe { (*handle).process_raw(params, input, output, len) };
        len
    })
}

/// Create a loudness meter, or null if the arguments are unusable.
#[no_mangle]
pub extern "C" fn rustacity_ebur128_new(rate: f64, channels: usize) -> *mut Ebur128 {
    guard(core::ptr::null_mut(), || {
        if !rate.is_finite() || rate <= 0.0 || channels == 0 {
            return core::ptr::null_mut();
        }
        Box::into_raw(Box::new(Ebur128::new(rate, channels)))
    })
}

#[no_mangle]
pub extern "C" fn rustacity_ebur128_free(meter: *mut Ebur128) {
    guard((), || {
        if meter.is_null() {
            return;
        }
        // SAFETY: the pointer came from `rustacity_ebur128_new` and is
        // consumed exactly once.
        unsafe { drop(Box::from_raw(meter)) };
    });
}

#[no_mangle]
pub extern "C" fn rustacity_ebur128_initialize(meter: *mut Ebur128) {
    guard((), || {
        // SAFETY: `meter.as_mut()` is null-checked.
        if let Some(meter) = unsafe { meter.as_mut() } {
            meter.initialize();
        }
    });
}

#[no_mangle]
pub extern "C" fn rustacity_ebur128_process_sample(meter: *mut Ebur128, x: f32, channel: usize) {
    guard((), || {
        // SAFETY: `meter.as_mut()` is null-checked.
        if let Some(meter) = unsafe { meter.as_mut() } {
            meter.process_sample_from_channel(x, channel);
        }
    });
}

#[no_mangle]
pub extern "C" fn rustacity_ebur128_next_sample(meter: *mut Ebur128) {
    guard((), || {
        // SAFETY: `meter.as_mut()` is null-checked.
        if let Some(meter) = unsafe { meter.as_mut() } {
            meter.next_sample();
        }
    });
}

#[no_mangle]
pub extern "C" fn rustacity_ebur128_process_block(
    meter: *mut Ebur128,
    channels: *const *const f32,
    channel_count: usize,
    len: usize,
) {
    guard((), || {
        if channels.is_null() {
            return;
        }
        // SAFETY: `meter.as_mut()` is null-checked.
        let Some(meter) = (unsafe { meter.as_mut() }) else {
            return;
        };
        let count = channel_count.min(meter.channel_count());
        let mut slices: Vec<&[f32]> = Vec::with_capacity(count);
        for i in 0..count {
            // SAFETY: the caller guarantees `channel_count` valid pointers.
            let channel = unsafe { *channels.add(i) };
            if channel.is_null() {
                return;
            }
            // SAFETY: the caller guarantees `len` readable samples.
            slices.push(unsafe { core::slice::from_raw_parts(channel, len) });
        }
        meter.process_block(&slices);
    });
}

#[no_mangle]
pub extern "C" fn rustacity_ebur128_integrative_loudness(meter: *mut Ebur128) -> f64 {
    guard(0.0, || {
        // SAFETY: `meter.as_mut()` is null-checked.
        match unsafe { meter.as_mut() } {
            Some(meter) => meter.integrative_loudness(),
            None => 0.0,
        }
    })
}

/// Fill `out[0..2]` with the K-weighting filter pair (HSF, HPF).
#[no_mangle]
pub extern "C" fn rustacity_ebur128_weighting_filter(
    fs: f64,
    out: *mut Biquad,
    cap: usize,
) -> c_int {
    guard(0, || {
        write_sections(out, cap, ebur128::calc_weighting_filter(fs).to_vec())
    })
}

/// The FFT and the window functions.
///
/// The tables a transform runs on belong to the C++ side, which pools one pair
/// per length (its `GetFFT`); every entry point here takes them as arguments.
/// `fftlen` is the whole transform: `fftlen / 2` ints of bit-reversal pointers
/// and `fftlen` floats of sine values.
///
/// The C++ ran its transforms on lengths it had not checked - the complex one
/// used to end the process on a length that was not a power of two - so the
/// lengths and the pointers are checked here, and a call that cannot be run
/// does nothing instead of reading out of its buffers.
///
/// The table pointers must not overlap the transform buffer.

/// Whether a length can be transformed: at least two complex bins and a power
/// of two.
fn usable_fftlen(fftlen: usize) -> bool {
    fftlen >= 4 && fftlen.is_power_of_two()
}

/// The tables of `fftlen` samples: bit-reversal pointers and sine values.
///
/// # Safety
///
/// `bit_reversed` must be valid for `fftlen / 2` ints and `sin_table` for
/// `fftlen` floats.
unsafe fn tables<'a>(fftlen: usize, bit_reversed: *const i32, sin_table: *const f32) -> (&'a [i32], &'a [f32]) {
    (
        unsafe { slice::from_raw_parts(bit_reversed, fftlen / 2) },
        unsafe { slice::from_raw_parts(sin_table, fftlen) },
    )
}

/// Fill the bit-reversal and sine tables of `fftlen` samples, which is the
/// C++ `InitializeFFT` without its allocation: `bit_reversed` holds `fftlen / 2`
/// ints and `sin_table` `fftlen` floats, and both are owned by the caller.
#[no_mangle]
pub extern "C" fn rustacity_fft_init_tables(
    fftlen: usize,
    bit_reversed: *mut i32,
    sin_table: *mut f32,
) {
    guard((), || {
        if bit_reversed.is_null() || sin_table.is_null() || !usable_fftlen(fftlen) {
            return;
        }
        // SAFETY: the caller allocated both tables for this length.
        let bit_reversed = unsafe { slice::from_raw_parts_mut(bit_reversed, fftlen / 2) };
        let sin_table = unsafe { slice::from_raw_parts_mut(sin_table, fftlen) };
        fft::init_tables(fftlen, bit_reversed, sin_table);
    });
}

/// The forward transform of a real signal, in place on a buffer of `fftlen`
/// floats whose output is bit-reversed, the Fs/2 component in the imaginary
/// half of the DC bin: `RealFFTf`.
#[no_mangle]
pub extern "C" fn rustacity_fft_forward(
    buffer: *mut f32,
    fftlen: usize,
    bit_reversed: *const i32,
    sin_table: *const f32,
) {
    guard((), || {
        if buffer.is_null() || bit_reversed.is_null() || sin_table.is_null() || !usable_fftlen(fftlen) {
            return;
        }
        // SAFETY: by contract the buffer holds `fftlen` floats and the tables
        // were built for that length; none of the three overlap.
        let buffer = unsafe { slice::from_raw_parts_mut(buffer, fftlen) };
        let (bit_reversed, sin_table) = unsafe { tables(fftlen, bit_reversed, sin_table) };
        fft::forward(buffer, bit_reversed, sin_table);
    });
}

/// The inverse transform to real data, in place on a bit-reversed buffer of
/// `fftlen` conjugate-symmetric floats: `InverseRealFFTf`.
#[no_mangle]
pub extern "C" fn rustacity_fft_inverse(
    buffer: *mut f32,
    fftlen: usize,
    bit_reversed: *const i32,
    sin_table: *const f32,
) {
    guard((), || {
        if buffer.is_null() || bit_reversed.is_null() || sin_table.is_null() || !usable_fftlen(fftlen) {
            return;
        }
        // SAFETY: as for `rustacity_fft_forward`.
        let buffer = unsafe { slice::from_raw_parts_mut(buffer, fftlen) };
        let (bit_reversed, sin_table) = unsafe { tables(fftlen, bit_reversed, sin_table) };
        fft::inverse(buffer, bit_reversed, sin_table);
    });
}

/// Undo the bit-reversal of a forward transform into the bins in order, DC and
/// Fs/2 included: `ReorderToFreq`.  `real_out` and `imag_out` hold
/// `fftlen / 2 + 1` floats each.
#[no_mangle]
pub extern "C" fn rustacity_fft_reorder_to_freq(
    bit_reversed: *const i32,
    fftlen: usize,
    buffer: *const f32,
    real_out: *mut f32,
    imag_out: *mut f32,
) {
    guard((), || {
        if bit_reversed.is_null()
            || buffer.is_null()
            || real_out.is_null()
            || imag_out.is_null()
            || !usable_fftlen(fftlen)
        {
            return;
        }
        // SAFETY: by contract the buffer holds `fftlen` floats and the two
        // outputs `fftlen / 2 + 1` floats each.
        let buffer = unsafe { slice::from_raw_parts(buffer, fftlen) };
        let bit_reversed = unsafe { slice::from_raw_parts(bit_reversed, fftlen / 2) };
        let real_out = unsafe { slice::from_raw_parts_mut(real_out, fftlen / 2 + 1) };
        let imag_out = unsafe { slice::from_raw_parts_mut(imag_out, fftlen / 2 + 1) };
        fft::reorder_to_freq(bit_reversed, buffer, real_out, imag_out);
    });
}

/// Undo the bit-reversal of an inverse transform into a real, interleaved
/// time-domain buffer of `fftlen` floats: `ReorderToTime`.
#[no_mangle]
pub extern "C" fn rustacity_fft_reorder_to_time(
    bit_reversed: *const i32,
    fftlen: usize,
    buffer: *const f32,
    time_out: *mut f32,
) {
    guard((), || {
        if bit_reversed.is_null() || buffer.is_null() || time_out.is_null() || !usable_fftlen(fftlen) {
            return;
        }
        // SAFETY: by contract the buffer and the output each hold `fftlen`
        // floats.
        let buffer = unsafe { slice::from_raw_parts(buffer, fftlen) };
        let bit_reversed = unsafe { slice::from_raw_parts(bit_reversed, fftlen / 2) };
        let time_out = unsafe { slice::from_raw_parts_mut(time_out, fftlen) };
        fft::reorder_to_time(bit_reversed, buffer, time_out);
    });
}

/// `RealFFT`: the transform of `fftlen` real samples into a conjugate-symmetric
/// spectrum of `fftlen` values in `real_out` and `imag_out`.
#[no_mangle]
pub extern "C" fn rustacity_fft_real(
    fftlen: usize,
    real_in: *const f32,
    real_out: *mut f32,
    imag_out: *mut f32,
    bit_reversed: *const i32,
    sin_table: *const f32,
) {
    guard((), || {
        if real_in.is_null()
            || real_out.is_null()
            || imag_out.is_null()
            || bit_reversed.is_null()
            || sin_table.is_null()
            || !usable_fftlen(fftlen)
        {
            return;
        }
        // SAFETY: by contract the input and both outputs hold `fftlen` floats.
        let real_in = unsafe { slice::from_raw_parts(real_in, fftlen) };
        let real_out = unsafe { slice::from_raw_parts_mut(real_out, fftlen) };
        let imag_out = unsafe { slice::from_raw_parts_mut(imag_out, fftlen) };
        let (bit_reversed, sin_table) = unsafe { tables(fftlen, bit_reversed, sin_table) };
        fft::real_fft(real_in, real_out, imag_out, bit_reversed, sin_table);
    });
}

/// `InverseRealFFT`: the inverse of `rustacity_fft_real`, whose first half is
/// assumed conjugate-symmetric so the output of `fftlen` floats is purely real.
/// `imag_in` may be null, in which case the imaginary parts are zero.
#[no_mangle]
pub extern "C" fn rustacity_fft_inverse_real(
    fftlen: usize,
    real_in: *const f32,
    imag_in: *const f32,
    real_out: *mut f32,
    bit_reversed: *const i32,
    sin_table: *const f32,
) {
    guard((), || {
        if real_in.is_null()
            || real_out.is_null()
            || bit_reversed.is_null()
            || sin_table.is_null()
            || !usable_fftlen(fftlen)
        {
            return;
        }
        // SAFETY: by contract the input arrays hold `fftlen` floats each, as
        // does the output; `imag_in` is only read when it is not null.
        let real_in = unsafe { slice::from_raw_parts(real_in, fftlen) };
        let imag_in = if imag_in.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(imag_in, fftlen) })
        };
        let real_out = unsafe { slice::from_raw_parts_mut(real_out, fftlen) };
        let (bit_reversed, sin_table) = unsafe { tables(fftlen, bit_reversed, sin_table) };
        fft::inverse_real_fft(real_in, imag_in, real_out, bit_reversed, sin_table);
    });
}

/// `PowerSpectrum`: the squared magnitude of each coefficient of the real
/// transform, `fftlen / 2 + 1` values in `out`.
#[no_mangle]
pub extern "C" fn rustacity_fft_power_spectrum(
    fftlen: usize,
    input: *const f32,
    out: *mut f32,
    bit_reversed: *const i32,
    sin_table: *const f32,
) {
    guard((), || {
        if input.is_null()
            || out.is_null()
            || bit_reversed.is_null()
            || sin_table.is_null()
            || !usable_fftlen(fftlen)
        {
            return;
        }
        // SAFETY: by contract the input holds `fftlen` floats and the output
        // `fftlen / 2 + 1`.
        let input = unsafe { slice::from_raw_parts(input, fftlen) };
        let out = unsafe { slice::from_raw_parts_mut(out, fftlen / 2 + 1) };
        let (bit_reversed, sin_table) = unsafe { tables(fftlen, bit_reversed, sin_table) };
        fft::power_spectrum(input, out, bit_reversed, sin_table);
    });
}

/// The complex transform of `num_samples` samples, forwards or inverse, from
/// `real_in` and `imag_in` (which may be null) into `real_out` and `imag_out`:
/// the `FFT` of the C++ tree.  The bit table it keeps is released by
/// `rustacity_fft_deinit`.
#[no_mangle]
pub extern "C" fn rustacity_fft_complex(
    num_samples: usize,
    inverse_transform: c_int,
    real_in: *const f32,
    imag_in: *const f32,
    real_out: *mut f32,
    imag_out: *mut f32,
) {
    guard((), || {
        if num_samples < 2
            || !num_samples.is_power_of_two()
            || real_in.is_null()
            || real_out.is_null()
            || imag_out.is_null()
        {
            return;
        }
        // SAFETY: by contract the input arrays hold `num_samples` floats each,
        // as do the outputs; `imag_in` is only read when it is not null.
        let real_in = unsafe { slice::from_raw_parts(real_in, num_samples) };
        let imag_in = if imag_in.is_null() {
            None
        } else {
            Some(unsafe { slice::from_raw_parts(imag_in, num_samples) })
        };
        let real_out = unsafe { slice::from_raw_parts_mut(real_out, num_samples) };
        let imag_out = unsafe { slice::from_raw_parts_mut(imag_out, num_samples) };
        fft::fft(real_in, imag_in, real_out, imag_out, inverse_transform != 0);
    });
}

/// Release what `rustacity_fft_complex` caches: the C++ `DeinitFFT`.
#[no_mangle]
pub extern "C" fn rustacity_fft_deinit() {
    guard((), fft::deinit_fft);
}

/// `NewWindowFunc`: multiply `num_samples` values in place by the chosen window
/// function, whose selector is the C++ `eWindowFunctions` value.
#[no_mangle]
pub extern "C" fn rustacity_fft_window_func(
    which_function: c_int,
    num_samples: usize,
    extra_sample: c_int,
    data: *mut f32,
) {
    guard((), || {
        if data.is_null() || num_samples == 0 {
            return;
        }
        // SAFETY: by contract the buffer holds `num_samples` floats, which is
        // as far as the window functions reach.
        let data = unsafe { slice::from_raw_parts_mut(data, num_samples) };
        fft::window_func(which_function, num_samples, extra_sample != 0, data);
    });
}

/// `DerivativeOfWindowFunc`: multiply `num_samples` values in place by the
/// derivative of the chosen window function.
#[no_mangle]
pub extern "C" fn rustacity_fft_derivative_of_window_func(
    which_function: c_int,
    num_samples: usize,
    extra_sample: c_int,
    data: *mut f32,
) {
    guard((), || {
        if data.is_null() || num_samples == 0 {
            return;
        }
        // SAFETY: as for `rustacity_fft_window_func`.
        let data = unsafe { slice::from_raw_parts_mut(data, num_samples) };
        fft::derivative_of_window_func(which_function, num_samples, extra_sample != 0, data);
    });
}

/// `InterpolateAudio`: fill `num_bad` samples of the caller's buffer from
/// `first_bad` with values interpolated from the samples around them.  The
/// noise the interpolation adds comes from a generator the core starts at the
/// same place every call, so the same arguments give the same output.
#[no_mangle]
pub extern "C" fn rustacity_interpolate_audio(
    buffer: *mut f32,
    len: usize,
    first_bad: usize,
    num_bad: usize,
) {
    guard((), || {
        if buffer.is_null() || len == 0 {
            return;
        }
        // SAFETY: by contract the caller owns `len` floats here, and the
        // entry point writes only inside the run it was given.
        let buffer = unsafe { slice::from_raw_parts_mut(buffer, len) };
        crate::interpolate_audio::interpolate_audio(buffer, first_bad, num_bad);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the guard: a panic inside the core comes back
    /// as a value, never as an unwind into C++ and never as an abort.
    /// The panic message printed by the default hook is expected here.
    #[test]
    fn guard_turns_a_panic_into_the_fallback() {
        assert_eq!(guard(7, || panic!("deliberate test panic")), 7);
    }

    #[test]
    fn guard_passes_a_normal_result_through() {
        assert_eq!(guard(7, || 9), 9);
    }

    /// The nulls and invalid values a C++ caller can produce must be
    /// tolerated, not turned into a process abort.
    #[test]
    fn entry_points_tolerate_bad_arguments() {
        rustacity_biquad_reset(core::ptr::null_mut());
        rustacity_biquad_process(
            core::ptr::null_mut(),
            core::ptr::null(),
            core::ptr::null_mut(),
            16,
        );
        assert_eq!(
            rustacity_biquad_design_butterworth(core::ptr::null_mut(), 0, 3, 48000.0, 1000.0, 0),
            0
        );
        assert_eq!(rustacity_biquad_cheby_poly(0, 0.5), 0.0);
        assert_eq!(rustacity_biquad_cheby_poly(99, 0.5), 0.0);

        assert!(rustacity_ebur128_new(f64::NAN, 2).is_null());
        assert!(rustacity_ebur128_new(48000.0, 0).is_null());

        assert!(rustacity_bass_treble_new(0.0).is_null());
        assert!(rustacity_bass_treble_new(f32::NAN).is_null());
        rustacity_bass_treble_free(core::ptr::null_mut());
        assert_eq!(
            rustacity_bass_treble_process(
                core::ptr::null_mut(),
                0.0,
                0.0,
                0.0,
                core::ptr::null(),
                core::ptr::null_mut(),
                8
            ),
            0
        );
        assert!(rustacity_phaser_new(0.0).is_null());
        assert!(rustacity_phaser_new(f32::NAN).is_null());
        rustacity_phaser_free(core::ptr::null_mut());
        assert_eq!(
            rustacity_phaser_process(
                core::ptr::null_mut(),
                2,
                128,
                0.4,
                0.0,
                100,
                0,
                0.0,
                core::ptr::null(),
                core::ptr::null_mut(),
                8
            ),
            0
        );
        assert!(rustacity_wahwah_new(0.0).is_null());
        assert!(rustacity_wahwah_new(f32::NAN).is_null());
        rustacity_wahwah_free(core::ptr::null_mut());
        assert_eq!(
            rustacity_wahwah_process(
                core::ptr::null_mut(),
                1.5,
                0.0,
                70,
                2.5,
                30,
                -6.0,
                core::ptr::null(),
                core::ptr::null_mut(),
                8
            ),
            0
        );
        assert!(rustacity_distortion_new(0.0).is_null());
        assert!(rustacity_distortion_new(f32::NAN).is_null());
        rustacity_distortion_free(core::ptr::null_mut());
        assert_eq!(
            rustacity_distortion_process(
                core::ptr::null_mut(),
                0,
                0,
                -6.0,
                -70.0,
                50.0,
                50.0,
                1,
                core::ptr::null(),
                core::ptr::null_mut(),
                8
            ),
            0
        );
        rustacity_ebur128_initialize(core::ptr::null_mut());
        rustacity_ebur128_process_sample(core::ptr::null_mut(), 0.0, 0);
        rustacity_ebur128_next_sample(core::ptr::null_mut());
        rustacity_ebur128_process_block(core::ptr::null_mut(), core::ptr::null(), 0, 0);
        assert_eq!(
            rustacity_ebur128_integrative_loudness(core::ptr::null_mut()),
            0.0
        );
        rustacity_ebur128_free(core::ptr::null_mut());
        assert_eq!(
            rustacity_ebur128_weighting_filter(48000.0, core::ptr::null_mut(), 0),
            0
        );
    }

    /// The FFT entry points must tolerate the same bad arguments, and lengths
    /// that cannot be transformed: a non-power-of-two used to end the process
    /// on the C++ side, and now does nothing.
    #[test]
    fn fft_entry_points_tolerate_bad_arguments() {
        let mut scratch = [0.0f32; 16];
        let mut table_i = [0i32; 8];
        let mut table_f = [0.0f32; 16];
        let cp = scratch.as_ptr();
        let p = scratch.as_mut_ptr();
        let (ti, tf) = (table_i.as_mut_ptr(), table_f.as_mut_ptr());
        let (cti, ctf) = (table_i.as_ptr(), table_f.as_ptr());

        rustacity_fft_init_tables(16, core::ptr::null_mut(), core::ptr::null_mut());
        rustacity_fft_init_tables(12, ti, tf);
        rustacity_fft_init_tables(2, ti, tf);
        rustacity_fft_forward(core::ptr::null_mut(), 16, cti, ctf);
        rustacity_fft_forward(p, 16, core::ptr::null(), core::ptr::null());
        rustacity_fft_forward(p, 16, cti, core::ptr::null());
        rustacity_fft_forward(p, 12, cti, ctf);
        rustacity_fft_forward(p, 0, cti, ctf);
        rustacity_fft_inverse(p, 12, cti, ctf);
        rustacity_fft_inverse(core::ptr::null_mut(), 16, cti, ctf);
        rustacity_fft_reorder_to_freq(
            core::ptr::null(),
            16,
            cp,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        rustacity_fft_reorder_to_time(core::ptr::null(), 16, cp, p);
        rustacity_fft_reorder_to_time(cti, 12, cp, p);
        rustacity_fft_real(
            16,
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            cti,
            ctf,
        );
        rustacity_fft_real(12, cp, p, p, cti, ctf);
        rustacity_fft_inverse_real(
            16,
            cp,
            core::ptr::null(),
            core::ptr::null_mut(),
            cti,
            ctf,
        );
        rustacity_fft_inverse_real(12, cp, core::ptr::null(), p, cti, ctf);
        rustacity_fft_power_spectrum(16, core::ptr::null(), core::ptr::null_mut(), cti, ctf);
        rustacity_fft_power_spectrum(16, cp, core::ptr::null_mut(), cti, ctf);
        rustacity_fft_complex(16, 0, core::ptr::null(), core::ptr::null(), p, p);
        rustacity_fft_complex(3, 0, cp, core::ptr::null(), p, p);
        rustacity_fft_complex(16, 0, cp, core::ptr::null(), p, core::ptr::null_mut());
        rustacity_fft_window_func(0, 16, 0, core::ptr::null_mut());
        rustacity_fft_window_func(2, 0, 1, p);
        rustacity_fft_derivative_of_window_func(2, 0, 0, p);
        rustacity_fft_derivative_of_window_func(2, 16, 1, core::ptr::null_mut());
        rustacity_fft_deinit();
    }

    /// A transform taken through the C ABI, from the tables to the bins.
    #[test]
    fn fft_entry_points_transform() {
        let fftlen = 16;
        let points = fftlen / 2;
        let mut bit_reversed = [0i32; 8];
        let mut sin_table = [0.0f32; 16];
        rustacity_fft_init_tables(
            fftlen,
            bit_reversed.as_mut_ptr(),
            sin_table.as_mut_ptr(),
        );

        // A constant signal is all in the DC bin, at its own length.
        let mut buffer = [1.0f32; 16];
        rustacity_fft_forward(
            buffer.as_mut_ptr(),
            fftlen,
            bit_reversed.as_ptr(),
            sin_table.as_ptr(),
        );
        let mut real = [0.0f32; 9];
        let mut imag = [0.0f32; 9];
        rustacity_fft_reorder_to_freq(
            bit_reversed.as_ptr(),
            fftlen,
            buffer.as_ptr(),
            real.as_mut_ptr(),
            imag.as_mut_ptr(),
        );
        assert_eq!(real[0], fftlen as f32);
        assert_eq!(real[points], 0.0);
        for i in 1..points {
            assert_eq!((real[i], imag[i]), (0.0, 0.0));
        }
    }

    /// The interpolation entry point must tolerate the same bad arguments as
    /// the rest: a null buffer, an empty run, and a run that is the whole
    /// buffer or does not fit inside it, all do nothing.
    #[test]
    fn interpolate_entry_point_tolerates_bad_arguments() {
        let mut buffer = [1.0f32; 8];
        rustacity_interpolate_audio(core::ptr::null_mut(), 8, 0, 2);
        rustacity_interpolate_audio(buffer.as_mut_ptr(), 0, 0, 2);
        rustacity_interpolate_audio(buffer.as_mut_ptr(), 8, 4, 8);
        rustacity_interpolate_audio(buffer.as_mut_ptr(), 8, 8, 2);
        rustacity_interpolate_audio(buffer.as_mut_ptr(), 8, 6, 4);
        rustacity_interpolate_audio(buffer.as_mut_ptr(), 8, 4, 0);
        assert_eq!(buffer, [1.0f32; 8]);
    }

    /// An interpolation taken through the C ABI: the run is filled, the good
    /// samples around it are left alone, and two calls give the same answer,
    /// which they did not while the noise came from the C library's global
    /// generator.
    #[test]
    fn interpolate_entry_point_fills_the_run() {
        let input: Vec<f32> = (0..64)
            .map(|i| ((i * 37) % 129) as f32 / 64.0 - 1.0)
            .collect();
        let mut first = input.clone();
        let mut second = input.clone();

        rustacity_interpolate_audio(first.as_mut_ptr(), 64, 20, 5);
        rustacity_interpolate_audio(second.as_mut_ptr(), 64, 20, 5);

        assert_eq!(first, second);
        assert_eq!(first[..20], input[..20]);
        assert_eq!(first[25..], input[25..]);
        for i in 20..25 {
            assert_ne!(first[i], input[i], "sample {} should have been filled", i);
        }
    }
}

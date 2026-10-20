// Rustacity: A Digital Audio Editor
//
// Copyright (C) 2026 Sato
//
// SPDX-License-Identifier: GPL-2.0-or-later

//! Golden-value tests for the FFT and the window functions.
//!
//! The expected values were captured from the ORIGINAL C++ implementation
//! (`src/RealFFTf.cpp` and `src/FFT.cpp`, whose arithmetic was moved into
//! `src/FFTMath.cpp` so that it could be compiled on its own) by
//! `rust/dsp/goldens/capture/fft.cpp`, and are checked in as
//! `rust/dsp/goldens/reference.txt`.
//!
//! Every record carries the length it ran at and the stride its values were
//! sampled with, and the last index is always among the captured ones, so the
//! index list is rebuilt here exactly as the dumper built it.

mod common;

use rustacity_dsp::fft;

/// The capture printed nine significant digits of an `f32`, and the port
/// performs the same operations in the same order, so a value that does not
/// involve an elementary function should come back bit for bit.  `cos`, `sin`
/// and `exp` are `f64` in the C++, though, and their last bits differ between
/// the capture's libm and this platform's, which can move an `f32` by an ulp:
/// about 1e-7 of the value.
const REL: f64 = 1e-5;
const ABS: f64 = 1e-7;

/// The indices a record captured: every `stride`-th one, last always included.
fn indices(len: usize, stride: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..len).step_by(stride).collect();
    if len > 0 && *idx.last().unwrap() != len - 1 {
        idx.push(len - 1);
    }
    idx
}

/// Compare `got`, sampled at `idx`, with the `count` fields of `r` that start
/// at `at`.
fn check(r: &common::Record, at: usize, idx: &[usize], got: &[f32], count: usize, what: &str) {
    assert_eq!(idx.len(), count, "{}: captured value count", what);
    for (k, &i) in idx.iter().enumerate() {
        common::close(
            f64::from(got[i]),
            r.f(at + k),
            REL,
            ABS,
            &format!("{}[{}]", what, i),
        );
    }
}

/// `InputSample` of the dumper: integer arithmetic narrowed to `f32`, so that
/// the same samples arise on any platform.  `block` is the dumper's block
/// index, which is what makes consecutive records run on different inputs.
fn input_sample(i: usize, block: usize) -> f32 {
    ((((i * 37 + block * 11) % 129) as i32 - 64) as f32) / 64.0
}

fn block_input(fftlen: usize, block: usize) -> Vec<f32> {
    (0..fftlen).map(|i| input_sample(i, block)).collect()
}

/// The block the dumper ran a size with: the sizes come first, in its table.
fn block_of_size(fftlen: usize) -> usize {
    match fftlen {
        16 => 0,
        64 => 1,
        1024 => 2,
        other => panic!("unexpected size {}", other),
    }
}

/// The block the dumper ran a window with: after the sizes, every window runs
/// at four variants.
fn block_of_window(which: usize, len: usize, extra: bool) -> usize {
    3 + which * 4
        + match (len, extra) {
            (64, false) => 0,
            (64, true) => 1,
            (257, false) => 2,
            (257, true) => 3,
            other => panic!("unexpected window variant {:?}", other),
        }
}

/// The tables of a size, as the capture made them.
fn tables(fftlen: usize) -> (Vec<i32>, Vec<f32>) {
    let mut bit_reversed = vec![0i32; fftlen / 2];
    let mut sin_table = vec![0.0f32; fftlen];
    fft::init_tables(fftlen, &mut bit_reversed, &mut sin_table);
    (bit_reversed, sin_table)
}

#[test]
fn golden_bit_reversed_and_sine_tables() {
    let mut checked = 0;
    for r in common::load() {
        let (points, len) = match r.kind.as_str() {
            "fft-bitreversed" => (r.i(0) as usize / 2, r.i(0) as usize / 2),
            "fft-sintable" => (r.i(0) as usize / 2, r.i(0) as usize),
            _ => continue,
        };
        let stride = r.i(1) as usize;
        let count = r.i(2) as usize;
        assert_eq!(r.fields.len(), 3 + count, "{}: field count", r.kind);
        let (bit_reversed, sin_table) = tables(r.i(0) as usize);
        let idx = indices(len, stride);

        if r.kind == "fft-bitreversed" {
            assert_eq!(points, bit_reversed.len());
            let got: Vec<f32> = bit_reversed.iter().map(|&v| v as f32).collect();
            check(&r, 3, &idx, &got, count, "fft-bitreversed");
        } else {
            check(&r, 3, &idx, &sin_table, count, "fft-sintable");
        }
        checked += 1;
    }
    assert_eq!(checked, 6, "every captured table record is checked");
}

#[test]
fn golden_forward_and_reorders() {
    let mut checked = 0;
    for r in common::load() {
        match r.kind.as_str() {
            "fft-forward" => {
                let fftlen = r.i(0) as usize;
                let stride = r.i(1) as usize;
                let count = r.i(2) as usize;
                let (bit_reversed, sin_table) = tables(fftlen);
                let mut buffer = block_input(fftlen, block_of_size(fftlen));
                fft::forward(&mut buffer, &bit_reversed, &sin_table);
                check(
                    &r,
                    3,
                    &indices(fftlen, stride),
                    &buffer,
                    count,
                    "fft-forward",
                );
            }
            "fft-reorder-freq" => {
                let fftlen = r.i(0) as usize;
                let stride = r.i(1) as usize;
                let count = r.i(2) as usize;
                let points = fftlen / 2;
                let (bit_reversed, sin_table) = tables(fftlen);
                let mut buffer = block_input(fftlen, block_of_size(fftlen));
                fft::forward(&mut buffer, &bit_reversed, &sin_table);
                let mut real = vec![0.0f32; points + 1];
                let mut imag = vec![0.0f32; points + 1];
                fft::reorder_to_freq(&bit_reversed, &buffer, &mut real, &mut imag);
                let idx = indices(points + 1, stride);
                check(&r, 3, &idx, &real, count, "fft-reorder-freq real");
                check(&r, 3 + count, &idx, &imag, count, "fft-reorder-freq imag");
            }
            "fft-reorder-time" => {
                let fftlen = r.i(0) as usize;
                let stride = r.i(1) as usize;
                let count = r.i(2) as usize;
                let (bit_reversed, sin_table) = tables(fftlen);
                let mut buffer = block_input(fftlen, block_of_size(fftlen));
                fft::forward(&mut buffer, &bit_reversed, &sin_table);
                let mut time = vec![0.0f32; fftlen];
                fft::reorder_to_time(&bit_reversed, &buffer, &mut time);
                check(
                    &r,
                    3,
                    &indices(fftlen, stride),
                    &time,
                    count,
                    "fft-reorder-time",
                );
            }
            _ => continue,
        }
        checked += 1;
    }
    assert_eq!(checked, 9, "every captured forward record is checked");
}

#[test]
fn golden_inverse() {
    let mut checked = 0;
    for r in common::load() {
        if r.kind != "fft-inverse" {
            continue;
        }
        let fftlen = r.i(0) as usize;
        let stride = r.i(1) as usize;
        let count = r.i(2) as usize;
        let points = fftlen / 2;
        let (bit_reversed, sin_table) = tables(fftlen);

        // The dumper builds the conjugate-symmetric buffer the same way the
        // real transform's input would: the Fs/2 component in the imaginary
        // half of the DC bin.
        let mut buffer = block_input(fftlen, block_of_size(fftlen));
        fft::forward(&mut buffer, &bit_reversed, &sin_table);
        let mut real = vec![0.0f32; points + 1];
        let mut imag = vec![0.0f32; points + 1];
        fft::reorder_to_freq(&bit_reversed, &buffer, &mut real, &mut imag);

        let mut sym = vec![0.0f32; fftlen];
        for i in 0..points {
            sym[2 * i] = real[i];
            sym[2 * i + 1] = imag[i];
        }
        sym[1] = real[points];
        fft::inverse(&mut sym, &bit_reversed, &sin_table);
        check(
            &r,
            3,
            &indices(fftlen, stride),
            &sym,
            count,
            "fft-inverse",
        );
        checked += 1;
    }
    assert_eq!(checked, 3, "every captured inverse record is checked");
}

#[test]
fn golden_real_transform_wrappers() {
    let mut checked = 0;
    for r in common::load() {
        match r.kind.as_str() {
            "fft-real" => {
                let fftlen = r.i(0) as usize;
                let stride = r.i(1) as usize;
                let count = r.i(2) as usize;
                let (bit_reversed, sin_table) = tables(fftlen);
                let input = block_input(fftlen, block_of_size(fftlen));
                let mut real = vec![0.0f32; fftlen];
                let mut imag = vec![0.0f32; fftlen];
                fft::real_fft(
                    &input,
                    &mut real,
                    &mut imag,
                    &bit_reversed,
                    &sin_table,
                );
                let idx = indices(fftlen, stride);
                check(&r, 3, &idx, &real, count, "fft-real real");
                check(&r, 3 + count, &idx, &imag, count, "fft-real imag");
            }
            "fft-inverse-real" => {
                let fftlen = r.i(0) as usize;
                let with_imag = r.i(1) != 0;
                let stride = r.i(2) as usize;
                let count = r.i(3) as usize;
                let (bit_reversed, sin_table) = tables(fftlen);
                let input = block_input(fftlen, block_of_size(fftlen));
                let mut real = vec![0.0f32; fftlen];
                let mut imag = vec![0.0f32; fftlen];
                fft::real_fft(
                    &input,
                    &mut real,
                    &mut imag,
                    &bit_reversed,
                    &sin_table,
                );
                let mut recon = vec![0.0f32; fftlen];
                let imag_in = if with_imag { Some(&imag[..]) } else { None };
                fft::inverse_real_fft(&real, imag_in, &mut recon, &bit_reversed, &sin_table);
                check(
                    &r,
                    4,
                    &indices(fftlen, stride),
                    &recon,
                    count,
                    "fft-inverse-real",
                );
            }
            _ => continue,
        }
        checked += 1;
    }
    assert_eq!(checked, 9, "every captured wrapper record is checked");
}

#[test]
fn golden_power_spectrum() {
    let mut checked = 0;
    for r in common::load() {
        if r.kind != "fft-power" {
            continue;
        }
        let fftlen = r.i(0) as usize;
        let stride = r.i(1) as usize;
        let count = r.i(2) as usize;
        let (bit_reversed, sin_table) = tables(fftlen);
        let input = block_input(fftlen, block_of_size(fftlen));
        let mut power = vec![0.0f32; fftlen / 2 + 1];
        fft::power_spectrum(&input, &mut power, &bit_reversed, &sin_table);
        check(
            &r,
            3,
            &indices(fftlen / 2 + 1, stride),
            &power,
            count,
            "fft-power",
        );
        checked += 1;
    }
    assert_eq!(checked, 3, "every captured power record is checked");
}

#[test]
fn golden_complex_transform() {
    let mut checked = 0;
    for r in common::load() {
        if r.kind != "fft-complex" {
            continue;
        }
        let fftlen = r.i(0) as usize;
        let inverse = r.i(1) != 0;
        let with_imag = r.i(2) != 0;
        let stride = r.i(3) as usize;
        let count = r.i(4) as usize;
        let block = block_of_size(fftlen);

        // The imaginary input is a second and third block, as the dumper's.
        let real_in = block_input(fftlen, block + 1);
        let imag_in = block_input(fftlen, block + 2);
        let mut real_out = vec![0.0f32; fftlen];
        let mut imag_out = vec![0.0f32; fftlen];
        let imag_arg = if with_imag { Some(&imag_in[..]) } else { None };
        fft::fft(
            &real_in,
            imag_arg,
            &mut real_out,
            &mut imag_out,
            inverse,
        );

        let idx = indices(fftlen, stride);
        check(&r, 5, &idx, &real_out, count, "fft-complex real");
        check(&r, 5 + count, &idx, &imag_out, count, "fft-complex imag");
        checked += 1;
    }
    assert_eq!(checked, 12, "every captured complex record is checked");
}

#[test]
fn golden_window_functions() {
    let mut checked = 0;
    for r in common::load() {
        if r.kind != "fft-window" {
            continue;
        }
        let which = r.i(0) as i32;
        let len = r.i(1) as usize;
        let extra = r.i(2) != 0;
        let stride = r.i(3) as usize;
        let count = r.i(4) as usize;
        let block = block_of_window(which as usize, len, extra);

        let mut data = vec![0.0f32; len + 1];
        for (i, value) in data.iter_mut().take(len).enumerate() {
            *value = input_sample(i, block);
        }
        fft::window_func(which, len, extra, &mut data);
        check(
            &r,
            5,
            &indices(len, stride),
            &data,
            count,
            "fft-window",
        );
        checked += 1;
    }
    assert_eq!(checked, 40, "every captured window record is checked");
}

#[test]
fn golden_window_derivatives() {
    let mut checked = 0;
    for r in common::load() {
        if r.kind != "fft-window-derivative" {
            continue;
        }
        let which = r.i(0) as i32;
        let len = r.i(1) as usize;
        let extra = r.i(2) != 0;
        let stride = r.i(3) as usize;
        let count = r.i(4) as usize;
        let block = block_of_window(which as usize, len, extra);

        let mut data = vec![0.0f32; len + 1];
        for (i, value) in data.iter_mut().take(len).enumerate() {
            *value = input_sample(i, block + 1);
        }
        fft::derivative_of_window_func(which, len, extra, &mut data);
        check(
            &r,
            5,
            &indices(len, stride),
            &data,
            count,
            "fft-window-derivative",
        );
        checked += 1;
    }
    assert_eq!(checked, 40, "every captured derivative record is checked");
}

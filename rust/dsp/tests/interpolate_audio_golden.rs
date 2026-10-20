// Rustacity: A Digital Audio Editor
//
// Copyright (C) 2026 Sato
//
// SPDX-License-Identifier: GPL-2.0-or-later

//! Golden-value tests for the repair interpolation and for the generator its
//! noise comes from.
//!
//! The expected values were captured from the ORIGINAL C++ implementation
//! (`src/InterpolateAudio.cpp` and `src/Matrix.{h,cpp}`, whose arithmetic was
//! moved into `src/InterpolateAudioMath.{h,cpp}` so that it could be compiled
//! on its own) by `rust/dsp/goldens/capture/interpolate.cpp`, and are checked
//! in as `rust/dsp/goldens/reference.txt`.
//!
//! The generator is pinned first.  The port replaces the C library's
//! process-global `rand()`, which the application seeds from the clock, with a
//! generator of its own, so comparing it with what the capture recorded is the
//! only way to know it hands out the same numbers.
//!
//! Every record of a case carries the length it ran at and the stride its
//! values were sampled with, and the last index is always among the captured
//! ones, so the index list is rebuilt here exactly as the dumper built it.

mod common;

use rustacity_dsp::interpolate_audio::{interpolate_audio, GlibcRand};

/// The capture printed nine significant digits of an `f32` and the port does
/// the same operations in the same order, so a value should come back almost
/// bit for bit; what is left is the last bits, because a C++ compiler may
/// contract a multiply and an addition into one instruction and Rust does not.
const REL: f64 = 1e-5;
const ABS: f64 = 1e-7;

/// `InputSample` of the dumper: integer arithmetic narrowed to `f32`, so that
/// the same samples arise on any platform.
fn input_sample(i: usize) -> f32 {
    ((((i * 37) % 129) as i32 - 64) as f32) / 64.0
}

/// The indices a record captured: every `stride`-th one, last always included.
fn indices(len: usize, stride: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..len).step_by(stride).collect();
    if len > 0 && *idx.last().unwrap() != len - 1 {
        idx.push(len - 1);
    }
    idx
}

/// Check every record of `kind`, which is
/// `kind <seed> <len> <first_bad> <num_bad> <stride> <count> <count floats>`,
/// and return how many there were.
fn check_interpolations(kind: &str) -> usize {
    let mut checked = 0;
    for r in common::load().iter().filter(|r| r.kind == kind) {
        let seed = r.i(0);
        let len = r.i(1) as usize;
        let first_bad = r.i(2) as usize;
        let num_bad = r.i(3) as usize;
        let stride = r.i(4) as usize;
        let count = r.i(5) as usize;

        // The dumper seeded the C library's generator; the port always starts
        // its own at the same place, so the two agree on a record captured with
        // the seed the C library itself starts from.
        assert_eq!(
            seed, 1,
            "the port draws the noise from a generator started at 1"
        );

        let before: Vec<f32> = (0..len).map(input_sample).collect();
        let mut got = before;
        interpolate_audio(&mut got, first_bad, num_bad);

        let idx = indices(len, stride);
        assert_eq!(idx.len(), count, "{}: captured value count", kind);
        for (k, &i) in idx.iter().enumerate() {
            common::close(
                f64::from(got[i]),
                r.f(6 + k),
                REL,
                ABS,
                &format!("{} at {} (seed {})", kind, i, seed),
            );
        }
        checked += 1;
    }
    checked
}

/// Check every `interp-rand <seed> <rand_max> <count> <count ints>` record and
/// return how many there were.
fn check_generator() -> usize {
    let mut checked = 0;
    for r in common::load().iter().filter(|r| r.kind == "interp-rand") {
        let seed = r.i(0) as u32;
        let count = r.i(2) as usize;

        // The capture platform's RAND_MAX.  A Windows build of the C++ would
        // have given 32767 here, which is part of what the port does away
        // with.
        assert_eq!(r.i(1), 2147483647, "the capture platform's RAND_MAX");

        let mut rand = GlibcRand::new(seed);
        for k in 0..count {
            assert_eq!(
                i64::from(rand.next()),
                r.i(3 + k),
                "interp-rand {}: value {}",
                seed,
                k
            );
        }
        checked += 1;
    }
    checked
}

#[test]
fn the_generator_hands_out_the_captured_values() {
    assert!(check_generator() > 0, "no interp-rand records to check");
}

#[test]
fn the_interpolation_fills_the_captured_values_of_the_solve() {
    assert!(
        check_interpolations("interp-lsar") > 0,
        "no interp-lsar records to check"
    );
}

#[test]
fn the_interpolation_fills_the_captured_values_of_the_fallback() {
    assert!(
        check_interpolations("interp-linear") > 0,
        "no interp-linear records to check"
    );
}

/// Every kind this unit captured is checked by one of the tests above, so that
/// a record added to the capture cannot be left unread.
#[test]
fn every_captured_kind_is_checked() {
    let records = common::load();
    let mut kinds: Vec<&str> = records
        .iter()
        .map(|r| r.kind.as_str())
        .filter(|kind| kind.starts_with("interp-"))
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds, ["interp-linear", "interp-lsar", "interp-rand"]);
}

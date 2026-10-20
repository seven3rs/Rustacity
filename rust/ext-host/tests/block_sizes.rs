// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Block sizes: the four the API names, and the one just past the ceiling.
//!
//! An editor hands an effect whatever block it happens to have, and the sizes
//! range from a single frame to a whole second's worth. The API says 65536
//! frames is the most an extension will ever see and that anything larger is
//! refused; this checks both ends of that.

mod fixtures;

use fixtures::{assert_close, fixture, planar, process_into, reference_gain, try_process};
use rustacity_ext::{Engine, ExtError, HostConfig};

fn engine() -> Engine {
    Engine::new(HostConfig::default()).expect("an engine")
}

#[test]
fn the_documented_block_sizes_all_work() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    // The largest instance the API allows: 65536 frames, stereo.
    let mut instance = extension
        .instantiate(44_100.0, 65_536, 2)
        .expect("an instance at the ceiling");
    instance.set_param_by_id("gain_db", -3.0).expect("gain");

    for frames in [1usize, 64, 4096, 65_536] {
        let input = planar(2, frames);
        let output = process_into(&mut instance, &input);
        for channel in 0..2 {
            assert_close(
                &output[channel],
                &reference_gain(&input[channel], -3.0, false),
                1e-6,
                &format!("{frames} frames, channel {channel}"),
            );
        }
    }
    assert!(!instance.is_poisoned());
}

#[test]
fn many_blocks_in_a_row_each_get_their_own_deadline() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(48_000.0, 4096, 1)
        .expect("an instance");
    instance.set_param_by_id("gain_db", 3.0).expect("gain");

    // Two thousand calls, none of them over budget: if the deadline were armed
    // once per instance rather than once per call, this would be the test that
    // noticed.
    let input = planar(1, 512);
    let expected = reference_gain(&input[0], 3.0, false);
    for round in 0..2_000 {
        let output = process_into(&mut instance, &input);
        assert_close(&output[0], &expected, 1e-6, &format!("round {round}"));
    }
}

#[test]
fn a_block_of_65537_frames_is_refused() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");

    // The host will not even prepare an instance for it.
    let error = extension
        .instantiate(44_100.0, 65_537, 1)
        .expect_err("one frame past the ceiling");
    assert!(matches!(error, ExtError::InvalidArgument(_)), "{error:?}");
    assert!(error.to_string().contains("65537"), "{error}");
    assert!(error.to_string().contains("65536"), "{error}");

    // And an instance prepared for less refuses more.
    let mut instance = extension
        .instantiate(44_100.0, 1024, 1)
        .expect("an instance");
    let error = try_process(&mut instance, &planar(1, 1025)).expect_err("1025 into 1024");
    assert!(matches!(error, ExtError::InvalidArgument(_)), "{error:?}");

    // Neither is a sandbox failure: the instance carries on.
    assert!(!instance.is_poisoned());
    let input = planar(1, 1024);
    let output = process_into(&mut instance, &input);
    assert_close(&output[0], &input[0], 1e-6, "still working");
}

#[test]
fn an_empty_block_is_not_a_block() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(44_100.0, 1024, 1)
        .expect("an instance");
    let error = try_process(&mut instance, &planar(1, 0)).expect_err("no frames");
    assert!(matches!(error, ExtError::InvalidArgument(_)), "{error:?}");
    assert!(!instance.is_poisoned());
}

#[test]
fn a_channel_count_of_zero_is_refused() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let error = extension
        .instantiate(44_100.0, 1024, 0)
        .expect_err("no channels");
    assert!(matches!(error, ExtError::InvalidArgument(_)), "{error:?}");
}

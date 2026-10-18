// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The first test the extension engine has to pass: a real extension, doing
//! arithmetic the test works out for itself.
//!
//! `gain-invert` is the reference extension: two parameters, a multiply, and
//! nothing else. If the host cannot carry a sample through it exactly, nothing
//! more interesting is worth testing. The comparison is against
//! [`fixtures::reference_gain`], not against the extension's own code, at a
//! tolerance of 1e-6 on every sample of every channel.

mod fixtures;

use fixtures::{assert_close, fixture, planar, process_into, reference_gain, signal, try_process};
use rustacity_ext::{Engine, ExtError, HostConfig};

fn engine() -> Engine {
    Engine::new(HostConfig::default()).expect("an engine")
}

#[test]
fn gain_and_inversion_match_the_reference_at_one_in_a_million() {
    let engine = engine();
    let extension = engine
        .load(&fixture("gain-invert"))
        .expect("gain-invert loads");

    assert_eq!(extension.id(), "com.rustacity.gain-invert");
    assert_eq!(extension.name(), "Gain / Invert");
    assert_eq!(extension.version(), "0.1.0");
    assert_eq!(extension.parameters().len(), 2);
    assert_eq!(extension.parameters()[0].id, "gain_db");
    assert_eq!(extension.parameters()[1].id, "invert");

    let cases: [(f32, bool, &str); 7] = [
        (0.0, false, "unity"),
        (6.0, false, "+6 dB"),
        (-6.0, false, "-6 dB"),
        (0.0, true, "polarity inverted"),
        (6.0, true, "+6 dB and inverted"),
        (24.0, false, "the top of the range"),
        (-60.0, false, "the bottom of the range"),
    ];

    for channels in [1usize, 2, 3] {
        for (gain_db, invert, what) in cases {
            let frames = 512;
            let input = planar(channels, frames);
            let mut instance = extension
                .instantiate(48_000.0, 4096, channels)
                .expect("an instance");

            instance
                .set_param_by_id("gain_db", gain_db)
                .expect("gain is in range");
            instance
                .set_param_by_id("invert", if invert { 1.0 } else { 0.0 })
                .expect("a switch is 0 or 1");

            let output = process_into(&mut instance, &input);
            for channel in 0..channels {
                assert_close(
                    &output[channel],
                    &reference_gain(&input[channel], gain_db, invert),
                    1e-6,
                    &format!("{what}, {channels} channel(s), channel {channel}"),
                );
            }
        }
    }
}

#[test]
fn a_fresh_instance_starts_from_the_manifest_defaults() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(44_100.0, 256, 2)
        .expect("an instance");

    assert_eq!(instance.sample_rate(), 44_100.0);
    assert_eq!(instance.channels(), 2);
    assert_eq!(instance.max_block(), 256);

    let input = planar(2, 256);
    let output = process_into(&mut instance, &input);
    for channel in 0..2 {
        assert_close(
            &output[channel],
            &input[channel],
            1e-6,
            "0 dB and no inversion is the identity",
        );
    }
}

#[test]
fn each_instance_is_its_own_run() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");

    let mut loud = extension
        .instantiate(48_000.0, 128, 1)
        .expect("an instance");
    loud.set_param_by_id("gain_db", 12.0).expect("gain");
    let mut quiet = extension.instantiate(48_000.0, 128, 1).expect("another");
    quiet.set_param_by_id("gain_db", -12.0).expect("gain");

    let input = planar(1, 128);
    let loud_out = process_into(&mut loud, &input);
    let quiet_out = process_into(&mut quiet, &input);
    assert_close(
        &loud_out[0],
        &reference_gain(&input[0], 12.0, false),
        1e-6,
        "the first instance",
    );
    assert_close(
        &quiet_out[0],
        &reference_gain(&input[0], -12.0, false),
        1e-6,
        "the second instance was not the first",
    );
}

#[test]
fn setting_every_parameter_to_its_default_is_the_identity_again() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(48_000.0, 128, 1)
        .expect("an instance");
    instance.set_param_by_id("gain_db", 9.0).expect("gain");
    instance.set_param_by_id("invert", 1.0).expect("invert");
    instance.set_default_parameters().expect("defaults");

    let input = planar(1, 128);
    let output = process_into(&mut instance, &input);
    assert_close(&output[0], &input[0], 1e-6, "back to the defaults");
}

/// A parameter set before the first block takes effect at once; one changed
/// while the instance is running is slept over 128 frames, which is what
/// `docs/extensions/API.md` promises and what `rx_reset` clears.
#[test]
fn a_gain_change_mid_stream_is_slewed_and_a_reset_clears_the_slew() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(48_000.0, 1024, 1)
        .expect("an instance");

    instance.set_param_by_id("gain_db", 0.0).expect("gain");
    let first = planar(1, 64);
    let first_out = process_into(&mut instance, &first);
    assert_close(
        &first_out[0],
        &reference_gain(&first[0], 0.0, false),
        1e-6,
        "before anything has run, a parameter is taken at once",
    );

    // The instance has processed a block, so this one is slewed.
    instance.set_param_by_id("gain_db", 6.0).expect("gain");
    let input = vec![signal(512, 0)];
    let output = process_into(&mut instance, &input);

    let from = 1.0f32;
    let to = 10f32.powf(6.0 / 20.0);
    let first_step = input[0][0] * (from + (to - from) / 128.0);
    assert!(
        (output[0][0] - first_step).abs() <= 1e-6,
        "the first frame jumped to {} instead of stepping to {first_step}",
        output[0][0]
    );
    assert!(
        (output[0][127] - input[0][127] * to).abs() <= 1e-6,
        "the 128th frame had not arrived: {} against {}",
        output[0][127],
        input[0][127] * to
    );
    assert_close(
        &output[0][128..],
        &reference_gain(&input[0][128..], 6.0, false),
        1e-6,
        "after the slew the new gain holds",
    );

    // A reset forgets that processing started, so the next set takes at once.
    instance.reset().expect("reset");
    instance.set_param_by_id("gain_db", -6.0).expect("gain");
    let after = process_into(&mut instance, &planar(1, 128));
    let expected = reference_gain(&planar(1, 128)[0], -6.0, false);
    assert_close(&after[0], &expected, 1e-6, "after a reset, the gain snaps");
}

/// A slew steps once per frame, not once per sample: every channel of a frame
/// is at the same point of the curve. Stepping per sample would leave the
/// second channel a hundred and twenty-eighth of the way behind the first, and
/// the pair would come out at different gains for the length of the slew.
#[test]
fn a_slew_treats_every_channel_of_a_frame_alike() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(48_000.0, 1024, 2)
        .expect("a stereo instance");

    // One block at unity, so that the change below is a change mid-stream.
    instance.set_param_by_id("gain_db", 0.0).expect("gain");
    let _ = process_into(&mut instance, &planar(2, 64));

    instance.set_param_by_id("gain_db", 6.0).expect("gain");
    let input = vec![signal(512, 0), signal(512, 1)];
    let output = process_into(&mut instance, &input);

    for frame in 0..512 {
        let (left_in, right_in) = (input[0][frame], input[1][frame]);
        let (left_out, right_out) = (output[0][frame], output[1][frame]);
        assert!(
            (left_out * right_in - right_out * left_in).abs() <= 1e-6,
            "frame {frame} left the channels at different gains: {left_out} / {left_in} \
             against {right_out} / {right_in}"
        );
    }
}

#[test]
fn a_channel_count_that_does_not_match_is_refused() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension.instantiate(48_000.0, 128, 2).expect("stereo");

    let error = try_process(&mut instance, &planar(1, 128)).expect_err("mono into stereo");
    assert!(matches!(error, ExtError::InvalidArgument(_)), "{error:?}");
    assert!(
        !instance.is_poisoned(),
        "a bad call is not the extension's fault"
    );
}

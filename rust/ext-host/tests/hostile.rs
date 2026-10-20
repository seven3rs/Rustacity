// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The tests that decide whether the sandbox is real.
//!
//! Every module here is the same crate (`rust/extensions/hostile`) built with a
//! different feature, so each behaviour is on its own and each one is tested on
//! its own. The pattern is the same throughout: load it, run it, check that the
//! failure is the one the API promises, and then check that a good extension
//! still loads and still produces the right samples. A sandbox that contains a
//! misbehaving extension is worth nothing if the host has to be restarted
//! afterwards.
//!
//! One of these (`hostile-wasi`) is not expected to fail at all: it runs to
//! completion and reports, in its own output, everything it managed to obtain
//! from outside. The test asserts that this is nothing.

mod fixtures;

use std::time::{Duration, Instant};

use fixtures::{assert_close, fixture, planar, process_into, reference_gain, try_process};
use rustacity_ext::{abi, Engine, ExtError, HostConfig};

fn engine() -> Engine {
    Engine::new(HostConfig::default()).expect("an engine")
}

fn load(engine: &Engine, name: &str) -> rustacity_ext::Extension {
    engine.load(&fixture(name)).unwrap_or_else(|error| {
        panic!("the '{name}' fixture should load: {error}");
    })
}

/// Runs a good extension and checks the samples, so that "the host still works"
/// means something stronger than "it did not crash".
fn a_good_extension_still_works(engine: &Engine) {
    let extension = load(engine, "gain-invert");
    let mut instance = extension
        .instantiate(48_000.0, 256, 2)
        .expect("a fresh instance");
    instance.set_param_by_id("gain_db", 6.0).expect("gain");
    let input = planar(2, 128);
    let output = process_into(&mut instance, &input);
    for channel in 0..2 {
        assert_close(
            &output[channel],
            &reference_gain(&input[channel], 6.0, false),
            1e-6,
            "the host is still usable",
        );
    }
}

#[test]
fn the_well_behaved_build_of_the_hostile_crate_does_nothing_wrong() {
    let engine = engine();
    let extension = load(&engine, "hostile-benign");
    let mut instance = extension.instantiate(48_000.0, 64, 2).expect("an instance");
    let input = planar(2, 64);
    let output = process_into(&mut instance, &input);
    for channel in 0..2 {
        assert_close(
            &output[channel],
            &input[channel],
            1e-6,
            "the control build passes audio through",
        );
    }
    assert!(!instance.is_poisoned());
}

// ---------------------------------------------------------------------------
// 1. A loop that never ends
// ---------------------------------------------------------------------------

#[test]
fn an_infinite_loop_is_stopped_within_the_deadline() {
    let engine = engine();
    let extension = load(&engine, "hostile-loop");
    let mut instance = extension.instantiate(48_000.0, 64, 1).expect("an instance");

    let started = Instant::now();
    let error =
        try_process(&mut instance, &planar(1, 64)).expect_err("the loop has to be cut short");
    let elapsed = started.elapsed();

    assert!(matches!(error, ExtError::Timeout(_)), "{error:?}");
    assert_eq!(error.status(), 7);
    let budget = HostConfig::default().limits.timeout;
    assert!(
        elapsed >= Duration::from_secs(3),
        "it gave up after only {elapsed:?}, but the budget is {budget:?}"
    );
    assert!(
        elapsed <= budget + Duration::from_secs(3),
        "the deadline overran: {elapsed:?} against a budget of {budget:?}"
    );

    // The instance is spent, and says so.
    assert!(instance.is_poisoned());
    assert!(instance.poison_reason().is_some());
    let error = try_process(&mut instance, &planar(1, 64)).expect_err("spent");
    assert!(matches!(error, ExtError::Poisoned(_)), "{error:?}");

    // Nothing else is.
    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// 2. Memory without end
// ---------------------------------------------------------------------------

#[test]
fn unbounded_allocation_is_stopped_by_the_memory_limit() {
    let engine = engine();
    let extension = load(&engine, "hostile-memory");
    let mut instance = extension.instantiate(48_000.0, 64, 1).expect("an instance");

    let error = try_process(&mut instance, &planar(1, 64))
        .expect_err("an extension cannot grow its heap for ever");
    assert!(
        matches!(error, ExtError::OutOfMemory(_) | ExtError::Trap(_)),
        "{error:?}"
    );
    assert!(matches!(error.status(), 6 | 8), "status {}", error.status());
    assert!(instance.is_poisoned());

    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// 3. A trap
// ---------------------------------------------------------------------------

#[test]
fn a_trap_is_contained() {
    let engine = engine();
    let extension = load(&engine, "hostile-trap");
    let mut instance = extension.instantiate(48_000.0, 64, 1).expect("an instance");

    let error = try_process(&mut instance, &planar(1, 64)).expect_err("the divide by zero");
    assert!(matches!(error, ExtError::Trap(_)), "{error:?}");
    assert_eq!(error.status(), 6);
    assert!(instance.is_poisoned());

    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// 4. Reaching for the outside world
// ---------------------------------------------------------------------------

#[test]
fn the_sandbox_gives_the_extension_nothing_to_read() {
    let engine = engine();
    let extension = load(&engine, "hostile-wasi");
    let mut instance = extension.instantiate(48_000.0, 64, 1).expect("an instance");

    let input = planar(1, 64);
    let output = process_into(&mut instance, &input);

    // The extension writes the number of bytes it managed to obtain into the
    // first output sample. Files, environment, arguments, network: nothing.
    assert_eq!(
        output[0][0], 0.0,
        "the extension obtained {} bytes it should not have had",
        output[0][0]
    );
    assert_close(&output[0][1..], &input[0][1..], 1e-6, "pass-through");

    // And it did try, which is what makes that zero mean something.
    let logs = instance.take_logs();
    let text = logs
        .iter()
        .map(|record| record.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("obtained 0 bytes"),
        "the extension should have reported what it got: {text}"
    );
    assert!(
        text.contains("/etc/passwd") || text.contains("read_dir"),
        "the extension should have tried the filesystem: {text}"
    );
    assert!(
        text.contains("connect:"),
        "the extension should have tried the network: {text}"
    );
    assert!(
        logs.iter().all(|record| (0..=4).contains(&record.level)),
        "levels should be within 0..=4: {logs:?}"
    );

    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// 5. Garbage back from the exports
// ---------------------------------------------------------------------------

#[test]
fn a_result_code_the_host_does_not_know_is_reported_not_hidden() {
    let engine = engine();
    let extension = load(&engine, "hostile-code");
    let mut instance = extension.instantiate(48_000.0, 64, 1).expect("an instance");

    let error = try_process(&mut instance, &planar(1, 64)).expect_err("rx_process returns 5");
    assert_eq!(error, ExtError::ModuleReportedError(5));
    assert_eq!(error.status(), 9);
    assert!(error.to_string().contains('5'), "{error}");

    // The extension said no; the sandbox did not have to. That is not a reason
    // to throw the instance away, and the API says so.
    assert!(!instance.is_poisoned());
    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// 6. A flood of log messages
// ---------------------------------------------------------------------------

#[test]
fn a_flood_of_log_messages_stays_within_its_budget() {
    let engine = engine();
    let extension = load(&engine, "hostile-log");
    let mut instance = extension.instantiate(48_000.0, 64, 1).expect("an instance");

    let input = planar(1, 64);
    let output = process_into(&mut instance, &input);
    assert_close(
        &output[0],
        &input[0],
        1e-6,
        "the flood does not stop the audio",
    );

    let logs = instance.take_logs();
    let bytes: usize = logs.iter().map(|record| record.message.len()).sum();
    assert!(
        logs.len() <= abi::MAX_LOG_MESSAGES_PER_CALL,
        "kept {} records, budget {}",
        logs.len(),
        abi::MAX_LOG_MESSAGES_PER_CALL
    );
    assert!(
        bytes <= abi::MAX_LOG_BYTES_PER_CALL,
        "kept {bytes} bytes, budget {}",
        abi::MAX_LOG_BYTES_PER_CALL
    );
    assert!(
        logs.iter()
            .all(|record| record.message.len() <= abi::MAX_LOG_MESSAGE_BYTES),
        "a single message was longer than {} bytes",
        abi::MAX_LOG_MESSAGE_BYTES
    );
    assert!(
        instance.dropped_log_records() > 0,
        "600 messages went through a 64-message budget; something should have been dropped"
    );

    // The run carries on, and the next call gets its own budget.
    a_good_extension_still_works(&engine);
}

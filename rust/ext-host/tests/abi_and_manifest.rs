// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the host refuses, and how it says so.
//!
//! A module built against another version of the API, a manifest that asks for
//! a capability this version does not have, and a parameter that is not in the
//! manifest's range: three mistakes a person writing an extension will actually
//! make, and each has to be answered before any audio moves - and answered in a
//! way that leaves the host able to carry on.

mod fixtures;

use fixtures::{assert_close, fixture, planar, process_into, reference_gain, Scratch};
use rustacity_ext::{Engine, ExtError, HostConfig};

fn engine() -> Engine {
    Engine::new(HostConfig::default()).expect("an engine")
}

/// Nothing above may leave the host unable to load and run a good extension.
fn a_good_extension_still_works(engine: &Engine) {
    let extension = engine.load(&fixture("gain-invert")).expect("still loads");
    let mut instance = extension.instantiate(48_000.0, 128, 1).expect("still runs");
    instance.set_param_by_id("gain_db", 6.0).expect("gain");
    let input = planar(1, 128);
    let output = process_into(&mut instance, &input);
    assert_close(
        &output[0],
        &reference_gain(&input[0], 6.0, false),
        1e-6,
        "the host is still usable",
    );
}

// ---------------------------------------------------------------------------
// The ABI
// ---------------------------------------------------------------------------

#[test]
fn a_module_that_speaks_another_api_version_is_refused_at_load() {
    let engine = engine();
    let error = engine
        .load(&fixture("hostile-abi"))
        .expect_err("a module that answers 2 where the host speaks 1");
    assert!(matches!(error, ExtError::AbiMismatch(_)), "{error:?}");
    assert_eq!(error.status(), 4);
    assert!(error.to_string().contains("version 2"), "{error}");
    assert!(
        error.to_string().contains("this host speaks version 1"),
        "{error}"
    );
    a_good_extension_still_works(&engine);
}

#[test]
fn a_module_that_imports_what_the_host_does_not_provide_is_refused_at_load() {
    let engine = engine();
    let error = engine
        .load(&fixture("hostile-import"))
        .expect_err("a module importing env.rx_evil");
    assert!(matches!(error, ExtError::ModuleInvalid(_)), "{error:?}");
    assert!(error.to_string().contains("env.rx_evil"), "{error}");
    assert!(
        error.to_string().contains("WASI preview 1"),
        "the message should say what is provided: {error}"
    );
    a_good_extension_still_works(&engine);
}

#[test]
fn a_module_that_hands_back_a_pointer_outside_its_memory_is_refused_at_load() {
    let engine = engine();
    let error = engine
        .load(&fixture("hostile-pointers"))
        .expect_err("rx_alloc pointing past the end of the module's memory");
    assert!(matches!(error, ExtError::AbiMismatch(_)), "{error:?}");
    assert!(
        error.to_string().contains("past the end"),
        "the message should say what was wrong: {error}"
    );
    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// The manifest of a built fixture, with its capability list replaced.
fn with_capabilities(list: &str) -> Scratch {
    let scratch = Scratch::from_fixture("gain-invert");
    let manifest = scratch.manifest().replace(
        "capabilities = [\"audio.read\", \"audio.write\"]",
        &format!("capabilities = [{list}]"),
    );
    assert!(
        manifest.contains(list),
        "the fixture's manifest does not look the way this test thinks"
    );
    scratch.with_manifest(&manifest);
    scratch
}

#[test]
fn a_capability_this_version_reserves_is_refused_by_name() {
    let engine = engine();
    for reserved in ["labels.write", "tracks.read"] {
        let scratch =
            with_capabilities(&format!("\"audio.read\", \"audio.write\", \"{reserved}\""));
        let error = engine.load(scratch.path()).expect_err(reserved);
        assert!(matches!(error, ExtError::CapabilityDenied(_)), "{error:?}");
        assert_eq!(error.status(), 5);
        assert!(error.to_string().contains(reserved), "{error}");
        assert!(
            error.to_string().contains("not supported in API version 1"),
            "{error}"
        );
    }
    a_good_extension_still_works(&engine);
}

#[test]
fn a_capability_that_does_not_exist_is_refused_by_name() {
    let engine = engine();
    let scratch = with_capabilities("\"audio.read\", \"audio.write\", \"fs.read\"");
    let error = engine.load(scratch.path()).expect_err("fs.read");
    assert!(matches!(error, ExtError::CapabilityDenied(_)), "{error:?}");
    assert!(
        error
            .to_string()
            .contains("'fs.read' is not a known capability"),
        "{error}"
    );
    a_good_extension_still_works(&engine);
}

#[test]
fn an_extension_with_no_capabilities_loads_but_will_not_be_run() {
    let engine = engine();
    let scratch = with_capabilities("");
    let extension = engine
        .load(scratch.path())
        .expect("a manifest asking for nothing is still a manifest");
    let error = extension
        .instantiate(48_000.0, 128, 1)
        .expect_err("no audio.read and audio.write");
    assert!(matches!(error, ExtError::CapabilityDenied(_)), "{error:?}");
    assert!(
        error.to_string().contains("audio.read") && error.to_string().contains("audio.write"),
        "{error}"
    );
    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// The manifest
// ---------------------------------------------------------------------------

#[test]
fn a_manifest_that_breaks_a_rule_is_refused_with_the_reason() {
    let engine = engine();
    let cases: [(&str, &str, &str); 4] = [
        (
            "api_version = 1",
            "api_version = 7",
            "api_version 7 is not supported",
        ),
        (
            "capabilities = [\"audio.read\", \"audio.write\"]",
            "capabilites = [\"audio.read\"]",
            "capabilites",
        ),
        (
            "version = \"0.1.0\"",
            "version = \"next\"",
            "semantic version",
        ),
        (
            "id = \"com.rustacity.gain-invert\"",
            "id = \"gain\"",
            "reverse-DNS",
        ),
    ];
    for (from, to, expected) in cases {
        let scratch = Scratch::from_fixture("gain-invert");
        let manifest = scratch.manifest().replace(from, to);
        assert_ne!(
            manifest,
            scratch.manifest(),
            "the case '{from}' did not apply"
        );
        scratch.with_manifest(&manifest);
        let error = engine.load(scratch.path()).expect_err(expected);
        assert!(
            matches!(error, ExtError::ManifestInvalid(_)),
            "{expected}: {error:?}"
        );
        assert_eq!(error.status(), 2, "{expected}");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn a_directory_that_is_not_an_extension_says_what_is_missing() {
    let engine = engine();
    let path = std::env::temp_dir().join("rustacity-ext-test-definitely-not-here");
    let error = engine.load(&path).expect_err("nothing there");
    assert!(matches!(error, ExtError::Io(_)), "{error:?}");
    assert!(error.to_string().contains("extension.toml"), "{error}");
    assert!(
        error.to_string().contains("extension.wasm"),
        "the message should say what an extension is: {error}"
    );
    a_good_extension_still_works(&engine);
}

// ---------------------------------------------------------------------------
// Parameter values
// ---------------------------------------------------------------------------

#[test]
fn a_value_outside_the_manifest_is_refused_and_leaves_the_instance_usable() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(48_000.0, 256, 2)
        .expect("an instance");

    // Above the maximum, below the minimum, and not a number at all.
    for bad in [24.5f32, -60.5, 1_000.0, -1_000.0, f32::NAN, f32::INFINITY] {
        let error = instance
            .set_param_by_id("gain_db", bad)
            .expect_err("out of range");
        assert!(
            matches!(error, ExtError::InvalidArgument(_)),
            "{bad}: {error:?}"
        );
        assert_eq!(error.status(), 12, "{bad}");
    }

    // A switch takes 0 or 1 and nothing in between.
    for bad in [0.5f32, 2.0, -1.0, 0.000_001] {
        let error = instance
            .set_param_by_id("invert", bad)
            .expect_err("not a switch value");
        assert!(
            matches!(error, ExtError::InvalidArgument(_)),
            "{bad}: {error:?}"
        );
    }

    // A parameter that does not exist is named as such, by index and by id.
    let error = instance.set_param(7, 0.0).expect_err("no parameter 7");
    assert!(error.to_string().contains("no parameter 7"), "{error}");
    let error = instance
        .set_param_by_id("volume", 0.0)
        .expect_err("no parameter volume");
    assert!(
        error.to_string().contains("no parameter 'volume'"),
        "{error}"
    );

    // None of that spent the instance, and the last good value still holds.
    assert!(!instance.is_poisoned());
    instance
        .set_param_by_id("gain_db", 6.0)
        .expect("a good value");
    let input = planar(2, 256);
    let output = process_into(&mut instance, &input);
    for channel in 0..2 {
        assert_close(
            &output[channel],
            &reference_gain(&input[channel], 6.0, false),
            1e-6,
            "after a run of refused values",
        );
    }
}

#[test]
fn the_ends_of_the_range_are_allowed() {
    let engine = engine();
    let extension = engine.load(&fixture("gain-invert")).expect("loads");
    let mut instance = extension
        .instantiate(48_000.0, 128, 1)
        .expect("an instance");
    instance
        .set_param_by_id("gain_db", -60.0)
        .expect("the minimum");
    instance
        .set_param_by_id("gain_db", 24.0)
        .expect("the maximum");
    instance.set_param_by_id("invert", 0.0).expect("off");
    instance.set_param_by_id("invert", 1.0).expect("on");
}

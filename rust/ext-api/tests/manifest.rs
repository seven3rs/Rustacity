// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! Every rule of API version 1 that the manifest validator enforces, one test
//! per rejection case, with the message a user will actually see.

use std::path::Path;

use rustacity_ext_api::{ExtError, Manifest, ParameterKind};

/// A manifest that passes: one float and one bool parameter.
fn base() -> String {
    r#"
api_version = 1
id = "com.example.gain-invert"
name = "Gain / Invert"
version = "0.1.0"
description = "Trim the gain and optionally invert polarity"
author = "Example"
license = "GPL-2.0-or-later"

capabilities = ["audio.read", "audio.write"]

[[parameters]]
id = "gain_db"
name = "Gain (dB)"
kind = "float"
min = -60.0
max = 24.0
default = 0.0
unit = "dB"

[[parameters]]
id = "invert"
name = "Invert polarity"
kind = "bool"
default = false
"#
    .to_string()
}

fn rejection(text: &str) -> ExtError {
    Manifest::from_str(text).expect_err("should be rejected")
}

#[test]
fn the_base_manifest_is_accepted() {
    let manifest = Manifest::from_str(&base()).expect("valid");
    assert_eq!(manifest.api_version, 1);
    assert_eq!(manifest.id, "com.example.gain-invert");
    assert_eq!(manifest.name, "Gain / Invert");
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.author.as_deref(), Some("Example"));
    assert_eq!(manifest.license.as_deref(), Some("GPL-2.0-or-later"));
    assert_eq!(manifest.capabilities, ["audio.read", "audio.write"]);
    assert!(manifest.can_process_audio());
    assert_eq!(manifest.parameters.len(), 2);
    assert_eq!(manifest.parameters[0].kind, ParameterKind::Float { min: -60.0, max: 24.0 });
    assert_eq!(manifest.parameters[1].kind, ParameterKind::Bool);
    assert_eq!(manifest.parameters[0].unit.as_deref(), Some("dB"));
}

#[test]
fn parameter_order_is_the_order_they_appear_in() {
    let swapped = base()
        .replace("[[parameters]]\nid = \"gain_db\"", "[[parameters]]\nid = \"invert_x\"")
        .replace("[[parameters]]\nid = \"invert\"", "[[parameters]]\nid = \"gain_db\"");
    // Put the bool first, on purpose.
    let text = format!(
        r#"
api_version = 1
id = "com.example.order"
name = "Order"
version = "1.0.0"
capabilities = ["audio.read", "audio.write"]

[[parameters]]
id = "invert"
name = "Invert polarity"
kind = "bool"
default = false

[[parameters]]
id = "gain_db"
name = "Gain (dB)"
kind = "float"
min = -60.0
max = 24.0
default = 0.0
"#
    );
    let manifest = Manifest::from_str(&text).expect("valid");
    assert_eq!(manifest.parameter("invert").expect("invert").index, 0);
    assert_eq!(manifest.parameter("gain_db").expect("gain_db").index, 1);
    // and the order is the vector's, which is what rx_set_param indexes.
    assert_eq!(manifest.parameters[0].id, "invert");
    assert_eq!(manifest.parameters[1].id, "gain_db");
    let _ = swapped;
}

#[test]
fn an_unknown_api_version_is_rejected_by_number() {
    let error = rejection(&base().replace("api_version = 1", "api_version = 2"));
    assert!(matches!(error, ExtError::ManifestInvalid(_)), "{error:?}");
    assert!(
        error.to_string().contains("api_version 2 is not supported"),
        "{error}"
    );
    assert!(error.to_string().contains("speaks version 1"), "{error}");
}

#[test]
fn an_id_must_be_reverse_dns() {
    let error = rejection(&base().replace("com.example.gain-invert", "gain-invert"));
    assert!(error.to_string().contains("reverse-DNS"), "{error}");

    let error = rejection(&base().replace("com.example.gain-invert", "com example.thing"));
    assert!(error.to_string().contains("reverse-DNS"), "{error}");

    let error = rejection(&base().replace("com.example.gain-invert", "com..thing"));
    assert!(error.to_string().contains("empty part"), "{error}");
}

#[test]
fn a_version_must_be_semantic() {
    let error = rejection(&base().replace("version = \"0.1.0\"", "version = \"one\""));
    assert!(error.to_string().contains("semantic version"), "{error}");

    let error = rejection(&base().replace("version = \"0.1.0\"", "version = \"\""));
    assert!(error.to_string().contains("semantic version"), "{error}");

    // A pre-release and a build suffix are fine.
    let manifest = Manifest::from_str(&base().replace(
        "version = \"0.1.0\"",
        "version = \"1.2.3-beta.1+build.5\"",
    ))
    .expect("valid");
    assert_eq!(manifest.version, "1.2.3-beta.1+build.5");
}

#[test]
fn an_unknown_capability_is_denied_by_name() {
    let error = rejection(&base().replace(
        "capabilities = [\"audio.read\", \"audio.write\"]",
        "capabilities = [\"audio.read\", \"audio.write\", \"fs.read\"]",
    ));
    assert!(matches!(error, ExtError::CapabilityDenied(_)), "{error:?}");
    assert!(error.to_string().contains("'fs.read' is not a known capability"), "{error}");
}

#[test]
fn a_reserved_capability_says_it_is_reserved() {
    let error = rejection(&base().replace(
        "capabilities = [\"audio.read\", \"audio.write\"]",
        "capabilities = [\"audio.read\", \"audio.write\", \"labels.write\"]",
    ));
    assert!(matches!(error, ExtError::CapabilityDenied(_)), "{error:?}");
    assert!(error.to_string().contains("reserved"), "{error}");
    assert!(error.to_string().contains("not supported in API version 1"), "{error}");

    let error = rejection(&base().replace(
        "capabilities = [\"audio.read\", \"audio.write\"]",
        "capabilities = [\"audio.read\", \"audio.write\", \"tracks.read\"]",
    ));
    assert!(error.to_string().contains("reserved"), "{error}");
}

#[test]
fn a_repeated_parameter_id_is_rejected() {
    let text = format!(
        "{}\n[[parameters]]\nid = \"gain_db\"\nname = \"Gain again\"\nkind = \"bool\"\ndefault = false\n",
        base()
    );
    let error = rejection(&text);
    assert!(error.to_string().contains("duplicate parameter id 'gain_db'"), "{error}");
}

#[test]
fn an_unknown_parameter_kind_is_rejected() {
    let error = rejection(&base().replace("kind = \"bool\"", "kind = \"enum\""));
    assert!(error.to_string().contains("unknown kind 'enum'"), "{error}");
    assert!(error.to_string().contains("float, bool and choice"), "{error}");
}

#[test]
fn a_float_needs_min_and_max() {
    let error = rejection(&base().replace("min = -60.0\nmax = 24.0\ndefault = 0.0\n", "default = 0.0\n"));
    assert!(error.to_string().contains("needs min and max"), "{error}");

    let error = rejection(&base().replace("min = -60.0", "min = 30.0"));
    assert!(error.to_string().contains("min must be smaller"), "{error}");
}

#[test]
fn a_default_outside_the_range_is_rejected_with_the_numbers() {
    let error = rejection(&base().replace("default = 0.0", "default = 48.0"));
    assert!(error.to_string().contains("default for 'gain_db' (48)"), "{error}");
    assert!(error.to_string().contains("(-60..24)"), "{error}");
}

#[test]
fn a_bool_default_must_be_a_bool() {
    let error = rejection(&base().replace("kind = \"bool\"\ndefault = false", "kind = \"bool\"\ndefault = 1.0"));
    assert!(error.to_string().contains("must be true or false"), "{error}");
}

#[test]
fn a_choice_needs_a_non_empty_list_and_a_real_default() {
    let error = rejection(&base().replace(
        "kind = \"bool\"\ndefault = false",
        "kind = \"choice\"\ndefault = 0",
    ));
    assert!(error.to_string().contains("needs a non-empty choices list"), "{error}");

    let error = rejection(&base().replace(
        "kind = \"bool\"\ndefault = false",
        "kind = \"choice\"\nchoices = [\"Add\", \"Replace\"]\ndefault = 3",
    ));
    assert!(error.to_string().contains("choice 3"), "{error}");

    let error = rejection(&base().replace(
        "kind = \"bool\"\ndefault = false",
        "kind = \"choice\"\nchoices = [\"Add\", \"Replace\"]\ndefault = \"Mix\"",
    ));
    assert!(error.to_string().contains("not one of: Add, Replace"), "{error}");

    let error = rejection(&base().replace(
        "kind = \"bool\"\ndefault = false",
        "kind = \"choice\"\nchoices = [\"Add\", \"Add\"]\ndefault = 0",
    ));
    assert!(error.to_string().contains("lists the choice 'Add' twice"), "{error}");
}

#[test]
fn a_missing_default_is_rejected() {
    let error = rejection(&base().replace("kind = \"bool\"\ndefault = false", "kind = \"bool\""));
    assert!(error.to_string().contains("needs a default"), "{error}");
}

#[test]
fn unknown_fields_are_rejected_so_a_typo_cannot_go_unnoticed() {
    let error = rejection(&base().replace("capabilities = ", "capabilites = "));
    assert!(error.to_string().contains("capabilites"), "{error}");

    let error = rejection(&base().replace("name = \"Invert polarity\"", "lable = \"Invert polarity\""));
    assert!(error.to_string().contains("lable"), "{error}");
}

#[test]
fn missing_required_fields_are_named() {
    let error = rejection(&base().replace("id = \"com.example.gain-invert\"\n", ""));
    assert!(error.to_string().contains("id"), "{error}");

    let error = rejection(&base().replace("api_version = 1\n", ""));
    assert!(error.to_string().contains("api_version"), "{error}");
}

#[test]
fn an_empty_file_is_a_manifest_error_not_a_panic() {
    let error = rejection("");
    assert!(matches!(error, ExtError::ManifestInvalid(_)), "{error:?}");
}

#[test]
fn a_missing_directory_is_reported_as_io_and_names_the_file() {
    let error = Manifest::load(Path::new("definitely/not/here")).expect_err("missing");
    assert!(matches!(error, ExtError::Io(_)), "{error:?}");
    assert!(error.to_string().contains("extension.toml"), "{error}");
}

#[test]
fn a_manifest_that_cannot_process_audio_does_not_claim_it() {
    let text = base().replace("capabilities = [\"audio.read\", \"audio.write\"]", "capabilities = []");
    let manifest = Manifest::from_str(&text).expect("valid");
    assert!(!manifest.can_process_audio());

    let text = base().replace(
        "capabilities = [\"audio.read\", \"audio.write\"]",
        "capabilities = [\"audio.read\"]",
    );
    let manifest = Manifest::from_str(&text).expect("valid");
    assert!(!manifest.can_process_audio());
}

#[test]
fn a_duplicate_capability_is_rejected() {
    let text = base().replace(
        "capabilities = [\"audio.read\", \"audio.write\"]",
        "capabilities = [\"audio.read\", \"audio.read\", \"audio.write\"]",
    );
    let error = rejection(&text);
    assert!(error.to_string().contains("listed twice"), "{error}");
}

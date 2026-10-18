// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The CLI over a real WAV file, checked sample by sample.
//!
//! The host's own tests call the library directly; this calls the binary the
//! way a person would, on a file read from disk and written back to disk, and
//! then compares what came out with what the API says should have. It is the
//! end of the chain the extension engine has to hold: WebAssembly module, host,
//! WAV reader, WAV writer, and a comparison at 1e-6 that survives all four.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

/// The whole test suite works through the built binary, which is what cargo
/// tells us the path of.
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustacity-ext-cli"))
}

/// A test that spawns the CLI takes this first.
///
/// The CLI is a debug build of a WebAssembly engine; starting one is cheap,
/// but starting seven at once is not, and the one that is meant to be stopped
/// by a five-second deadline would then be measured while it competes for the
/// machine. Taking turns is what makes the timing below a statement about the
/// deadline rather than about the test suite's own load.
static THE_CLI: Mutex<()> = Mutex::new(());

fn take_turn() -> MutexGuard<'static, ()> {
    THE_CLI
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn workspace_temp(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&directory).expect("a temporary directory");
    directory
}

/// The extension directory for a variant, built here and kept private.
///
/// The host's tests and `scripts/build-extensions.sh` lay the same thing out
/// under `rust/extensions/dist`; this builds the one variant it needs with the
/// same command and copies it into a directory of its own, so that the two
/// test binaries running side by side cannot read a directory the other is
/// halfway through writing. Which crate and which feature produce a variant
/// comes from `rust/extensions/variants.txt`, the same list they read.
fn fixture(name: &str) -> PathBuf {
    let extensions = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("extensions");
    let variants = std::fs::read_to_string(extensions.join("variants.txt")).expect("variants.txt");
    let line = variants
        .lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && line.split_whitespace().next() == Some(name)
        })
        .unwrap_or_else(|| panic!("variants.txt does not list '{name}'"));
    let fields: Vec<&str> = line.split_whitespace().collect();
    let (directory, feature, module) = (fields[1], fields[2], fields[3]);

    let crate_dir = extensions.join(directory);
    let mut build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()));
    build
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip1",
            "--manifest-path",
        ])
        .arg(crate_dir.join("Cargo.toml"));
    if feature != "-" {
        build.args(["--features", feature]);
    }
    let output = build
        .output()
        .expect("cargo should be runnable to build the extension fixture");
    assert!(
        output.status.success(),
        "building the '{name}' fixture failed (the wasm32-wasip1 target is needed for the \
         toolchain running these tests: rustup target add wasm32-wasip1):\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let built = extensions
        .join("target")
        .join("wasm32-wasip1")
        .join("release")
        .join(module);
    let fixture = workspace_temp(&format!("fixture-{name}"));
    std::fs::copy(
        crate_dir.join("extension.toml"),
        fixture.join("extension.toml"),
    )
    .expect("the fixture manifest");
    std::fs::copy(built, fixture.join("extension.wasm")).expect("the fixture module");
    fixture
}

/// A deterministic signal in -1..1, so a failure reads the same twice.
fn signal(frame: usize, channel: usize) -> f32 {
    let phase = (frame as f32 + channel as f32 * 0.25) * 0.02;
    (phase.sin() * 0.6) + ((phase * 3.0).cos() * 0.3)
}

/// Writes a 16-bit WAV of that signal, returning what the file actually holds.
///
/// Quantising here rather than in the comparison is the point: the test's
/// reference has to start from the same samples the extension was given.
fn write_input(path: &Path, channels: u16, sample_rate: u32, frames: usize) -> Vec<Vec<f32>> {
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("the input file");
    let mut input = vec![vec![0.0f32; frames]; channels as usize];
    for (channel, row) in input.iter_mut().enumerate() {
        for (frame, slot) in row.iter_mut().enumerate() {
            *slot = ((signal(frame, channel) * 32768.0).round() as i16) as f32 / 32768.0;
        }
    }
    for (frame, _) in input[0].iter().enumerate() {
        for row in &input {
            writer
                .write_sample((row[frame] * 32768.0) as i16)
                .expect("an input sample");
        }
    }
    writer.finalize().expect("the input file's header");
    input
}

/// Reads what the CLI wrote, which is always 32-bit float.
fn read_output(path: &Path, channels: usize) -> Vec<Vec<f32>> {
    let mut reader = WavReader::open(path).expect("the output file");
    let spec = reader.spec();
    assert_eq!(
        spec.sample_format,
        SampleFormat::Float,
        "the output is float"
    );
    assert_eq!(spec.bits_per_sample, 32, "the output is 32-bit");
    assert_eq!(spec.channels as usize, channels);
    let mut output = vec![Vec::new(); channels];
    for (index, sample) in reader.samples::<f32>().enumerate() {
        output[index % channels].push(sample.expect("an output sample"));
    }
    output
}

fn run(command: &mut Command) -> Output {
    command.output().expect("the CLI should be runnable")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn describe_says_what_the_extension_is() {
    let _turn = take_turn();
    let extension = fixture("gain-invert");
    let finished = run(cli().args(["--describe"]).arg(&extension));
    assert!(finished.status.success(), "{}", stderr_of(&finished));

    let text = stdout_of(&finished);
    assert!(text.contains("com.rustacity.gain-invert"), "{text}");
    assert!(text.contains("Gain / Invert"), "{text}");
    assert!(text.contains("gain_db"), "{text}");
    assert!(text.contains("invert"), "{text}");
    assert!(text.contains("audio.read, audio.write"), "{text}");
}

#[test]
fn six_db_through_the_cli_is_six_db_to_one_in_a_million() {
    let _turn = take_turn();
    let extension = fixture("gain-invert");
    let directory = workspace_temp("six-db");
    let input_path = directory.join("input.wav");
    let output_path = directory.join("output.wav");

    let input = write_input(&input_path, 2, 44_100, 1_000);
    let finished = run(cli()
        .arg(&extension)
        .arg(&input_path)
        .arg(&output_path)
        .args(["--param", "gain_db=6", "--verbose"]));
    assert!(finished.status.success(), "{}", stderr_of(&finished));

    let output = read_output(&output_path, 2);
    assert_eq!(output[0].len(), 1_000);
    let factor = 10f32.powf(6.0 / 20.0);
    for channel in 0..2 {
        for frame in 0..1_000 {
            let expected = input[channel][frame] * factor;
            let difference = (output[channel][frame] - expected).abs();
            assert!(
                difference <= 1e-6,
                "channel {channel} frame {frame}: {} against {expected} ({difference})",
                output[channel][frame]
            );
        }
    }

    // The summary says what it did, and the verbose lines name the parameter.
    let text = stdout_of(&finished);
    assert!(text.contains("gain_db = 6"), "{text}");
    assert!(text.contains("1000 frame(s)"), "{text}");
    assert!(text.contains("ms inside the extension"), "{text}");
}

#[test]
fn inverting_is_a_polarity_flip() {
    let _turn = take_turn();
    let extension = fixture("gain-invert");
    let directory = workspace_temp("invert");
    let input_path = directory.join("input.wav");
    let output_path = directory.join("output.wav");

    let input = write_input(&input_path, 1, 48_000, 512);
    let finished = run(cli()
        .arg(&extension)
        .arg(&input_path)
        .arg(&output_path)
        .args(["--param", "gain_db=0", "--param", "invert=true"]));
    assert!(finished.status.success(), "{}", stderr_of(&finished));

    let output = read_output(&output_path, 1);
    for frame in 0..512 {
        assert!(
            (output[0][frame] + input[0][frame]).abs() <= 1e-6,
            "frame {frame}: {} is not the negative of {}",
            output[0][frame],
            input[0][frame]
        );
    }
}

#[test]
fn the_block_size_does_not_change_the_result() {
    let _turn = take_turn();
    let extension = fixture("gain-invert");
    let directory = workspace_temp("blocks");
    let input_path = directory.join("input.wav");

    let input = write_input(&input_path, 2, 44_100, 300);
    let mut results: Vec<Vec<Vec<f32>>> = Vec::new();
    for block in ["1", "64", "65536"] {
        let output_path = directory.join(format!("block-{block}.wav"));
        let finished = run(cli()
            .arg(&extension)
            .arg(&input_path)
            .arg(&output_path)
            .args(["--param", "gain_db=-3", "--block", block]));
        assert!(finished.status.success(), "{}", stderr_of(&finished));
        results.push(read_output(&output_path, 2));
    }

    let factor = 10f32.powf(-3.0 / 20.0);
    for (index, block) in ["1", "64", "65536"].iter().enumerate() {
        for channel in 0..2 {
            for frame in 0..300 {
                let expected = input[channel][frame] * factor;
                let difference = (results[index][channel][frame] - expected).abs();
                assert!(
                    difference <= 1e-6,
                    "block {block}, channel {channel} frame {frame}: {} against {expected}",
                    results[index][channel][frame]
                );
            }
        }
    }
}

#[test]
fn a_parameter_that_cannot_be_satisfied_is_refused() {
    let _turn = take_turn();
    let extension = fixture("gain-invert");
    let directory = workspace_temp("bad-parameter");
    let input_path = directory.join("input.wav");
    let output_path = directory.join("output.wav");
    write_input(&input_path, 1, 44_100, 64);

    for (argument, expected) in [
        ("gain_db=1000", "outside the range of 'gain_db'"),
        ("gain_db=loud", "is not a number"),
        ("invert=0.5", "is not true or false"),
        ("volume=1", "is not a parameter of this extension"),
    ] {
        let finished = run(cli()
            .arg(&extension)
            .arg(&input_path)
            .arg(&output_path)
            .args(["--param", argument]));
        assert!(!finished.status.success(), "'{argument}' was accepted");
        assert!(
            stderr_of(&finished).contains(expected),
            "'{argument}': expected '{expected}' in {}",
            stderr_of(&finished)
        );
    }
}

#[test]
fn a_bad_command_line_explains_itself_and_hangs_nothing() {
    let _turn = take_turn();
    let finished = run(cli().args(["--nonsense"]));
    assert_eq!(finished.status.code(), Some(2));
    assert!(stderr_of(&finished).contains("there is no option '--nonsense'"));
    assert!(
        stderr_of(&finished).contains("Usage:"),
        "the usage goes with it"
    );
}

#[test]
fn the_infinite_loop_extension_is_stopped_and_reported() {
    let _turn = take_turn();
    let extension = fixture("hostile-loop");
    let directory = workspace_temp("loop");
    let input_path = directory.join("input.wav");
    let output_path = directory.join("output.wav");
    write_input(&input_path, 1, 44_100, 64);

    let started = Instant::now();
    let finished = run(cli().arg(&extension).arg(&input_path).arg(&output_path));
    let elapsed = started.elapsed();

    assert!(!finished.status.success(), "the loop was not stopped");
    assert!(
        stderr_of(&finished).contains("timed out"),
        "{}",
        stderr_of(&finished)
    );
    // The deadline is five seconds. This is not that measurement - the host's
    // own test does that - it is here to catch a deadline that has stopped
    // being a deadline at all.
    assert!(
        elapsed.as_secs() <= 15,
        "it took {elapsed:?}, against a five-second deadline"
    );
}

// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension fixtures the host's tests run against.
//!
//! `rust/extensions/variants.txt` is the list of modules and the cargo feature
//! that produces each one; `scripts/build-extensions.sh` builds them for people
//! and for CI, and this module builds them for the tests, from the same list,
//! so the two cannot disagree.
//!
//! Nothing here is skipped when the target is missing: a fixture that was not
//! built fails the test that wanted it, with the command that would fix it.

// This module is compiled into every test binary, and they do not all use all
// of it.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use rustacity_ext::{ExtError, Instance};

/// The target every extension is built for.
pub const WASM_TARGET: &str = "wasm32-wasip1";

/// `rust/extensions`, wherever the crate was built.
pub fn extensions_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("extensions")
}

/// Where the built extensions are laid out, one directory each.
pub fn dist_dir() -> PathBuf {
    extensions_dir().join("dist")
}

/// The directory of one built fixture, building it first if need be.
///
/// The names are the first column of `variants.txt`: `gain-invert`,
/// `hostile-loop`, `hostile-wasi`, and so on.
pub fn fixture(name: &str) -> PathBuf {
    ensure_built();
    let directory = dist_dir().join(name);
    let manifest = directory.join("extension.toml");
    let module = directory.join("extension.wasm");
    assert!(
        manifest.is_file() && module.is_file(),
        "the fixture '{name}' was not built; run scripts/build-extensions.sh"
    );
    directory
}

/// A directory that copies itself away when the test ends, for tests that need
/// an extension whose manifest is wrong in a particular way.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Copies a built fixture into a fresh directory.
    pub fn from_fixture(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let source = fixture(name);
        let path = std::env::temp_dir().join(format!(
            "rustacity-ext-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a temporary directory");
        for file in ["extension.toml", "extension.wasm"] {
            std::fs::copy(source.join(file), path.join(file)).expect("copying the fixture");
        }
        Self { path }
    }

    /// Rewrites the manifest.
    pub fn with_manifest(&self, text: &str) -> &Self {
        std::fs::write(self.path.join("extension.toml"), text).expect("writing the manifest");
        self
    }

    /// The manifest as it stands.
    pub fn manifest(&self) -> String {
        std::fs::read_to_string(self.path.join("extension.toml")).expect("reading the manifest")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

struct Variant {
    name: String,
    directory: String,
    feature: Option<String>,
    module: String,
}

fn variants() -> Vec<Variant> {
    let text = std::fs::read_to_string(extensions_dir().join("variants.txt"))
        .expect("rust/extensions/variants.txt");
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let mut fields = line.split_whitespace();
            Some(Variant {
                name: fields.next()?.to_string(),
                directory: fields.next()?.to_string(),
                feature: match fields.next()? {
                    "-" => None,
                    feature => Some(feature.to_string()),
                },
                module: fields.next()?.to_string(),
            })
        })
        .collect()
}

/// Builds the fixtures, once per test binary, when what is on disk is older
/// than what is in the tree.
fn ensure_built() {
    static STATE: Mutex<Option<&'static str>> = Mutex::new(None);
    let mut state = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.is_some() {
        return;
    }
    if stale() {
        build();
    }
    *state = Some("built");
}

/// Whether any source is newer than the oldest built module.
fn stale() -> bool {
    let variants = variants();
    if variants.is_empty() {
        return true;
    }
    let mut oldest: Option<SystemTime> = None;
    for variant in &variants {
        let module = dist_dir().join(&variant.name).join("extension.wasm");
        let Ok(modified) = std::fs::metadata(&module).and_then(|meta| meta.modified()) else {
            return true;
        };
        oldest = Some(match oldest {
            Some(current) => current.min(modified),
            None => modified,
        });
    }
    let Some(oldest) = oldest else {
        return true;
    };

    let mut sources: Vec<PathBuf> = vec![
        extensions_dir().join("variants.txt"),
        extensions_dir().join("Cargo.toml"),
    ];
    for variant in &variants {
        sources.push(extensions_dir().join(&variant.directory));
    }
    let mut newest: Option<SystemTime> = None;
    for source in sources {
        if let Some(modified) = newest_under(&source) {
            newest = Some(match newest {
                Some(current) => current.max(modified),
                None => modified,
            });
        }
    }
    match newest {
        Some(newest) => newest > oldest,
        None => true,
    }
}

/// The newest modification time in a file or, recursively, in a directory.
/// The build directory is not a source.
fn newest_under(path: &Path) -> Option<SystemTime> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.is_file() {
        return metadata.modified().ok();
    }
    let mut newest = metadata.modified().ok();
    for entry in std::fs::read_dir(path).ok()? {
        let entry = entry.ok()?;
        if entry.file_name() == "target" {
            continue;
        }
        if let Some(modified) = newest_under(&entry.path()) {
            newest = Some(match newest {
                Some(current) => current.max(modified),
                None => modified,
            });
        }
    }
    newest
}

fn build() {
    // Under `cargo test` this is the cargo that is running; otherwise whatever
    // is on the path.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    for variant in variants() {
        let crate_dir = extensions_dir().join(&variant.directory);
        let mut command = Command::new(&cargo);
        command
            .arg("build")
            .arg("--release")
            .arg("--target")
            .arg(WASM_TARGET)
            .arg("--manifest-path")
            .arg(crate_dir.join("Cargo.toml"));
        if let Some(feature) = &variant.feature {
            command.arg("--features").arg(feature);
        }
        let output = command.output().unwrap_or_else(|error| {
            panic!("cannot run {cargo} to build the extension fixtures: {error}")
        });
        assert!(
            output.status.success(),
            "building the '{}' fixture failed:\n{}\n{}\n\
             (the {WASM_TARGET} target is needed for the toolchain running these tests: \
             rustup target add {WASM_TARGET})",
            variant.name,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let module = extensions_dir()
            .join("target")
            .join(WASM_TARGET)
            .join("release")
            .join(&variant.module);
        assert!(
            module.is_file(),
            "the build of '{}' did not produce {}",
            variant.name,
            module.display()
        );

        let directory = dist_dir().join(&variant.name);
        std::fs::create_dir_all(&directory).expect("the fixture directory");
        std::fs::copy(
            crate_dir.join("extension.toml"),
            directory.join("extension.toml"),
        )
        .expect("the fixture manifest");
        std::fs::copy(module, directory.join("extension.wasm")).expect("the fixture module");
    }
}

// ---------------------------------------------------------------------------
// Audio, for the tests that check what came out
// ---------------------------------------------------------------------------

/// A deterministic signal: no `rand`, so a failure says the same thing twice.
pub fn signal(frames: usize, channel: usize) -> Vec<f32> {
    (0..frames)
        .map(|index| {
            let phase = (index as f32 + channel as f32 * 0.25) * 0.05;
            (phase.sin() * 0.5) + ((phase * 7.0).cos() * 0.25)
        })
        .collect()
}

pub fn planar(channels: usize, frames: usize) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|channel| signal(frames, channel))
        .collect()
}

/// What the API says a gain and an inversion should do, worked out here in the
/// test rather than borrowed from the extension's own code.
pub fn reference_gain(input: &[f32], gain_db: f32, invert: bool) -> Vec<f32> {
    let mut factor = 10f32.powf(gain_db / 20.0);
    if invert {
        factor = -factor;
    }
    input.iter().map(|sample| sample * factor).collect()
}

/// Runs one block, returning what came out.
pub fn process_into(instance: &mut Instance, input: &[Vec<f32>]) -> Vec<Vec<f32>> {
    try_process(instance, input).expect("the extension should have processed this block")
}

/// Runs one block, returning the error if there was one.
pub fn try_process(instance: &mut Instance, input: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, ExtError> {
    let mut output: Vec<Vec<f32>> = input
        .iter()
        .map(|channel| vec![0.0f32; channel.len()])
        .collect();
    {
        let input_slices: Vec<&[f32]> = input.iter().map(|channel| channel.as_slice()).collect();
        let mut output_slices: Vec<&mut [f32]> = output
            .iter_mut()
            .map(|channel| channel.as_mut_slice())
            .collect();
        instance.process(&input_slices, &mut output_slices)?;
    }
    Ok(output)
}

/// Asserts two blocks are the same to a tolerance, naming the first frame that
/// is not.
#[track_caller]
pub fn assert_close(actual: &[f32], expected: &[f32], tolerance: f32, what: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{what}: {} samples were expected, {} came back",
        expected.len(),
        actual.len()
    );
    for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (got - want).abs() <= tolerance,
            "{what}: sample {index} is {got}, expected {want} (tolerance {tolerance})"
        );
    }
}

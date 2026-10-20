// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Asking the extension host whether an extension really is one.
//!
//! The manager could answer "this directory has a manifest and a module" and
//! stop there, and it would be wrong often enough to be useless: a module built
//! against another API version, a truncated `.wasm`, a `rx_init` that traps, a
//! `rx_process` that never returns - none of those are visible in a file
//! listing, and all of them are exactly what a user wants to find out.
//!
//! So the check is the host's own, in the two stages the host has: the same
//! [`Engine`] the editor links loads the extension the way the editor would
//! (manifest rules, module compiled, start-up run), and then one block of
//! silence is pushed through it.  The second stage is what separates an
//! extension that exists from one that works, and it is the only reason the
//! `hostile-*` fixtures are worth having.
//!
//! It happens only when the user asks, because both stages run the extension's
//! code - under the same limits the editor runs it under, which is also what
//! keeps a `hostile-loop` to a fixed wait rather than a hung window.

use std::path::Path;

use rustacity_ext::{Engine, HostConfig};

use crate::model::{self, ManagerError, ParameterInfo, Slot};

/// The rate the probe runs at.
const PROBE_RATE: f32 = 44_100.0;
/// The block it is handed, in frames.  One block, because that is enough to
/// reach every stage of an extension and short enough to wait for.
const PROBE_BLOCK: usize = 4096;
/// One channel: what the C++ effect asks for, and the smallest case that has
/// to work.
const PROBE_CHANNELS: usize = 1;

/// What the host made of an extension directory.
#[derive(Debug, Clone)]
pub struct Report {
    pub name: String,
    pub slot: Slot,
    pub directory: String,
    /// Whether the host loaded it: manifest accepted, module compiled, and the
    /// module's own start-up survived.
    pub loaded: bool,
    pub load_error: Option<String>,
    /// Whether a block of silence went through it afterwards.
    pub ran: bool,
    pub run_error: Option<String>,
    /// What the host calls it, read back from the loaded module rather than
    /// from the file listing.
    pub id: Option<String>,
    pub version: Option<String>,
    /// The parameters the host will offer, in the order the ABI indexes them.
    pub parameters: Vec<ParameterInfo>,
    /// How it was run, in the words the interface shows - so that a result is
    /// never read as a stronger claim than it is.
    pub probe: String,
}

/// Loads and runs the named extension the way the editor would.
pub fn verify(root: &Path, name: &str, slot: Slot) -> Result<Report, ManagerError> {
    if !model::is_plain_name(name) {
        return Err(ManagerError::BadName(name.to_owned()));
    }
    let directory = model::directory_of(root, name, slot);
    if !directory.is_dir() {
        return Err(ManagerError::NotFound(directory));
    }
    Ok(check(&directory, name, slot))
}

fn check(directory: &Path, name: &str, slot: Slot) -> Report {
    let mut report = Report {
        name: name.to_owned(),
        slot,
        directory: directory.display().to_string(),
        loaded: false,
        load_error: None,
        ran: false,
        run_error: None,
        id: None,
        version: None,
        parameters: Vec::new(),
        probe: format!(
            "{PROBE_BLOCK} frames of silence, {PROBE_CHANNELS} channel(s) at {} Hz",
            PROBE_RATE as i64
        ),
    };

    let engine = match Engine::new(HostConfig::default()) {
        Ok(engine) => engine,
        Err(error) => {
            report.load_error = Some(format!("cannot start the extension host: {error}"));
            return report;
        }
    };

    let extension = match engine.load(directory) {
        Ok(extension) => extension,
        Err(error) => {
            report.load_error = Some(error.to_string());
            return report;
        }
    };
    report.loaded = true;
    report.id = Some(extension.id().to_owned());
    report.version = Some(extension.version().to_owned());
    report.parameters = extension
        .parameters()
        .iter()
        .map(ParameterInfo::from)
        .collect();

    if !extension.manifest().can_process_audio() {
        report.run_error = Some(
            "the manifest does not declare audio.read and audio.write, so there is no audio to run"
                .to_owned(),
        );
        return report;
    }

    let mut instance = match extension.instantiate(PROBE_RATE, PROBE_BLOCK, PROBE_CHANNELS) {
        Ok(instance) => instance,
        Err(error) => {
            report.run_error = Some(error.to_string());
            return report;
        }
    };
    // The manifest's own defaults, through the manifest's own rules: what a
    // user would get if they changed nothing.
    if let Err(error) = instance.set_default_parameters() {
        report.run_error = Some(format!("the manifest's own defaults were refused: {error}"));
        return report;
    }

    let input = vec![0.0f32; PROBE_BLOCK];
    let mut output = vec![0.0f32; PROBE_BLOCK];
    match instance.process(&[input.as_slice()], &mut [output.as_mut_slice()]) {
        Ok(()) => report.ran = true,
        Err(error) => {
            // The instance keeps its own reason for failing, which is often the
            // same sentence: append it only when it says something more.
            let message = error.to_string();
            report.run_error = Some(match instance.poison_reason() {
                Some(reason) if reason != message => format!("{message} ({reason})"),
                _ => message,
            })
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustacity_ext_api::Manifest;

    /// A directory that claims to be an extension but has nothing to load.
    fn half_made(tag: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("rustacity-verify-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let directory = root.join("half-made");
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(
            Manifest::path_in(&directory),
            "api_version = 1\nid = \"com.rustacity.half\"\nname = \"Half\"\nversion = \"0.1.0\"\ncapabilities = [\"audio.read\", \"audio.write\"]\n",
        )
        .expect("manifest");
        root
    }

    #[test]
    fn an_extension_with_no_module_does_not_load() {
        let root = half_made("no-module");
        let report = verify(&root, "half-made", Slot::Enabled).expect("report");
        assert!(!report.loaded);
        assert!(!report.ran);
        let error = report.load_error.expect("an error");
        // The host's message names what it could not read, which is the point
        // of asking it rather than looking at the directory.
        assert!(
            error.to_lowercase().contains("module") || error.contains("extension.wasm"),
            "{error}"
        );
        assert!(
            report.probe.contains("4096 frames of silence"),
            "{}",
            report.probe
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_that_is_a_path_is_refused_here_too() {
        let root = std::env::temp_dir();
        let error = verify(&root, "../etc", Slot::Enabled).expect_err("refused");
        assert!(matches!(error, ManagerError::BadName(_)), "{error}");
    }
}

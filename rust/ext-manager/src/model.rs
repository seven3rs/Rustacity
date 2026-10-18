// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extensions directory, as this tool sees it.
//!
//! An extension is a directory holding `extension.toml` and `extension.wasm`.
//! "Installed" means that directory sits in the extensions root; "turned off"
//! means it sits in a `disabled` subdirectory of that root instead.  The state
//! is therefore the location, and there is no list of disabled extensions to
//! keep in step with the directory: a user can see both states in a file
//! manager, and change either one there, without any tool agreeing to anything.
//!
//! Nothing in this module writes outside the root, and nothing deletes: the
//! only two changes are renames across the root's own boundary, which are also
//! each other's undo.

use std::path::{Path, PathBuf};

use rustacity_ext_api::{Manifest, Parameter, ParameterKind};
use serde::{Deserialize, Serialize};

/// The subdirectory of the extensions root that holds what is turned off.
pub const DISABLED_DIR: &str = "disabled";

/// Which side of the root an extension is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    /// In the root.  The editor loads these.
    Enabled,
    /// In `disabled/`.  The editor does not.
    Disabled,
}

/// Everything the manager knows about one installed extension.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    /// The directory's own name.  This is what every operation addresses,
    /// because it is the only name that is certainly unique per root; the
    /// manifest's `id` is shown beside it.
    pub name: String,
    pub slot: Slot,
    /// The extension directory, absolute.
    pub directory: String,
    pub module_bytes: Option<u64>,
    pub manifest: Option<ManifestInfo>,
    /// Why the manifest could not be used, when it could not be.  An extension
    /// the manager cannot read is still listed: it is installed, and the user
    /// needs to see that something is there.
    pub manifest_error: Option<String>,
}

/// A manifest, flattened into something that can go over the wire.
///
/// The API's own [`Manifest`] is parsed rather than deserialized, deliberately,
/// so it has no `Serialize` of its own and this is the boundary where it gets
/// one.
#[derive(Debug, Clone, Serialize)]
pub struct ManifestInfo {
    pub api_version: i32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub capabilities: Vec<String>,
    /// Whether the manifest claims the audio contract, which API version 1
    /// requires of anything that will be given an instance.
    pub processes_audio: bool,
    pub parameters: Vec<ParameterInfo>,
}

/// One parameter, flattened.
#[derive(Debug, Clone, Serialize)]
pub struct ParameterInfo {
    pub id: String,
    pub name: String,
    /// Position in the manifest, which is the index the ABI takes.
    pub index: usize,
    /// `float`, `bool` or `choice`.
    pub kind: String,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub default: f32,
    pub unit: Option<String>,
    pub description: Option<String>,
    pub choices: Vec<String>,
}

impl From<&Parameter> for ParameterInfo {
    fn from(parameter: &Parameter) -> Self {
        let (minimum, maximum, choices) = match &parameter.kind {
            ParameterKind::Float { min, max } => (Some(*min), Some(*max), Vec::new()),
            ParameterKind::Bool => (None, None, Vec::new()),
            ParameterKind::Choice { choices } => (None, None, choices.clone()),
        };
        ParameterInfo {
            id: parameter.id.clone(),
            name: parameter.name.clone(),
            index: parameter.index,
            kind: parameter.kind.name().to_owned(),
            minimum,
            maximum,
            default: parameter.default,
            unit: parameter.unit.clone(),
            description: parameter.description.clone(),
            choices,
        }
    }
}

impl From<&Manifest> for ManifestInfo {
    fn from(manifest: &Manifest) -> Self {
        ManifestInfo {
            api_version: manifest.api_version,
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            author: manifest.author.clone(),
            license: manifest.license.clone(),
            capabilities: manifest.capabilities.clone(),
            processes_audio: manifest.can_process_audio(),
            parameters: manifest
                .parameters
                .iter()
                .map(ParameterInfo::from)
                .collect(),
        }
    }
}

/// What is in the extensions directory.
#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    pub root: String,
    pub disabled_root: String,
    pub entries: Vec<Entry>,
    /// Directories in the root that are not extensions, and files sitting
    /// loose in it.  They are named rather than hidden: a user who has copied
    /// the wrong thing in should be able to see that the manager saw it.
    pub ignored: Vec<String>,
}

/// The `disabled` subdirectory of a root.
pub fn disabled_dir(root: &Path) -> PathBuf {
    root.join(DISABLED_DIR)
}

/// Where an extension named `name` is, or would be, in `slot`.
pub fn directory_of(root: &Path, name: &str, slot: Slot) -> PathBuf {
    match slot {
        Slot::Enabled => root.join(name),
        Slot::Disabled => disabled_dir(root).join(name),
    }
}

/// Reads the whole extensions directory.
///
/// A root that does not exist yet is not an error: it is an editor nobody has
/// installed an extension for, and it reads as empty.
pub fn scan(root: &Path) -> Inventory {
    let mut inventory = Inventory {
        root: root.display().to_string(),
        disabled_root: disabled_dir(root).display().to_string(),
        entries: Vec::new(),
        ignored: Vec::new(),
    };
    collect(root, Slot::Enabled, &mut inventory);
    collect(&disabled_dir(root), Slot::Disabled, &mut inventory);
    // Sorted by name so that a browser and a terminal listing agree, and so
    // that the order does not depend on what the filesystem hands back.
    inventory
        .entries
        .sort_by_key(|entry| entry.name.to_lowercase());
    inventory.ignored.sort();
    inventory
}

fn collect(dir: &Path, slot: Slot, inventory: &mut Inventory) {
    let Ok(reader) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in reader.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // The subdirectory we keep the turned-off ones in is not one of them.
        if slot == Slot::Enabled && name == DISABLED_DIR {
            continue;
        }
        if !path.is_dir() || !Manifest::path_in(&path).is_file() {
            inventory.ignored.push(name);
            continue;
        }
        inventory.entries.push(read_entry(name, path, slot));
    }
}

fn read_entry(name: String, directory: PathBuf, slot: Slot) -> Entry {
    let module = Manifest::module_path_in(&directory);
    let module_bytes = std::fs::metadata(&module).ok().map(|meta| meta.len());
    let (manifest, manifest_error) = match Manifest::load(&directory) {
        Ok(manifest) => (Some(ManifestInfo::from(&manifest)), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Entry {
        name,
        slot,
        directory: directory.display().to_string(),
        module_bytes,
        manifest,
        manifest_error,
    }
}

/// What a move did.
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub name: String,
    pub slot: Slot,
    pub from: String,
    pub to: String,
}

/// Turns an extension on or off by moving it across the root's boundary.
///
/// This is the only way the manager changes anything, and it is reversible by
/// construction: the two directories are children of the same root, so the
/// rename cannot cross a filesystem, and the opposite call undoes it exactly.
pub fn set_enabled(root: &Path, name: &str, enabled: bool) -> Result<Outcome, ManagerError> {
    if !is_plain_name(name) {
        return Err(ManagerError::BadName(name.to_owned()));
    }
    let (from, to) = if enabled {
        (disabled_dir(root).join(name), root.join(name))
    } else {
        (root.join(name), disabled_dir(root).join(name))
    };
    if !from.is_dir() {
        return Err(ManagerError::NotFound(from));
    }
    // Something already answers to that name on the far side.  Refusing is the
    // only safe answer: the alternative overwrites one of them.
    if to.exists() {
        return Err(ManagerError::Exists(to));
    }
    let parent = to.parent().unwrap_or(root);
    std::fs::create_dir_all(parent).map_err(|source| ManagerError::Io {
        action: "create",
        path: parent.to_path_buf(),
        source,
    })?;
    std::fs::rename(&from, &to).map_err(|source| ManagerError::Io {
        action: "move",
        path: from.clone(),
        source,
    })?;
    Ok(Outcome {
        name: name.to_owned(),
        slot: if enabled {
            Slot::Enabled
        } else {
            Slot::Disabled
        },
        from: from.display().to_string(),
        to: to.display().to_string(),
    })
}

/// Whether `name` is a name inside the root and not a way out of it.
///
/// The names come from a web page, so they are checked here rather than
/// trusted: one component, no separators, no parent, and not the name of the
/// directory the turned-off ones live in.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name != DISABLED_DIR
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// What can go wrong when the manager touches the directory.
#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("'{0}' is not a name in the extensions directory")]
    BadName(String),
    #[error("there is no extension directory at {}", .0.display())]
    NotFound(PathBuf),
    #[error("there is already something at {}", .0.display())]
    Exists(PathBuf),
    #[error("cannot {action} {}: {source}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A directory of its own under the system temporary directory, removed
    /// when the test that asked for it drops it.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "rustacity-manager-{tag}-{}-{unique}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temp root");
            TempRoot(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A manifest every rule of version 1 accepts, so that a test failing means
    /// the thing it names failed and not the manifest.
    fn manifest_text(id: &str) -> String {
        format!(
            "api_version = 1\n\
             id = \"{id}\"\n\
             name = \"Test extension\"\n\
             version = \"0.1.0\"\n\
             description = \"For the manager's tests\"\n\n\
             capabilities = [\"audio.read\", \"audio.write\"]\n\n\
             [[parameters]]\n\
             id = \"gain_db\"\n\
             name = \"Gain\"\n\
             kind = \"float\"\n\
             min = -60.0\n\
             max = 24.0\n\
             default = 0.0\n\
             unit = \"dB\"\n"
        )
    }

    /// A module the manager only ever measures, never reads: its size is what
    /// the listing shows, so a few bytes are as good as a real one here.
    const MODULE: &[u8] = b"\0asm fake";

    /// Installs an extension directory under `parent`.
    fn install(parent: &Path, name: &str, id: &str) {
        let directory = parent.join(name);
        fs::create_dir_all(&directory).expect("extension directory");
        fs::write(Manifest::path_in(&directory), manifest_text(id)).expect("manifest");
        fs::write(Manifest::module_path_in(&directory), MODULE).expect("module");
    }

    #[test]
    fn a_missing_root_reads_as_empty() {
        let temp = TempRoot::new("missing-root");
        let inventory = scan(&temp.path().join("nothing-here"));
        assert!(inventory.entries.is_empty());
        assert!(inventory.ignored.is_empty());
    }

    #[test]
    fn both_slots_are_listed_and_the_rest_is_named() {
        let temp = TempRoot::new("both-slots");
        let root = temp.path();
        install(root, "gain-invert", "com.rustacity.gain-invert");
        install(&disabled_dir(root), "hostile", "com.rustacity.hostile");
        // Not an extension: no manifest.
        fs::create_dir_all(root.join("not-an-extension")).expect("stray directory");
        // Neither is a loose file.
        fs::write(root.join("notes.txt"), b"hello").expect("stray file");

        let inventory = scan(root);
        let names: Vec<&str> = inventory.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["gain-invert", "hostile"]);
        assert_eq!(inventory.entries[0].slot, Slot::Enabled);
        assert_eq!(inventory.entries[1].slot, Slot::Disabled);
        assert_eq!(inventory.ignored, vec!["not-an-extension", "notes.txt"]);
        // The disabled directory itself is never listed as an entry.
        assert!(inventory.ignored.iter().all(|name| name != DISABLED_DIR));
    }

    #[test]
    fn the_manifest_is_summarised() {
        let temp = TempRoot::new("manifest");
        let root = temp.path();
        install(root, "gain-invert", "com.rustacity.gain-invert");

        let inventory = scan(root);
        let entry = &inventory.entries[0];
        assert!(entry.manifest_error.is_none());
        let manifest = entry.manifest.as_ref().expect("manifest");
        assert_eq!(manifest.id, "com.rustacity.gain-invert");
        assert_eq!(manifest.api_version, 1);
        assert!(manifest.processes_audio);
        assert_eq!(manifest.parameters.len(), 1);
        let parameter = &manifest.parameters[0];
        assert_eq!(parameter.id, "gain_db");
        assert_eq!(parameter.kind, "float");
        assert_eq!(parameter.minimum, Some(-60.0));
        assert_eq!(parameter.maximum, Some(24.0));
        assert_eq!(parameter.unit.as_deref(), Some("dB"));
        assert_eq!(parameter.index, 0);
        assert_eq!(entry.module_bytes, Some(MODULE.len() as u64));
    }

    #[test]
    fn an_indistinct_extension_is_still_listed() {
        let temp = TempRoot::new("broken");
        let root = temp.path();
        let directory = root.join("half-installed");
        fs::create_dir_all(&directory).expect("extension directory");
        fs::write(Manifest::path_in(&directory), "api_version = 1\n").expect("bad manifest");

        let inventory = scan(root);
        assert_eq!(inventory.entries.len(), 1);
        let entry = &inventory.entries[0];
        assert!(entry.manifest.is_none());
        assert!(
            entry
                .manifest_error
                .as_deref()
                .is_some_and(|error| error.contains("extension.toml")),
            "{:?}",
            entry.manifest_error
        );
        // No module was written, and its absence is visible as such.
        assert_eq!(entry.module_bytes, None);
    }

    #[test]
    fn turning_one_off_moves_it_and_turning_it_on_moves_it_back() {
        let temp = TempRoot::new("toggle");
        let root = temp.path();
        install(root, "gain-invert", "com.rustacity.gain-invert");

        let outcome = set_enabled(root, "gain-invert", false).expect("disable");
        assert_eq!(outcome.slot, Slot::Disabled);
        assert!(!root.join("gain-invert").exists());
        assert!(disabled_dir(root).join("gain-invert").is_dir());
        assert_eq!(scan(root).entries[0].slot, Slot::Disabled);

        let outcome = set_enabled(root, "gain-invert", true).expect("enable");
        assert_eq!(outcome.slot, Slot::Enabled);
        assert!(root.join("gain-invert").is_dir());
        assert!(!disabled_dir(root).join("gain-invert").exists());
        assert_eq!(scan(root).entries[0].slot, Slot::Enabled);
    }

    #[test]
    fn two_extensions_can_swap_the_same_name_without_agreeing() {
        // The name is taken on the far side, so the move is refused rather than
        // allowed to decide which of the two the user meant.
        let temp = TempRoot::new("clash");
        let root = temp.path();
        install(root, "gain-invert", "com.rustacity.gain-invert");
        install(&disabled_dir(root), "gain-invert", "com.rustacity.other");

        let error = set_enabled(root, "gain-invert", false).expect_err("clash");
        assert!(matches!(error, ManagerError::Exists(_)), "{error}");
        // Nothing moved.
        assert!(root.join("gain-invert").is_dir());
        assert!(disabled_dir(root).join("gain-invert").is_dir());
    }

    #[test]
    fn a_name_that_is_a_path_is_refused() {
        let temp = TempRoot::new("bad-name");
        let root = temp.path();
        install(root, "gain-invert", "com.rustacity.gain-invert");

        for name in ["", ".", "..", "disabled", "a/b", "a\\b", "../outside"] {
            let error = set_enabled(root, name, false).expect_err(name);
            assert!(matches!(error, ManagerError::BadName(_)), "{name}: {error}");
        }
    }

    #[test]
    fn turning_off_something_that_is_not_there_is_an_error() {
        let temp = TempRoot::new("absent");
        let error = set_enabled(temp.path(), "nothing", false).expect_err("absent");
        assert!(matches!(error, ManagerError::NotFound(_)), "{error}");
    }

    #[test]
    fn a_plain_name_is_one_component() {
        assert!(is_plain_name("gain-invert"));
        assert!(is_plain_name("gain_invert.1"));
        assert!(!is_plain_name(""));
        assert!(!is_plain_name("."));
        assert!(!is_plain_name(".."));
        assert!(!is_plain_name(DISABLED_DIR));
        assert!(!is_plain_name("one/two"));
        assert!(!is_plain_name("one\\two"));
    }
}

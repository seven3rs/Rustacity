// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Where extensions live.
//!
//! The default is the directory the editor will read as well: `<user data
//! directory>/Extensions`, beside the `Plug-Ins` directory that
//! `src/FileNames.cpp` already keeps there.  The rules for the parent are
//! copied from that file rather than invented a second time - the
//! application's name is lowered in the path, and on GTK a `-data` suffix is
//! added - so the two programs agree about the location without sharing a
//! setting.
//!
//! Nothing here creates anything.  The manager makes a directory when it has
//! something to put in it.

use std::path::PathBuf;

/// The environment variable that overrides the default location.
pub const ROOT_ENV: &str = "RUSTACITY_EXTENSIONS_DIR";

/// The subdirectory of the user data directory that holds extensions.
pub const EXTENSIONS_DIR: &str = "Extensions";

/// The directory extensions are installed into when nothing says otherwise.
///
/// `RUSTACITY_EXTENSIONS_DIR` wins over the platform default, which is what a
/// test or a copy of the editor moved off the beaten path wants.
pub fn default_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(ROOT_ENV) {
        if !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    user_data_dir().map(|data| data.join(EXTENSIONS_DIR))
}

/// The editor's user data directory, as the editor works it out.
///
/// Windows keeps it in `%APPDATA%`; GTK in a dot-directory in `$HOME` with the
/// `-data` suffix that separates it from the preferences file wx would
/// otherwise write in its place; macOS in `Application Support`.
#[cfg(windows)]
fn user_data_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("rustacity"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn user_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".rustacity-data"))
}

#[cfg(target_os = "macos")]
fn user_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("rustacity"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The override is read before the platform default is worked out, which is
    /// the whole of what this program promises about it.
    #[test]
    fn the_environment_variable_wins() {
        let before = std::env::var_os(ROOT_ENV);
        std::env::set_var(ROOT_ENV, "C:\\somewhere\\else");
        let root = default_root();
        match before {
            Some(value) => std::env::set_var(ROOT_ENV, value),
            None => std::env::remove_var(ROOT_ENV),
        }
        assert_eq!(root, Some(PathBuf::from("C:\\somewhere\\else")));
    }

    /// An empty value is a mistake, not an instruction, so the default stands.
    #[test]
    fn an_empty_override_is_ignored() {
        let before = std::env::var_os(ROOT_ENV);
        std::env::set_var(ROOT_ENV, "");
        let root = default_root();
        match before {
            Some(value) => std::env::set_var(ROOT_ENV, value),
            None => std::env::remove_var(ROOT_ENV),
        }
        if let Some(root) = root {
            assert!(root.ends_with(EXTENSIONS_DIR), "{}", root.display());
        }
    }
}

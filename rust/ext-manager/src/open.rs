// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! Handing something to the desktop: the file manager.
//!
//! Best-effort.  A user without `explorer`, `open` or `xdg-open` gets an error
//! message and a path to paste - not a failure of the manager, which has
//! already done its work by then.

use std::path::Path;

/// Shows a directory in the desktop's file manager.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    platform::reveal(path)
}

#[cfg(windows)]
mod platform {
    use super::*;

    pub fn reveal(path: &Path) -> std::io::Result<()> {
        // explorer exits non-zero even when it has opened a window, so the
        // spawn is all that can be checked and all that is checked.
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub fn reveal(path: &Path) -> std::io::Result<()> {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::*;

    pub fn reveal(path: &Path) -> std::io::Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
}

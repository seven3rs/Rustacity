// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! Handing something to the desktop: a browser, or the file manager.
//!
//! Both are best-effort.  A user without a browser, or without `xdg-open`, gets
//! an error message and a URL to paste - not a failure of the manager, which
//! has already done its work by then.

use std::path::Path;

/// Opens a URL in the desktop's browser.
pub fn open_url(url: &str) -> std::io::Result<()> {
    platform::open_url(url)
}

/// Shows a directory in the desktop's file manager.
pub fn reveal(path: &Path) -> std::io::Result<()> {
    platform::reveal(path)
}

#[cfg(windows)]
mod platform {
    use super::*;

    pub fn open_url(url: &str) -> std::io::Result<()> {
        // `start` is a command of cmd.exe, not a program, and it takes the
        // window title as its first argument - hence the empty one.
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
    }

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

    pub fn open_url(url: &str) -> std::io::Result<()> {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }

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

    pub fn open_url(url: &str) -> std::io::Result<()> {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }

    pub fn reveal(path: &Path) -> std::io::Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
}

// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension manager's window.
//!
//! This is the program to run when you want to see what extensions are
//! installed, turn one on or off, or ask the extension host whether one still
//! loads.  It is one process: the window is egui, drawn with OpenGL, and the
//! directory it manages is read and changed in the same process by
//! `rustacity-ext-manager` - which is also on its own, as a listing for a
//! terminal.
//!
//! Which directory that is comes from `RUSTACITY_EXTENSIONS_DIR` if it is set
//! and from the platform's user data directory otherwise, exactly as the
//! listing program works it out.

// A window, not a console: a release build on Windows must not open one.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod view;

use rustacity_ext_manager::paths;

fn main() -> eframe::Result {
    let Some(root) = paths::default_root() else {
        eprintln!(
            "rustacity-ext-manager-gui: no extensions directory could be worked out; set {}",
            paths::ROOT_ENV
        );
        std::process::exit(1);
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rustacity extension manager")
            .with_inner_size([1040.0, 740.0])
            .with_min_inner_size([620.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Rustacity extension manager",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(root, cc)))),
    )
}

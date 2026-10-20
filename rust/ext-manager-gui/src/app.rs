// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension manager's window: egui, drawn natively, in the same process
//! as the code that reads and changes the extensions directory.
//!
//! There is no server behind this.  The window links `rustacity-ext-manager`
//! and calls [`model::scan`], [`model::set_enabled`] and [`verify::verify`]
//! directly, so what a click does is one function call away from where it is
//! written and tested, and there is no address, port or protocol to get wrong.
//!
//! The state changes themselves live on [`Model`] and take the root as an
//! argument, so they can be driven without a window - which is how they are
//! tested below.  What is left in [`App`] is the frame loop.
//!
//! The one thing that is not a function call is verification, because it runs
//! the extension's own code and a `hostile-loop` module costs the host's whole
//! wall-clock limit before it gives up.  That runs on a thread of its own, and
//! the window keeps drawing while it does.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use rustacity_ext_manager::model::{self, Entry, Inventory, Slot};
use rustacity_ext_manager::open;
use rustacity_ext_manager::verify::{self, Report};

use crate::view;

/// A line to show the user about what just happened.
pub enum Message {
    Did(String),
    Failed(String),
}

/// Everything the window shows, and every change it makes.
pub struct Model {
    pub inventory: Inventory,
    /// The directory name and slot the detail pane is about.  The name is what
    /// the directory work addresses; the slot is there so that turning the
    /// extension off empties the pane rather than showing it in the wrong one.
    pub selected: Option<(String, Slot)>,
    /// What the host said, once it has been asked.
    pub report: Option<Report>,
    pub filter: String,
    pub message: Option<Message>,
    pub busy: bool,
}

impl Model {
    fn new(inventory: Inventory) -> Self {
        Model {
            inventory,
            selected: None,
            report: None,
            filter: String::new(),
            message: None,
            busy: false,
        }
    }

    /// The entry the detail pane is about, if it is still there.
    pub fn selected_entry(&self) -> Option<&Entry> {
        let (name, slot) = self.selected.as_ref()?;
        self.inventory
            .entries
            .iter()
            .find(|entry| &entry.name == name && entry.slot == *slot)
    }

    pub fn counts(&self) -> (usize, usize) {
        let enabled = self
            .inventory
            .entries
            .iter()
            .filter(|entry| entry.slot == Slot::Enabled)
            .count();
        (enabled, self.inventory.entries.len() - enabled)
    }

    /// Reads the extensions directory again.
    ///
    /// The directory is read rather than remembered, so a user who moves
    /// something in a file manager and presses Refresh sees what is really
    /// there - the same reading the editor will do.
    pub fn refresh(&mut self, root: &Path) {
        self.inventory = model::scan(root);
        // If the selected extension has moved, follow it: the detail pane
        // should not empty itself because it was just turned off.
        if let Some((name, slot)) = self.selected.clone() {
            let now = self
                .inventory
                .entries
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.slot);
            if let Some(now) = now {
                if now != slot {
                    self.selected = Some((name, now));
                    self.report = None;
                }
            }
        }
    }

    /// Turns one on or off, by moving it across the root's boundary.
    pub fn toggle(&mut self, root: &Path, name: &str, slot: Slot) {
        let enable = slot == Slot::Disabled;
        match model::set_enabled(root, name, enable) {
            Ok(_) => {
                self.message = Some(Message::Did(if enable {
                    format!("Turned on '{name}'")
                } else {
                    format!("Turned off '{name}'")
                }));
                self.refresh(root);
            }
            Err(error) => self.message = Some(Message::Failed(error.to_string())),
        }
    }

    /// Shows the extensions directory in the desktop's file manager.
    pub fn reveal(&mut self, root: &Path) {
        self.message = Some(match open::reveal(root) {
            Ok(()) => Message::Did("Opened the extensions folder".to_owned()),
            Err(error) => Message::Failed(format!("could not open {}: {error}", root.display())),
        });
    }
}

/// What the window decided to do, worked out after the drawing is done - so
/// that nothing changes while the model is still borrowed to draw it.
pub enum Action {
    Refresh,
    Reveal,
    Select(String, Slot),
    Toggle(String, Slot),
    Verify(String, Slot),
}

pub struct App {
    /// The extensions directory, worked out once when the window starts.
    root: PathBuf,
    model: Model,
    /// The verification running on its own thread, when one is.
    pending: Option<Receiver<Result<Report, String>>>,
}

impl App {
    pub fn new(root: PathBuf, cc: &eframe::CreationContext<'_>) -> Self {
        // Dark, like the rest of the editor's tooling.
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let model = Model::new(model::scan(&root));
        App {
            root,
            model,
            pending: None,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.collect(ctx);
        let mut action = None;
        view::draw(ctx, &mut self.model, &mut action);
        self.perform(action);
    }
}

impl App {
    /// Takes the answer of a verification that has finished, if one has.
    fn collect(&mut self, ctx: &egui::Context) {
        let finished = match self.pending.as_ref() {
            None => return,
            Some(receiver) => match receiver.try_recv() {
                Ok(outcome) => outcome,
                Err(TryRecvError::Empty) => {
                    // Pick the frame up again shortly, so that the spinner
                    // turns while the host is busy with the extension.
                    ctx.request_repaint_after(Duration::from_millis(100));
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    Err("the check stopped without answering".to_owned())
                }
            },
        };
        self.pending = None;
        self.model.busy = false;
        match finished {
            Ok(report) => self.model.report = Some(report),
            Err(message) => self.model.message = Some(Message::Failed(message)),
        }
    }

    fn perform(&mut self, action: Option<Action>) {
        match action {
            // Nothing is said about a plain refresh: the listing was read
            // again because the user asked, and a folder that did not change
            // is not news.
            Some(Action::Refresh) => self.model.refresh(&self.root),
            Some(Action::Reveal) => self.model.reveal(&self.root),
            Some(Action::Select(name, slot)) => {
                self.model.selected = Some((name, slot));
                // A verdict is about one directory at one moment, so it does
                // not follow the selection.
                self.model.report = None;
                self.model.message = None;
            }
            Some(Action::Toggle(name, slot)) => self.model.toggle(&self.root, &name, slot),
            Some(Action::Verify(name, slot)) => self.start_verify(&name, slot),
            None => {}
        }
    }

    /// Asks the host whether this extension loads and runs, on its own thread.
    fn start_verify(&mut self, name: &str, slot: Slot) {
        // One at a time: the button is disabled while one runs, and this makes
        // that true of the state as well as the drawing.
        if self.pending.is_some() {
            return;
        }
        let root = self.root.clone();
        let name = name.to_owned();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = verify::verify(&root, &name, slot).map_err(|error| error.to_string());
            // The window may have closed while this ran, and then there is
            // nobody left to tell; that is not a failure of the check.
            let _ = sender.send(outcome);
        });
        self.pending = Some(receiver);
        self.model.busy = true;
        self.model.report = None;
        self.model.message = None;
    }
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
                "rustacity-manager-gui-{tag}-{}-{unique}",
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

    /// An extension as far as the manager is concerned: a directory with a
    /// manifest it accepts.  No module, because nothing here loads one.
    fn install(root: &Path, name: &str) {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("extension directory");
        fs::write(
            directory.join("extension.toml"),
            format!(
                "api_version = 1\n\
                 id = \"com.rustacity.{name}\"\n\
                 name = \"Test extension\"\n\
                 version = \"0.1.0\"\n\
                 capabilities = [\"audio.read\", \"audio.write\"]\n"
            ),
        )
        .expect("manifest");
    }

    fn said(message: &Option<Message>) -> Option<&str> {
        match message {
            Some(Message::Did(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    fn failed(message: &Option<Message>) -> Option<&str> {
        match message {
            Some(Message::Failed(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    /// The whole of what the Turn off button does, without a window.
    #[test]
    fn turning_one_off_moves_the_folder_and_follows_it() {
        let temp = TempRoot::new("toggle");
        let root = temp.path();
        install(root, "gain-invert");
        let mut model = Model::new(model::scan(root));
        model.selected = Some(("gain-invert".to_owned(), Slot::Enabled));
        assert_eq!(model.counts(), (1, 0));

        model.toggle(root, "gain-invert", Slot::Enabled);

        assert_eq!(said(&model.message), Some("Turned off 'gain-invert'"));
        assert!(root.join(model::DISABLED_DIR).join("gain-invert").is_dir());
        assert!(!root.join("gain-invert").exists());
        // The listing is read again, so the window shows where it went...
        assert_eq!(model.counts(), (0, 1));
        // ...and the detail pane stays on it rather than emptying itself.
        assert_eq!(
            model.selected_entry().expect("still selected").slot,
            Slot::Disabled
        );

        model.toggle(root, "gain-invert", Slot::Disabled);

        assert_eq!(said(&model.message), Some("Turned on 'gain-invert'"));
        assert!(root.join("gain-invert").is_dir());
        assert_eq!(model.counts(), (1, 0));
    }

    /// A refused move is reported in the window rather than panicking in it.
    #[test]
    fn turning_off_something_that_is_not_there_says_why() {
        let temp = TempRoot::new("absent");
        let root = temp.path();
        let mut model = Model::new(model::scan(root));

        model.toggle(root, "nothing", Slot::Enabled);

        let text = failed(&model.message).expect("a message");
        assert!(text.contains("no extension directory"), "{text}");
    }

    /// Refresh is what makes a change made in a file manager visible.
    #[test]
    fn refresh_notices_a_change_made_outside_the_window() {
        let temp = TempRoot::new("refresh");
        let root = temp.path();
        let mut model = Model::new(model::scan(root));
        assert_eq!(model.counts(), (0, 0));

        install(root, "gain-invert");
        model.refresh(root);

        assert_eq!(model.counts(), (1, 0));
    }
}

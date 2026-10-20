// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension manager as a program without a window.
//!
//! Run it with no arguments and it says where the extensions are and what is
//! installed there, as text: the same listing the window shows, readable in a
//! terminal and easy to paste into a bug report.  Turning an extension on or
//! off, and asking the host whether one still loads, is the window's work -
//! `rustacity-ext-manager-gui`, which links this crate rather than copying it.

use std::path::PathBuf;
use std::process::ExitCode;

use rustacity_ext_manager::model::{self, Entry, Inventory, Slot};
use rustacity_ext_manager::paths;

/// What the manager is and what it takes.
const USAGE: &str = "\
Usage:
rustacity-ext-manager [options]

Lists the extensions installed for Rustacity, and where they are.  It changes
nothing: turning an extension on or off, and asking the extension host whether
one still loads, is done in the window (rustacity-ext-manager-gui).

Extensions live in <user data directory>/Extensions, beside the editor's
Plug-Ins directory: %APPDATA%\\rustacity on Windows, ~/.rustacity-data on GTK,
~/Library/Application Support/rustacity on macOS.

Options:
--root <dir>       the extensions directory to read; RUSTACITY_EXTENSIONS_DIR
                   sets the same thing, and the platform default is used if
                   neither does
--print-root       print the extensions directory and stop
--help, -h         this text
--version          this tool's version and the extension API it speaks
";

/// How the manager failed: badly enough to explain the arguments, or just
/// badly.
#[derive(Debug)]
enum Fault {
    Usage(String),
    Failed(String),
}

/// What the arguments asked for.
enum Request {
    List(PathBuf),
    PrintRoot(PathBuf),
    Help,
    Version,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fault::Usage(message)) => {
            eprintln!("rustacity-ext-manager: {message}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Fault::Failed(message)) => {
            eprintln!("rustacity-ext-manager: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: &[String]) -> Result<(), Fault> {
    match parse(arguments)? {
        Request::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Request::Version => {
            println!(
                "rustacity-ext-manager {} (extension API {})",
                env!("CARGO_PKG_VERSION"),
                rustacity_ext_api::abi::API_VERSION
            );
            Ok(())
        }
        Request::PrintRoot(root) => {
            println!("{}", root.display());
            Ok(())
        }
        Request::List(root) => {
            print!("{}", listing(&model::scan(&root)));
            Ok(())
        }
    }
}

fn parse(arguments: &[String]) -> Result<Request, Fault> {
    let mut root: Option<PathBuf> = None;
    let mut print_root = false;

    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        index += 1;
        match argument {
            "--help" | "-h" => return Ok(Request::Help),
            "--version" => return Ok(Request::Version),
            "--print-root" => print_root = true,
            "--root" => {
                let value = arguments
                    .get(index)
                    .ok_or_else(|| Fault::Usage("--root needs a value after it".to_owned()))?;
                index += 1;
                root = Some(PathBuf::from(value));
            }
            other => return Err(Fault::Usage(format!("'{other}' is not an option"))),
        }
    }

    let root = match root {
        Some(root) => root,
        None => paths::default_root().ok_or_else(|| {
            Fault::Failed(format!(
                "no extensions directory could be worked out; pass --root, or set {}",
                paths::ROOT_ENV
            ))
        })?,
    };
    Ok(if print_root {
        Request::PrintRoot(root)
    } else {
        Request::List(root)
    })
}

/// The listing, as text.
///
/// Pure, so that what it looks like can be checked without a directory to
/// read: it takes the same [`Inventory`] the window is drawn from.
fn listing(inventory: &Inventory) -> String {
    let mut text = format!("{}\n", inventory.root);
    if inventory.entries.is_empty() && inventory.ignored.is_empty() {
        text.push_str("\nNothing is installed here.\n");
        return text;
    }
    for (slot, heading) in [(Slot::Enabled, "Enabled"), (Slot::Disabled, "Turned off")] {
        let entries: Vec<&Entry> = inventory
            .entries
            .iter()
            .filter(|entry| entry.slot == slot)
            .collect();
        if entries.is_empty() {
            continue;
        }
        text.push_str(&format!("\n{heading} ({})\n", entries.len()));
        for entry in entries {
            text.push_str(&format!("  {}\n", entry.label()));
        }
    }
    if !inventory.ignored.is_empty() {
        text.push_str(&format!(
            "\nNot extensions ({}) - directories holding no manifest, and loose files\n",
            inventory.ignored.len()
        ));
        for name in &inventory.ignored {
            text.push_str(&format!("  {name}\n"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustacity_ext_manager::model::{ManifestInfo, ParameterInfo};

    fn arguments(list: &[&str]) -> Vec<String> {
        list.iter().map(|argument| argument.to_string()).collect()
    }

    /// An entry the listing can be built from without touching a directory.
    fn entry(name: &str, slot: Slot, manifest: Option<ManifestInfo>) -> Entry {
        Entry {
            name: name.to_owned(),
            slot,
            directory: format!("/somewhere/{name}"),
            module_bytes: Some(1024),
            manifest,
            manifest_error: None,
        }
    }

    fn manifest(name: &str) -> ManifestInfo {
        ManifestInfo {
            api_version: 1,
            id: "com.rustacity.test".to_owned(),
            name: name.to_owned(),
            version: "0.1.0".to_owned(),
            description: None,
            author: None,
            license: None,
            capabilities: Vec::new(),
            processes_audio: true,
            parameters: Vec::<ParameterInfo>::new(),
        }
    }

    #[test]
    fn a_root_with_nothing_in_it_says_so() {
        let inventory = Inventory {
            root: "/somewhere".to_owned(),
            disabled_root: "/somewhere/disabled".to_owned(),
            entries: Vec::new(),
            ignored: Vec::new(),
        };
        assert_eq!(
            listing(&inventory),
            "/somewhere\n\nNothing is installed here.\n"
        );
    }

    #[test]
    fn the_listing_is_grouped_by_slot_and_names_the_rest() {
        let inventory = Inventory {
            root: "/somewhere".to_owned(),
            disabled_root: "/somewhere/disabled".to_owned(),
            entries: vec![
                entry("gain-invert", Slot::Enabled, Some(manifest("Gain"))),
                entry("hostile-abi", Slot::Disabled, None),
            ],
            ignored: vec!["notes.txt".to_owned()],
        };
        let text = listing(&inventory);
        // Both slots are named, and the count is of what is in that slot.
        assert!(text.contains("Enabled (1)"), "{text}");
        assert!(text.contains("Turned off (1)"), "{text}");
        // The name the manifest gives is shown with the directory beside it.
        assert!(text.contains("  Gain   (gain-invert)   v0.1.0\n"), "{text}");
        // One that cannot be read is still listed, and says so.
        assert!(
            text.contains("  hostile-abi   (manifest cannot be read)\n"),
            "{text}"
        );
        assert!(text.contains("Not extensions (1)"), "{text}");
        assert!(text.contains("  notes.txt\n"), "{text}");
    }

    #[test]
    fn an_empty_slot_is_not_headed() {
        let inventory = Inventory {
            root: "/somewhere".to_owned(),
            disabled_root: "/somewhere/disabled".to_owned(),
            entries: vec![entry("gain-invert", Slot::Enabled, Some(manifest("Gain")))],
            ignored: Vec::new(),
        };
        let text = listing(&inventory);
        assert!(text.contains("Enabled (1)"), "{text}");
        assert!(!text.contains("Turned off"), "{text}");
        assert!(!text.contains("Not extensions"), "{text}");
    }

    #[test]
    fn no_arguments_at_all_asks_for_the_listing() {
        let request = parse(&arguments(&["--root", "/somewhere"])).expect("parsed");
        let Request::List(root) = request else {
            panic!("not a listing");
        };
        assert_eq!(root, PathBuf::from("/somewhere"));
    }

    #[test]
    fn the_root_can_be_printed_without_reading_anything() {
        let request = parse(&arguments(&["--root", "/somewhere", "--print-root"])).expect("parsed");
        let Request::PrintRoot(root) = request else {
            panic!("not a print");
        };
        assert_eq!(root, PathBuf::from("/somewhere"));
    }

    #[test]
    fn a_missing_root_value_is_a_usage_error() {
        assert!(matches!(
            parse(&arguments(&["--root"])),
            Err(Fault::Usage(_))
        ));
    }

    #[test]
    fn an_unknown_option_is_a_usage_error() {
        assert!(matches!(
            parse(&arguments(&["--verbose"])),
            Err(Fault::Usage(_))
        ));
    }
}

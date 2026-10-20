// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! Drawing the extension manager's window.
//!
//! Everything here reads the [`Model`] and writes at most one [`Action`], and
//! changes nothing itself.  That is the whole reason for the split: what the
//! window says about an extension and what it does to one are two different
//! questions, and only the second one can lose somebody's work.

use rustacity_ext_manager::model::{Entry, ParameterInfo, Slot};

use crate::app::{Action, Message, Model};

/// Draws the window: a heading, the list, and the detail pane.
///
/// The model is borrowed mutably for one thing only - the filter box, which is
/// a text field being edited - and immutably for everything else.
pub fn draw(ctx: &egui::Context, model: &mut Model, action: &mut Option<Action>) {
    head(ctx, model, action);
    list(ctx, model, action);
    egui::CentralPanel::default().show(ctx, |ui| details(ui, model, action));
}

fn head(ctx: &egui::Context, model: &Model, action: &mut Option<Action>) {
    egui::TopBottomPanel::top("head").show(ctx, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Rustacity extension manager");
            ui.add_space(12.0);
            if ui
                .add_enabled(!model.busy, egui::Button::new("Refresh"))
                .clicked()
            {
                *action = Some(Action::Refresh);
            }
            if ui
                .add_enabled(!model.busy, egui::Button::new("Open extensions folder"))
                .clicked()
            {
                *action = Some(Action::Reveal);
            }
            if model.busy {
                ui.spinner();
            }
        });
        let (enabled, turned_off) = model.counts();
        ui.label(
            egui::RichText::new(format!(
                "{}     {enabled} enabled, {turned_off} turned off",
                model.inventory.root
            ))
            .small()
            .weak(),
        );
        match &model.message {
            Some(Message::Did(text)) => {
                ui.colored_label(egui::Color32::from_rgb(120, 190, 120), text)
            }
            Some(Message::Failed(text)) => {
                ui.colored_label(egui::Color32::from_rgb(215, 110, 110), text)
            }
            None => ui.label(""),
        };
        ui.add_space(4.0);
    });
}

fn list(ctx: &egui::Context, model: &mut Model, action: &mut Option<Action>) {
    egui::SidePanel::left("extensions")
        .resizable(true)
        .default_width(310.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Filter");
                ui.text_edit_singleline(&mut model.filter);
            });
            let filter = model.filter.to_lowercase();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut shown = 0;
                    for slot in [Slot::Enabled, Slot::Disabled] {
                        let entries: Vec<&Entry> = model
                            .inventory
                            .entries
                            .iter()
                            .filter(|entry| entry.slot == slot)
                            .filter(|entry| matches(&filter, entry))
                            .collect();
                        if entries.is_empty() {
                            continue;
                        }
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(match slot {
                                Slot::Enabled => "Enabled",
                                Slot::Disabled => "Turned off",
                            })
                            .strong()
                            .small(),
                        );
                        for entry in entries {
                            shown += 1;
                            let selected =
                                model
                                    .selected
                                    .as_ref()
                                    .is_some_and(|(name, selected_slot)| {
                                        name == &entry.name && *selected_slot == slot
                                    });
                            if ui.selectable_label(selected, entry.label()).clicked() {
                                *action = Some(Action::Select(entry.name.clone(), slot));
                            }
                        }
                    }
                    if shown == 0 {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(if model.inventory.entries.is_empty() {
                                "Nothing is installed here yet."
                            } else {
                                "Nothing matches the filter."
                            })
                            .weak(),
                        );
                    }
                    if !model.inventory.ignored.is_empty() {
                        ui.add_space(10.0);
                        egui::CollapsingHeader::new(
                            egui::RichText::new(format!(
                                "Not extensions ({})",
                                model.inventory.ignored.len()
                            ))
                            .small(),
                        )
                        .show(ui, |ui| {
                            for name in &model.inventory.ignored {
                                ui.label(egui::RichText::new(name).small().weak());
                            }
                        });
                    }
                });
        });
}

fn details(ui: &mut egui::Ui, model: &mut Model, action: &mut Option<Action>) {
    let Some(entry) = model.selected_entry() else {
        ui.add_space(12.0);
        ui.label("Choose an extension on the left to see what it declares.");
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(
                "An extension is a directory holding extension.toml and extension.wasm. \
                 Putting one in the folder above installs it; turning it off moves it into \
                 a disabled/ subdirectory, which is the whole of what \"off\" means.",
            )
            .weak(),
        );
        return;
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let title = entry
                    .manifest
                    .as_ref()
                    .map(|manifest| manifest.name.clone())
                    .unwrap_or_else(|| entry.name.clone());
                ui.heading(title);
                ui.add_space(10.0);
                if ui
                    .add_enabled(
                        !model.busy,
                        egui::Button::new(match entry.slot {
                            Slot::Enabled => "Turn off",
                            Slot::Disabled => "Turn on",
                        }),
                    )
                    .clicked()
                {
                    *action = Some(Action::Toggle(entry.name.clone(), entry.slot));
                }
                if ui
                    .add_enabled(!model.busy, egui::Button::new("Verify"))
                    .clicked()
                {
                    *action = Some(Action::Verify(entry.name.clone(), entry.slot));
                }
                if model.busy {
                    ui.spinner();
                }
            });

            if let Some(error) = &entry.manifest_error {
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(215, 110, 110), error);
            }
            if let Some(manifest) = &entry.manifest {
                if let Some(description) = &manifest.description {
                    ui.add_space(2.0);
                    ui.label(description);
                }
            }

            ui.add_space(8.0);
            egui::Grid::new("facts")
                .num_columns(2)
                .spacing([18.0, 4.0])
                .show(ui, |ui| {
                    fact(ui, "Directory name", &entry.name);
                    if let Some(manifest) = &entry.manifest {
                        fact(ui, "Identifier", &manifest.id);
                        fact(ui, "Extension version", &manifest.version);
                        if let Some(author) = &manifest.author {
                            fact(ui, "Author", author);
                        }
                        if let Some(license) = &manifest.license {
                            fact(ui, "Licence", license);
                        }
                        fact(
                            ui,
                            "Capabilities",
                            &if manifest.capabilities.is_empty() {
                                "none declared".to_owned()
                            } else {
                                manifest.capabilities.join(", ")
                            },
                        );
                        fact(
                            ui,
                            "Processes audio",
                            if manifest.processes_audio {
                                "yes"
                            } else {
                                "no"
                            },
                        );
                    }
                    fact(
                        ui,
                        "Module",
                        &match entry.module_bytes {
                            Some(bytes) => format!("{bytes} bytes"),
                            None => "missing".to_owned(),
                        },
                    );
                    fact(ui, "Folder", &entry.directory);
                });

            if let Some(manifest) = &entry.manifest {
                ui.add_space(12.0);
                ui.label(egui::RichText::new("Parameters").strong());
                ui.add_space(2.0);
                if manifest.parameters.is_empty() {
                    ui.label(
                        egui::RichText::new("None: this extension has nothing to set.").weak(),
                    );
                } else {
                    egui::Grid::new("parameters")
                        .num_columns(4)
                        .striped(true)
                        .spacing([16.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Name").strong().small());
                            ui.label(egui::RichText::new("Kind").strong().small());
                            ui.label(egui::RichText::new("Range").strong().small());
                            ui.label(egui::RichText::new("Default").strong().small());
                            ui.end_row();
                            for parameter in &manifest.parameters {
                                ui.horizontal(|ui| {
                                    ui.label(&parameter.name);
                                    if let Some(unit) = &parameter.unit {
                                        ui.label(egui::RichText::new(unit).small().weak());
                                    }
                                });
                                ui.label(&parameter.kind);
                                ui.label(range(parameter));
                                ui.label(default_of(parameter));
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "What the extension declares, and what the extension host will offer \
                             it. The editor does not have a dialog for these yet.",
                        )
                        .small()
                        .weak(),
                    );
                }
            }

            ui.add_space(12.0);
            ui.separator();
            ui.label(egui::RichText::new("Does it load and run?").strong());
            ui.add_space(2.0);
            match &model.report {
                Some(report) => {
                    verdict(ui, "Loads", report.loaded, report.load_error.as_deref());
                    if report.loaded {
                        verdict(ui, "Runs", report.ran, report.run_error.as_deref());
                    }
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(&report.probe).small().weak());
                }
                None if model.busy => {
                    ui.label(
                        egui::RichText::new(
                            "Checking: the extension is loaded in the host's own sandbox and one \
                             block of audio is run through it.",
                        )
                        .weak(),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new(
                            "Not asked yet. Verifying loads the extension in the host's own \
                             sandbox and then runs one block of audio through it, which is the \
                             only way to find a module that loads and then fails.",
                        )
                        .weak(),
                    );
                }
            }
        });
}

fn fact(ui: &mut egui::Ui, name: &str, value: &str) {
    ui.label(egui::RichText::new(name).small().weak());
    ui.label(egui::RichText::new(value).monospace());
    ui.end_row();
}

fn verdict(ui: &mut egui::Ui, name: &str, ok: bool, error: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{name}:"));
        if ok {
            ui.colored_label(egui::Color32::from_rgb(120, 190, 120), "yes");
        } else {
            ui.colored_label(
                egui::Color32::from_rgb(215, 110, 110),
                error.unwrap_or("no"),
            );
        }
    });
}

/// Whether an entry matches what the user typed in the filter box.
fn matches(filter: &str, entry: &Entry) -> bool {
    if filter.is_empty() {
        return true;
    }
    entry.name.to_lowercase().contains(filter)
        || entry.manifest.as_ref().is_some_and(|manifest| {
            manifest.name.to_lowercase().contains(filter)
                || manifest.id.to_lowercase().contains(filter)
                || manifest
                    .description
                    .as_ref()
                    .is_some_and(|text| text.to_lowercase().contains(filter))
        })
}

fn range(parameter: &ParameterInfo) -> String {
    match (parameter.minimum, parameter.maximum) {
        (Some(min), Some(max)) => format!("{min} .. {max}"),
        _ => match parameter.kind.as_str() {
            "bool" => "on or off".to_owned(),
            "choice" => parameter.choices.join(" / "),
            _ => String::new(),
        },
    }
}

fn default_of(parameter: &ParameterInfo) -> String {
    match parameter.kind.as_str() {
        "bool" => {
            if parameter.default == 0.0 {
                "off".to_owned()
            } else {
                "on".to_owned()
            }
        }
        "choice" => parameter
            .choices
            .get(parameter.default as usize)
            .cloned()
            .unwrap_or_else(|| parameter.default.to_string()),
        _ => match &parameter.unit {
            Some(unit) => format!("{} {unit}", parameter.default),
            None => parameter.default.to_string(),
        },
    }
}

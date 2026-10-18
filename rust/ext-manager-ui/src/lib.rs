// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The extension manager's interface: egui, built to WebAssembly, in a browser.
//!
//! It owns no files and knows no paths.  Everything it shows it was told by
//! `rustacity-ext-manager` over HTTP on the loopback interface, and everything
//! it changes it asks that program to change - which is where the directory
//! work is, and where the tests for it are.  What is left here is the part a
//! browser is actually good at: a list, a detail pane, and what the extension
//! host said when it tried the extension.
//!
//! The page is served by the same program, so every request is same-origin, and
//! each one carries the header the server insists on.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use eframe::wasm_bindgen::prelude::wasm_bindgen;
use eframe::wasm_bindgen::{JsCast, JsValue};
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen_futures::{spawn_local, JsFuture};

/// Which side of the extensions root an extension is on, as the API spells it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Slot {
    #[default]
    Enabled,
    Disabled,
}

/// What is installed, as the API describes it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Inventory {
    root: String,
    entries: Vec<Entry>,
    ignored: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Entry {
    name: String,
    slot: Slot,
    directory: String,
    module_bytes: Option<u64>,
    manifest: Option<Manifest>,
    /// Why the manifest could not be used, when it could not be.
    manifest_error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    description: Option<String>,
    author: Option<String>,
    license: Option<String>,
    capabilities: Vec<String>,
    processes_audio: bool,
    parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Parameter {
    name: String,
    kind: String,
    minimum: Option<f64>,
    maximum: Option<f64>,
    default: f32,
    unit: Option<String>,
    choices: Vec<String>,
}

/// What the host made of an extension, as the API reports it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Report {
    loaded: bool,
    load_error: Option<String>,
    ran: bool,
    run_error: Option<String>,
    probe: String,
}

/// The API's envelope: every reply says whether it worked, and one more field.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Reply {
    ok: bool,
    error: Option<String>,
    inventory: Option<Inventory>,
    report: Option<Report>,
}

/// A line to show the user about what just happened.
enum Message {
    Did(String),
    Failed(String),
}

/// Everything the window shows.
#[derive(Default)]
struct Model {
    inventory: Option<Inventory>,
    selected: Option<(String, Slot)>,
    report: Option<Report>,
    filter: String,
    message: Option<Message>,
    busy: bool,
}

impl Model {
    /// The entry the detail pane is about, if it is still there.
    fn selected_entry(&self) -> Option<&Entry> {
        let (name, slot) = self.selected.as_ref()?;
        self.inventory
            .as_ref()?
            .entries
            .iter()
            .find(|entry| &entry.name == name && entry.slot == *slot)
    }

    fn counts(&self) -> (usize, usize) {
        let Some(inventory) = &self.inventory else {
            return (0, 0);
        };
        let enabled = inventory
            .entries
            .iter()
            .filter(|entry| entry.slot == Slot::Enabled)
            .count();
        (enabled, inventory.entries.len() - enabled)
    }
}

/// What the interface decided to ask for, worked out after the drawing is done
/// - so that no request is ever started while the model is still borrowed.
enum Action {
    Refresh,
    Reveal,
    Select(String, Slot),
    Toggle(String, Slot),
    Verify(String, Slot),
}

struct App {
    ctx: egui::Context,
    model: Rc<RefCell<Model>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        // Dark, to match the page around the canvas - which is what shows for
        // the moment before the first frame is drawn.
        ctx.set_theme(egui::ThemePreference::Dark);
        let model = Rc::new(RefCell::new(Model::default()));
        refresh(&model, &ctx);
        App { ctx, model }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut action: Option<Action> = None;

        egui::TopBottomPanel::top("head").show(ctx, |ui| {
            let model = self.model.borrow();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Rustacity extension manager");
                ui.add_space(12.0);
                if ui
                    .add_enabled(!model.busy, egui::Button::new("Refresh"))
                    .clicked()
                {
                    action = Some(Action::Refresh);
                }
                if ui
                    .add_enabled(!model.busy, egui::Button::new("Open extensions folder"))
                    .clicked()
                {
                    action = Some(Action::Reveal);
                }
                if model.busy {
                    ui.spinner();
                }
            });
            if let Some(inventory) = &model.inventory {
                let (enabled, turned_off) = model.counts();
                ui.label(
                    egui::RichText::new(format!(
                        "{}     {enabled} enabled, {turned_off} turned off",
                        inventory.root
                    ))
                    .small()
                    .weak(),
                );
            }
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

        egui::SidePanel::left("extensions")
            .resizable(true)
            .default_width(310.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Filter");
                    ui.text_edit_singleline(&mut self.model.borrow_mut().filter);
                });
                let filter = self.model.borrow().filter.to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let model = self.model.borrow();
                        let mut shown = 0;
                        let Some(inventory) = &model.inventory else {
                            ui.add_space(8.0);
                            ui.label("Reading the extensions folder...");
                            return;
                        };
                        for slot in [Slot::Enabled, Slot::Disabled] {
                            let entries: Vec<&Entry> = inventory
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
                                let label = match &entry.manifest {
                                    Some(manifest) => {
                                        // The folder name is what is on disk
                                        // and what every action addresses, and
                                        // it is often the only thing telling
                                        // two similarly named extensions apart,
                                        // so it is shown whenever it is not the
                                        // name the extension gives itself.
                                        let mut label = manifest.name.clone();
                                        if entry.name != manifest.name {
                                            label.push_str(&format!("   ({})", entry.name));
                                        }
                                        format!("{label}   v{}", manifest.version)
                                    }
                                    None => format!("{}   (manifest cannot be read)", entry.name),
                                };
                                if ui.selectable_label(selected, label).clicked() {
                                    action = Some(Action::Select(entry.name.clone(), slot));
                                }
                            }
                        }
                        if shown == 0 {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(if inventory.entries.is_empty() {
                                    "Nothing is installed here yet."
                                } else {
                                    "Nothing matches the filter."
                                })
                                .weak(),
                            );
                        }
                        if !inventory.ignored.is_empty() {
                            ui.add_space(10.0);
                            egui::CollapsingHeader::new(
                                egui::RichText::new(format!(
                                    "Not extensions ({})",
                                    inventory.ignored.len()
                                ))
                                .small(),
                            )
                            .show(ui, |ui| {
                                for name in &inventory.ignored {
                                    ui.label(egui::RichText::new(name).small().weak());
                                }
                            });
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            details(ui, &self.model.borrow(), &mut action);
        });

        match action {
            Some(Action::Refresh) => refresh(&self.model, &self.ctx),
            Some(Action::Reveal) => act(
                &self.model,
                &self.ctx,
                "reveal",
                json!({}),
                "Opened the extensions folder",
            ),
            Some(Action::Select(name, slot)) => {
                let mut model = self.model.borrow_mut();
                model.selected = Some((name, slot));
                // A verdict is about one directory at one moment, so it does
                // not follow the selection.
                model.report = None;
                model.message = None;
            }
            Some(Action::Toggle(name, slot)) => {
                let (verb, done) = match slot {
                    Slot::Enabled => ("disable", format!("Turned off '{name}'")),
                    Slot::Disabled => ("enable", format!("Turned on '{name}'")),
                };
                act(
                    &self.model,
                    &self.ctx,
                    verb,
                    json!({ "name": name, "slot": slot }),
                    &done,
                );
            }
            Some(Action::Verify(name, slot)) => verify(&self.model, &self.ctx, name, slot),
            None => {}
        }
    }
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

fn details(ui: &mut egui::Ui, model: &Ref<Model>, action: &mut Option<Action>) {
    let Some(entry) = model.selected_entry() else {
        ui.add_space(12.0);
        if model.inventory.is_some() {
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
        } else {
            ui.label("Reading the extensions folder...");
        }
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

fn range(parameter: &Parameter) -> String {
    match (parameter.minimum, parameter.maximum) {
        (Some(min), Some(max)) => format!("{min} .. {max}"),
        _ => match parameter.kind.as_str() {
            "bool" => "on or off".to_owned(),
            "choice" => parameter.choices.join(" / "),
            _ => String::new(),
        },
    }
}

fn default_of(parameter: &Parameter) -> String {
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

// ---------------------------------------------------------------------------
// Talking to the manager
// ---------------------------------------------------------------------------

/// The header every API call carries, and the reason the server asks for it.
const GUARD: (&str, &str) = ("x-rustacity-manager", "1");

/// Reads the extensions folder again.
fn refresh(model: &Rc<RefCell<Model>>, ctx: &egui::Context) {
    model.borrow_mut().busy = true;
    let model = Rc::clone(model);
    let ctx = ctx.clone();
    spawn_local(async move {
        let reply = request("GET", "/api/state", None).await;
        {
            let mut model = model.borrow_mut();
            model.busy = false;
            match reply {
                Ok(reply) => {
                    if let Some(inventory) = reply.inventory {
                        model.inventory = Some(inventory);
                    }
                    // If the selected extension has moved, follow it: the
                    // detail pane should not empty itself because it was just
                    // turned off.
                    if let Some((name, slot)) = model.selected.clone() {
                        let now = model.inventory.as_ref().and_then(|inventory| {
                            inventory
                                .entries
                                .iter()
                                .find(|entry| entry.name == name)
                                .map(|entry| entry.slot)
                        });
                        if let Some(now) = now {
                            if now != slot {
                                model.selected = Some((name, now));
                                model.report = None;
                            }
                        }
                    }
                }
                Err(message) => model.message = Some(Message::Failed(message)),
            }
        }
        ctx.request_repaint();
    });
}

/// Asks the manager to change something, then reads the folder back.
fn act(
    model: &Rc<RefCell<Model>>,
    ctx: &egui::Context,
    action: &str,
    body: serde_json::Value,
    done: &str,
) {
    model.borrow_mut().busy = true;
    // Owned before the future: the future outlives this call, so nothing it
    // captures may still be borrowed from here.
    let action = action.to_owned();
    let done = done.to_owned();
    let model = Rc::clone(model);
    let ctx = ctx.clone();
    spawn_local(async move {
        let reply = request("POST", &format!("/api/{action}"), Some(body.to_string())).await;
        {
            let mut model = model.borrow_mut();
            model.busy = false;
            model.message = Some(match reply {
                Ok(reply) if reply.ok => Message::Did(done),
                Ok(reply) => Message::Failed(
                    reply
                        .error
                        .unwrap_or_else(|| "the manager refused the change".to_owned()),
                ),
                Err(message) => Message::Failed(message),
            });
        }
        refresh(&model, &ctx);
    });
}

/// Asks the extension host whether this extension loads and runs.
fn verify(model: &Rc<RefCell<Model>>, ctx: &egui::Context, name: String, slot: Slot) {
    model.borrow_mut().busy = true;
    let model = Rc::clone(model);
    let ctx = ctx.clone();
    spawn_local(async move {
        let body = json!({ "name": name, "slot": slot }).to_string();
        let reply = request("POST", "/api/verify", Some(body)).await;
        {
            let mut model = model.borrow_mut();
            model.busy = false;
            match reply {
                Ok(reply) => {
                    if !reply.ok {
                        model.message = Some(Message::Failed(
                            reply
                                .error
                                .unwrap_or_else(|| "the check could not be run".to_owned()),
                        ));
                    } else {
                        model.report = reply.report;
                    }
                }
                Err(message) => model.message = Some(Message::Failed(message)),
            }
        }
        ctx.request_repaint();
    });
}

/// One request to the manager, parsed as the envelope every reply uses.
async fn request(method: &str, url: &str, body: Option<String>) -> Result<Reply, String> {
    let window = web_sys::window().ok_or_else(|| "this program needs a window".to_owned())?;
    let options = web_sys::RequestInit::new();
    options.set_method(method);
    // The page came from the same server, so this is a same-origin request by
    // construction: the browser has no cross-origin case to make here.
    options.set_mode(web_sys::RequestMode::SameOrigin);
    let headers = web_sys::Headers::new().map_err(show)?;
    headers.set(GUARD.0, GUARD.1).map_err(show)?;
    if let Some(body) = &body {
        headers
            .set("content-type", "application/json")
            .map_err(show)?;
        options.set_body(&JsValue::from_str(body));
    }
    options.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(url, &options).map_err(show)?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(show)?;
    let response: web_sys::Response = response.dyn_into().map_err(show)?;
    let text = JsFuture::from(response.text().map_err(show)?)
        .await
        .map_err(show)?;
    let text = text.as_string().unwrap_or_default();
    serde_json::from_str(&text)
        .map_err(|error| format!("the manager's reply was not the JSON expected: {error}"))
}

/// A `JsValue` as something a person can read.
fn show(value: JsValue) -> String {
    format!("{value:?}")
}

// ---------------------------------------------------------------------------
// The browser's entry point
// ---------------------------------------------------------------------------

thread_local! {
    /// eframe's runner holds the window in `Rc`s, so it has to outlive the call
    /// that starts it.  Keeping it here keeps it for the life of the page.
    static RUNNER: RefCell<Option<eframe::WebRunner>> = const { RefCell::new(None) };
}

/// Called once from the page, with the canvas to draw in.
#[wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
    let runner = RUNNER.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(eframe::WebRunner::new)
            .clone()
    });
    runner
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| Ok(Box::new(App::new(cc)))),
        )
        .await
}

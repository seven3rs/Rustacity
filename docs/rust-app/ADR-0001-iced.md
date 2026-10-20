# ADR 0001: iced for the Rust shell

- **Status:** accepted
- **Date:** 2026-10-20
- **Deciders:** Sato

## Context

The C++ application's user interface is built on wxWidgets 3.1.3, the pinned
Audacity fork. The Rust shell replaces it with a native Rust toolkit. The shell
must host a large editor: docked panels, menus and toolbars, dialogs, a
waveform canvas that redraws at interactive rates, and later accessibility and
localisation.

Four candidates were considered: iced, egui, Slint and Tauri.

## Decision

Use **iced 0.14** for the Rust shell.

iced is retained-mode, written in Rust, and exposes a canvas widget with custom
drawing, which is the shape the waveform view needs. It keeps the application's
state and messages explicit (the Elm architecture), which maps well onto the
editor's command model and makes the engine/UI boundary testable. It is MIT
licensed and has no C++ dependency.

Docking is not provided by iced, so `rustacity-ui` will own a small pane tree
instead of adopting a third-party dock crate in M0. That keeps the layout under
our control while the editor is still small.

## Consequences

- The shell is pure Rust and builds with Cargo, independent of the C++ tree.
- The waveform canvas is a custom iced widget with a downsampled peak cache; it
  is not a per-sample draw.
- Accessibility (AccessKit) and native menu behaviour need explicit work and
  are post-MVP tracks.
- The renderer can use wgpu or tiny-skia. The default build keeps both so a
  machine without a usable GPU still starts; the waveform path is tuned for
  wgpu first.

## Alternatives

- **egui** — fastest to an MVP and has `egui_dock`, but immediate mode makes a
  large, stateful editor harder to structure, and long-term accessibility and
  text editing are weaker.
- **Slint** — good declarative tooling and native look, but its licensing and
  accessibility story were less certain for a GPL desktop editor.
- **Tauri** — excellent for complex UI, but the UI would be HTML/CSS/JS, not
  Rust, which defeats the purpose of the migration.

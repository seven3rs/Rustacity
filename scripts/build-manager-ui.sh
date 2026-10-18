#!/usr/bin/env bash
#
# Rustacity: A Digital Audio Editor
# Copyright (C) 2026 Sadik Pranto
# SPDX-License-Identifier: GPL-2.0-or-later
#
# Builds the extension manager's interface for the browser and lays it out as
# the directory rustacity-ext-manager serves:
#
#   rust/ext-manager-ui/dist/
#     index.html
#     rustacity-ext-manager-ui.js
#     rustacity-ext-manager-ui_bg.wasm
#
# The interface is a WebAssembly program (rust/ext-manager-ui), which is a
# workspace of its own: it targets wasm32-unknown-unknown, nothing links it,
# and building the host never builds it.  wasm-bindgen turns the compiled
# module into the JavaScript the page imports.
#
# Usage:
#   scripts/build-manager-ui.sh              build it
#   scripts/build-manager-ui.sh --check      build it and say where it is
#
# Requires:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.129 --locked
#
# On a Windows machine with no MSVC linker, name a toolchain that has one:
#   RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu scripts/build-manager-ui.sh
#
# The wasm-bindgen version has to match the wasm-bindgen crate the interface
# depends on, which is what rust/ext-manager-ui/Cargo.lock pins.  The check
# below says so rather than letting it fail inside wasm-bindgen.

set -euo pipefail

cargo_bin="${CARGO:-cargo}"
target="wasm32-unknown-unknown"
out_name="rustacity-ext-manager-ui"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ui="$here/rust/ext-manager-ui"
dist="$ui/dist"
module="$ui/target/$target/release/rustacity_ext_manager_ui.wasm"

if [ ! -f "$ui/Cargo.toml" ]; then
    echo "error: $ui/Cargo.toml is missing" >&2
    exit 1
fi

bindgen="${WASM_BINDGEN:-wasm-bindgen}"
if ! command -v "$bindgen" >/dev/null 2>&1; then
    echo "error: $bindgen is not on PATH." >&2
    echo "       cargo install wasm-bindgen-cli --version 0.2.129 --locked" >&2
    exit 1
fi

# The crate's own pinned version, so that a mismatch is reported here with the
# command that fixes it rather than by wasm-bindgen in its own words.
if [ -f "$ui/Cargo.lock" ]; then
    required="$(awk '
        /^name = "wasm-bindgen"$/ { found = 1; next }
        found && /^version = /      { gsub(/[",]/, "", $3); print $3; exit }
    ' "$ui/Cargo.lock")"
    have="$("$bindgen" --version 2>/dev/null | awk '{print $2}')"
    if [ -n "${required:-}" ] && [ "${have:-}" != "$required" ]; then
        echo "error: wasm-bindgen $have is installed and the interface pins $required." >&2
        echo "       cargo install wasm-bindgen-cli --version $required --locked" >&2
        exit 1
    fi
fi

echo "building the interface for $target"
"$cargo_bin" build --manifest-path "$ui/Cargo.toml" --target "$target" --release

if [ ! -f "$module" ]; then
    echo "error: $module was not produced" >&2
    exit 1
fi

echo "wrapping it for the browser"
rm -rf "$dist"
mkdir -p "$dist"
"$bindgen" --target web --out-dir "$dist" --out-name "$out_name" "$module"
cp "$ui/index.html" "$dist/index.html"

echo
echo "built $dist"
echo "run it with:"
echo "  $cargo_bin run -p rustacity-ext-manager -- --assets '$dist'"

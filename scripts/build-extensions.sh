#!/usr/bin/env bash
#
# Rustacity: A Digital Audio Editor
# Copyright (C) 2026 Sato
# SPDX-License-Identifier: GPL-2.0-or-later
#
# Builds every extension for wasm32-wasip1 and lays it out as an installable
# extension directory under rust/extensions/dist/:
#
#   dist/gain-invert/
#     extension.toml
#     extension.wasm
#
# The list of modules built comes from rust/extensions/variants.txt, which the
# host's integration tests read too, so the two cannot disagree about what a
# variant is called or which feature produces it.  The tests build their own
# fixtures through the same list (rust/ext-host/tests/fixtures.rs); this script
# is for people and for CI, and does exactly the same thing.
#
# Usage:
#   scripts/build-extensions.sh              build everything
#   scripts/build-extensions.sh gain-invert  build one variant, by dist name
#
# Requires the wasm32-wasip1 target:  rustup target add wasm32-wasip1

set -euo pipefail

cargo_bin="${CARGO:-cargo}"
target="wasm32-wasip1"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
extensions="$here/rust/extensions"
dist="$extensions/dist"
variants="$extensions/variants.txt"

if [ ! -f "$variants" ]; then
    echo "error: $variants is missing" >&2
    exit 1
fi

if ! rustup target list --installed 2>/dev/null | grep -qx "$target"; then
    echo "error: the $target target is not installed; run: rustup target add $target" >&2
    exit 1
fi

wanted="${1:-}"

mkdir -p "$dist"

built=0
while read -r name dir feature module; do
    # A checkout with CRLF line endings - Git for Windows is one by default -
    # leaves a carriage return on the last field of every line, and turns a
    # blank line into a single field holding nothing but that character.  Left
    # in, that blank line falls through the case below, points the crate at the
    # extensions workspace and the module at the release directory, and the
    # build fails at the bottom of the loop having built the wrong thing.
    name=${name%$'\r'} dir=${dir%$'\r'}
    feature=${feature%$'\r'} module=${module%$'\r'}
    case "$name" in ''|'#'*) continue ;; esac
    if [ -n "$wanted" ] && [ "$wanted" != "$name" ]; then
        continue
    fi

    crate="$extensions/$dir"
    if [ ! -f "$crate/Cargo.toml" ]; then
        echo "error: no crate at $crate" >&2
        exit 1
    fi

    features=()
    if [ "$feature" != "-" ]; then
        features=(--features "$feature")
    fi

    echo "building $name${feature:+ (feature $feature)}"
    "$cargo_bin" build --release --target "$target" \
        --manifest-path "$crate/Cargo.toml" ${features[@]+"${features[@]}"} \
        >/dev/null

    module_path="$extensions/target/$target/release/$module"
    if [ ! -f "$module_path" ]; then
        echo "error: $module_path was not produced by the build" >&2
        exit 1
    fi

    mkdir -p "$dist/$name"
    cp "$crate/extension.toml" "$dist/$name/extension.toml"
    cp "$module_path" "$dist/$name/extension.wasm"
    built=$((built + 1))
done < "$variants"

if [ "$built" -eq 0 ]; then
    echo "error: nothing matched '${wanted}' in $variants" >&2
    exit 1
fi

echo
echo "built $built extension(s) into $dist:"
for directory in "$dist"/*/; do
    printf '  %-18s %8s bytes\n' "$(basename "$directory")" \
        "$(wc -c < "$directory/extension.wasm")"
done

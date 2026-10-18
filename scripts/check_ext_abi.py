#!/usr/bin/env python3
"""Check that the extension host and its C header agree about the ABI.

Why this script exists
----------------------
``rust/ext-host/include/rustacity_ext.h`` is written by hand: it mirrors the
``#[no_mangle] pub unsafe extern "C"`` functions in ``rust/ext-host/src/ffi.rs``,
and it publishes three sets of constants that live in Rust - the status codes
(``ExtError::status``), the parameter kinds, and the API version.  Nothing in
the build notices when the two drift apart: a function exported by Rust but
missing from the header still compiles, a status code renumbered on one side
only still links, and both show up later as a crash in a platform build that
cannot be reproduced locally.

The check is textual: it compares the names and the numbers and fails when they
differ, so the header is updated in the same commit as the change that made them
disagree.  Run it from the repository root:

    python3 scripts/check_ext_abi.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FFI = ROOT / "rust" / "ext-host" / "src" / "ffi.rs"
HEADER = ROOT / "rust" / "ext-host" / "include" / "rustacity_ext.h"
ERRORS = ROOT / "rust" / "ext-api" / "src" / "error.rs"
ABI = ROOT / "rust" / "ext-api" / "src" / "abi.rs"

BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.DOTALL)
LINE_COMMENT = re.compile(r"//[^\n]*")

# `pub extern "C" fn name(` and `pub unsafe extern "C" fn name(`.
EXPORT = re.compile(r'pub\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)')

# A declaration: a return type from the header's small vocabulary, then the
# function name and "(".  The parameter list may run over several lines.
RETURN_TYPE = (
    r"(?:void|int|float|double|size_t|uint64_t"
    r"|const\s+char\s*\*|rustacity_ext_[a-z0-9_]+_t\s*\*)"
)
DECLARATION = re.compile(
    rf"^\s*{RETURN_TYPE}\s*(rustacity_ext_[a-z0-9_]+)\s*\(", re.MULTILINE
)

# The status method of ExtError: `ExtError::Trap(_) => 6,`.
STATUS_ARM = re.compile(r"ExtError::([A-Za-z0-9_]+)\s*(?:\([^)]*\))?\s*=>\s*(\d+)")
STATUS_OK = re.compile(r"pub const STATUS_OK: i32 = (\d+);")

# The parameter kinds and the API version, as Rust constants.
RUST_CONSTANT = re.compile(r"pub const ([A-Z0-9_]+): (?:i32|c_int) = (\d+);")

# `#define RUSTACITY_EXT_ERROR_TRAP 6`.
DEFINE = re.compile(r"^#define\s+(RUSTACITY_EXT_[A-Z0-9_]+)\s+(\d+)\s*$", re.MULTILINE)

# The variant of ExtError, and the macro name the header publishes it under.
STATUS_NAMES = {
    "Internal": "RUSTACITY_EXT_ERROR_INTERNAL",
    "ManifestInvalid": "RUSTACITY_EXT_ERROR_MANIFEST_INVALID",
    "ModuleInvalid": "RUSTACITY_EXT_ERROR_MODULE_INVALID",
    "AbiMismatch": "RUSTACITY_EXT_ERROR_ABI_MISMATCH",
    "CapabilityDenied": "RUSTACITY_EXT_ERROR_CAPABILITY_DENIED",
    "Trap": "RUSTACITY_EXT_ERROR_TRAP",
    "Timeout": "RUSTACITY_EXT_ERROR_TIMEOUT",
    "OutOfMemory": "RUSTACITY_EXT_ERROR_OUT_OF_MEMORY",
    "ModuleReportedError": "RUSTACITY_EXT_ERROR_MODULE_REPORTED",
    "Io": "RUSTACITY_EXT_ERROR_IO",
    "Poisoned": "RUSTACITY_EXT_ERROR_POISONED",
    "InvalidArgument": "RUSTACITY_EXT_ERROR_INVALID_ARGUMENT",
}

# The kinds, and the macro name the header publishes each under.
KIND_NAMES = {
    "RUSTACITY_EXT_PARAM_FLOAT": "float",
    "RUSTACITY_EXT_PARAM_BOOL": "bool",
    "RUSTACITY_EXT_PARAM_CHOICE": "choice",
}


def strip_comments(text: str) -> str:
    """Remove C and C++ comments so prose is never mistaken for code."""
    return LINE_COMMENT.sub("", BLOCK_COMMENT.sub("", text))


def exported_functions(text: str) -> dict[str, bool]:
    """Map each exported function name to whether it carries #[no_mangle]."""
    source = strip_comments(text)
    found: dict[str, bool] = {}
    for match in EXPORT.finditer(source):
        name = match.group(1)
        # Without #[no_mangle] the symbol is mangled and C cannot call it.
        found[name] = source[: match.start()].rstrip().endswith("#[no_mangle]")
    return found


def declared_functions(text: str) -> set[str]:
    """The function names the C header declares."""
    return set(DECLARATION.findall(strip_comments(text)))


def defines(text: str) -> dict[str, int]:
    """The numbered `#define`s the C header publishes."""
    return {name: int(value) for name, value in DEFINE.findall(text)}


def check_functions(problems: list[str]) -> int:
    functions = exported_functions(FFI.read_text(encoding="utf-8"))
    exported = set(functions)
    mangled = sorted(name for name, unmangled in functions.items() if not unmangled)
    declared = declared_functions(HEADER.read_text(encoding="utf-8"))

    header = HEADER.relative_to(ROOT).as_posix()
    ffi = FFI.relative_to(ROOT).as_posix()

    missing = sorted(exported - declared)
    if missing:
        problems.append(
            f"exported by {ffi} but not declared in {header}:\n"
            + "".join(f"   {name}\n" for name in missing)
        )
    extra = sorted(declared - exported)
    if extra:
        problems.append(
            f"declared in {header} but not exported by {ffi}:\n"
            + "".join(f"   {name}\n" for name in extra)
        )
    if mangled:
        problems.append(
            "exported without #[no_mangle], so C cannot link them:\n"
            + "".join(f"   {name}\n" for name in mangled)
        )

    print(f"{ffi}: {len(exported)} exports")
    print(f"{header}: {len(declared)} declarations")
    for name in sorted(exported & declared):
        print(f"   {name}")
    return len(exported)


def check_numbers(problems: list[str]) -> None:
    """The status codes, the parameter kinds and the API version."""
    header = HEADER.relative_to(ROOT).as_posix()
    defines_found = defines(HEADER.read_text(encoding="utf-8"))

    # The statuses: what each ExtError variant maps to, and STATUS_OK.
    error_source = ERRORS.read_text(encoding="utf-8")
    statuses = {variant: int(value) for variant, value in STATUS_ARM.findall(error_source)}
    ok = STATUS_OK.search(error_source)
    if not ok:
        problems.append(f"{ERRORS.relative_to(ROOT).as_posix()}: STATUS_OK was not found")
        return
    statuses["Ok"] = int(ok.group(1))

    unnamed = {name for name in defines_found if name.startswith("RUSTACITY_EXT_ERROR_")}
    for variant, number in sorted(statuses.items(), key=lambda pair: pair[1]):
        macro = "RUSTACITY_EXT_OK" if variant == "Ok" else STATUS_NAMES.get(variant)
        if macro is None:
            problems.append(
                f"ExtError::{variant} has no status name; add it to STATUS_NAMES in "
                f"{Path(__file__).name} and to {header}"
            )
            continue
        if macro not in defines_found:
            problems.append(f"{header} does not define {macro} (ExtError maps it to {number})")
        elif defines_found[macro] != number:
            problems.append(
                f"{macro} is {defines_found[macro]} in {header} but {number} in "
                f"{ERRORS.relative_to(ROOT).as_posix()}"
            )
        unnamed.discard(macro)
    for name in sorted(unnamed):
        problems.append(f"{header} defines {name}, which no ExtError variant maps to")

    # The parameter kinds, from the constants the FFI hands out.
    ffi_relative = FFI.relative_to(ROOT).as_posix()
    kinds = {
        name: int(number)
        for name, number in RUST_CONSTANT.findall(FFI.read_text(encoding="utf-8"))
        if name.startswith("RUSTACITY_EXT_PARAM_")
    }
    for name, number in sorted(kinds.items()):
        if name not in defines_found:
            problems.append(f"{header} does not define {name} (it is kind {number})")
        elif defines_found[name] != number:
            problems.append(
                f"{name} is {defines_found[name]} in {header} but {number} in {ffi_relative}"
            )
    for name in sorted(set(defines_found) & set(KIND_NAMES)):
        if name not in kinds:
            problems.append(f"{header} defines {name}, which {ffi_relative} does not publish")

    # The API version the host implements.
    versions = dict(RUST_CONSTANT.findall(ABI.read_text(encoding="utf-8")))
    version = versions.get("API_VERSION")
    if version is None:
        problems.append(f"{ABI.relative_to(ROOT).as_posix()}: API_VERSION was not found")
    elif defines_found.get("RUSTACITY_EXT_API_VERSION") != int(version):
        problems.append(
            f"RUSTACITY_EXT_API_VERSION is "
            f"{defines_found.get('RUSTACITY_EXT_API_VERSION')} in {header} but "
            f"{version} in {ABI.relative_to(ROOT).as_posix()}"
        )

    print(f"statuses: {len(statuses)}, each named in the header")
    print(f"parameter kinds: {len(kinds)}")
    print(f"API version: {version}")


def main() -> int:
    problems: list[str] = []
    check_functions(problems)
    check_numbers(problems)

    if problems:
        for problem in problems:
            print(f"\nERROR: {problem}", file=sys.stderr)
        return 1

    print("\nThe extension ABI and its header agree.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

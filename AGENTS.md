# AGENTS.md

Rustacity is a fork of **Audacity 2.4.1**: the `src/` C++ tree is the (legacy) editor, and
`rust/` holds the Rust audio core plus a sandboxed WebAssembly extension engine. The full
builder docs live in `README.md`, `CONTRIBUTING.md` and `docs/`. Read this file first; it
only records what is easy to get wrong.

## Hard rules

- **`src/` is legacy and is not extended.** New DSP goes in `rust/dsp/`. When a unit is
  ported, the C++ that held the maths is deleted (see "Porting" below).
- **Never edit `lib-src/`.** Vendored third-party code.
- **No tree-wide renames.** Files, classes and functions keep their names.
- **Zero compiler warnings** is the bar.

## Rust workspaces and the three traps

`rust/` is one cargo workspace (`dsp`, `ext-api`, `ext-host`, `ext-cli`, `ext-manager`,
`ext-manager-gui`). The desktop shell is a second workspace in `rust/app/` (`app`, `ui`,
`engine`, `io`, `project`). Three things bite:

1. **`rust/extensions/` is a separate workspace, excluded from `rust/`.** A plain
   `cargo test --workspace` does **not** build or test it. It targets `wasm32-wasip1` and is
   built by `scripts/build-extensions.sh`; `rust/extensions/variants.txt` is the single list
   the build script and the host's test fixtures both read.
2. **`rust/ext-host`'s tests compile their own wasm fixtures**, so run
   `rustup target add wasm32-wasip1` before `cargo test --workspace`, or the host tests fail.
3. **`rust/app/` is a separate workspace, excluded from `rust/`.** A plain
   `cargo test --workspace` in `rust/` does not build the shell, and its iced/wgpu stack is
   not part of the C++ application. Run the shell's gates from `rust/app`; it depends on
   `rust/dsp` by path.

```sh
cd rust
cargo test --workspace          # unit, golden-value and ext-host-into-wasm tests

cd app
cargo test --workspace          # the iced shell (M0: engine vocabulary and views)
```

## Verify before pushing (CI gates)

```sh
cd rust && cargo test --workspace
python3 scripts/check_dsp_abi.py   # rust/dsp/include/rustacity_dsp.h <-> dsp/src/ffi.rs
python3 scripts/check_ext_abi.py   # rust/ext-host/include/rustacity_ext.h <-> ext-host/src/ffi.rs
```

Adding or changing an ABI entry point means editing the hand-written header and `ffi.rs` in
the **same commit**, or these scripts fail.

Clippy and `cargo fmt --check` are **scoped on purpose** — `rust/dsp` carries pre-existing
lints and is *not* held to them; match its surrounding style by hand. The gates cover only
`rustacity-ext-api`, `rustacity-ext-host`, `rustacity-ext-cli`, `rustacity-ext-manager`,
`rustacity-ext-manager-gui`, and the `rust/extensions` crates (for `wasm32-wasip1`):

```sh
cd rust && cargo clippy -p rustacity-ext-api -p rustacity-ext-host -p rustacity-ext-cli \
  -p rustacity-ext-manager -p rustacity-ext-manager-gui --all-targets -- -D warnings
cd rust/extensions && cargo clippy --workspace --all-targets --target wasm32-wasip1 -- -D warnings
```

The shell's gates are separate and cover only its five crates, so the DSP crate's
pre-existing formatting is not touched:

```sh
cd rust/app
cargo fmt -p rustacity-app -p rustacity-engine -p rustacity-io -p rustacity-project -p rustacity-ui --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Panic policy (do not change)

`rust/Cargo.toml` sets `panic = "unwind"`: every `#[no_mangle] extern "C"` entry point runs
under `ffi::guard`, so a core bug yields a wrong value, never an abort that costs unsaved
work. `rust/extensions` sets `panic = "abort"` instead, which is what turns an extension bug
into a host-survivable Trap. Keep both.

## Porting a unit (extract, capture, port, delete)

A port is **not finished until the C++ is deleted**. Golden values are captured from the C++
while it still exists (dumpers in `rust/dsp/goldens/capture/`, records appended to
`rust/dsp/goldens/reference.txt`), then a `rust/dsp/tests/<unit>_golden.rs` reads them.
Capture and golden tests run on **Linux only** and compare with a tolerance (float/libm
varies by platform). Details: `docs/goldens.md`, `docs/architecture.md`. Check
`docs/port-status.md` and `docs/roadmap.md` before starting to avoid duplicate work.

## Building the editor

CMake is the only build; it runs `cargo build --release -p rustacity-dsp -p
rustacity-ext-host` and links both static libraries, so a Rust toolchain is required. The
full build also needs C++/CMake and the pinned wxWidgets fork (`audacity/wxWidgets`, branch
`audacity-fixes-3.1.3`). `.github/workflows/cmake_build.yml` is the canonical per-platform
recipe — follow the job for your OS rather than guessing. The manager crates are **not** part
of the application build.

## Docs map

| Topic | File |
|---|---|
| Architecture, C ABI rules, porting rule, CI jobs | `docs/architecture.md` |
| What every top-level directory is (ours vs inherited) | `docs/repository-layout.md` |
| What is ported / what is next | `docs/port-status.md`, `docs/roadmap.md` |
| Golden capture loop | `docs/goldens.md`, `rust/dsp/goldens/capture/README.md` |
| Extensions: contract, sandbox, writer guide, manager, open seams | `docs/extensions/*.md` |

`README.md` is the front page; `CONTRIBUTING.md` is the contributor contract. Comments and
docs in the tree are unusually detailed — read the doc for a subsystem before changing it.

# Architecture

Rustacity is two halves joined by a C ABI: a C++ application that owns the
user interface and the project, and a Rust core that owns the audio maths,
the audio buffers and edits behind WAV-imported clips, and the sandbox that
runs WebAssembly extensions.

## The C++ shell

`src/` is the Audacity 2.4.1 application. It owns the wxWidgets user
interface, the project and track model, the non-WAV importers, the effect
framework and every effect dialog. It is legacy: it is not extended, and where
a unit has been ported, the class that used to hold the maths is now a caller
of the Rust core. A WAV-imported clip is the first case where the data behind
a C++ model object lives in Rust.

## The Rust core

`rust/dsp/` is the `rustacity-dsp` crate: the audio maths, with no
dependencies. `rust/engine/` is the project/track/buffer model,
`rust/io/` holds the WAV decoder and the peak cache, and `rust/backend/` is
the C ABI over one mono `Sequence` and the WAV import. The editor keeps its
C++ project and track objects; a WAV-imported clip's samples live in
`rust/backend`. Beside those, `rust/ext-api/` holds the extension API's
manifest and error types, and `rust/ext-host/` is the sandbox that runs
WebAssembly extensions. `rust/ext-cli/` is a command-line tool for running one
extension over one WAV file, and is not part of the application.
`rust/ext-manager/` and `rust/ext-manager-gui/` are the extension manager: a
listing and a window that read and rearrange the extensions directory
([`docs/extensions/MANAGER.md`](extensions/MANAGER.md)). The application links
`ext-host` (and `ext-api` through it), but not the command-line tool or the
two manager crates.

The CMake build names the three crates the application links - `cargo build
--release -p rustacity-dsp -p rustacity-ext-host -p rustacity-backend` - and
links the results as static libraries, so a Rust toolchain is required to
build Rustacity at all. The workspace is also buildable and testable on its
own:

```sh
cd rust
cargo test --workspace
```

`rust/extensions/` is a workspace of its own, deliberately excluded from that
one, holding the example extensions and the misbehaving fixtures the sandbox's
tests run. They are built for `wasm32-wasip1`, never into the application. The
extension side has documents of its own:
[`docs/extensions/API.md`](extensions/API.md) is the contract,
[`docs/extensions/SANDBOX.md`](extensions/SANDBOX.md) is what the sandbox is
and is not, [`docs/extensions/WRITING_AN_EXTENSION.md`](extensions/WRITING_AN_EXTENSION.md)
is how to write one, and
[`docs/extensions/INTEGRATION_NOTES.md`](extensions/INTEGRATION_NOTES.md) says
what is not built or tested yet.

## The ABI

The C interface is `rust/dsp/include/rustacity_dsp.h`; the implementations are
in `rust/dsp/src/ffi.rs`. The extension host has a second one,
`rust/ext-host/include/rustacity_ext.h`, held to the same rule by
`scripts/check_ext_abi.py`. The audio backend has a third,
`rust/backend/include/rustacity_backend.h`, checked by
`scripts/check_backend_abi.py`. The rules that keep the ABIs safe:

- **Opaque handles, one per channel.** A unit exposes `_new`, `_free` and a
  block call (usually `_process_block`), not one call per sample. The
  `rustacity_ebur128_*` entry points are the model.
- **Rust owns the loop.** Block calls take the input and output pointers,
  which may alias, plus the sample count and the current parameter values; the
  iteration over samples happens inside the crate.
- **No C++ type crosses the boundary.** Pointers, `size_t` lengths and scalars
  only - no wxWidgets type, no C++ class, no STL container, no `bool` that is
  really a C++ `bool`.
- **Arguments are validated.** A null or nonsensical argument returns a
  harmless value instead of taking the editor down.
- **The header and the exports must agree.** `scripts/check_dsp_abi.py`,
  `scripts/check_ext_abi.py` and `scripts/check_backend_abi.py` fail if a
  header and its `ffi.rs` differ, so an entry point is added to both in the
  same commit.

The `rustacity_fft_*` entry points are the exception to the handle rule, and
deliberately so: a transform is arithmetic over buffers the caller already
owns, and the tables it runs on (the bit-reversal pointers and the sine values)
belong to the C++ pool `GetFFT` keeps, so they are passed in with each call
rather than held in a handle. The only state on the Rust side is the bit table
of the complex transform, which `rustacity_fft_deinit` releases at shutdown,
exactly as the C++ freed `gFFTBitTable`.

## Audio storage and the `Sequence` boundary

The first backend slice keeps the wxWidgets editor and moves one boundary:
`Sequence`, the C++ class that owns a clip's mono samples and the edits on
them. There are now two paths through that class:

- **Legacy path.** A sequence loaded from an `.aup` block file, or imported by
  a non-WAV format, keeps the original `BlockArray` code. Nothing about its
  behaviour changes.
- **Rust path.** A WAV imported by `PCMImportFileHandle` is decoded by
  `rust/backend` (through `rust/io`'s hound decoder) into one mono sequence per
  channel. The C++ `Sequence` holds a `RustSequence` handle and delegates
  `GetNumSamples`, `Get`, `SetSamples`, `GetMinMax`, `GetRMS`,
  `GetWaveDisplay`, `Copy`, `Paste`, `Append`, `Delete`, `InsertSilence`,
  `GetBlockStart`, `GetBestBlockSize` and `GetIdealAppendLen` to it. The C++
  `WaveClip`, `WaveTrack`, TrackPanel, menu and dialogs are unchanged.

`Sequence::WriteXML` converts a Rust-backed sequence into legacy block files in
place before writing the project, so `.aup` save/reload still works
without teaching the project reader a Rust format. That is a bridge, not the
destination: a Rust project format will replace it. Until then, editing a
WAV-imported clip creates a block file on save, and the legacy path remains
compiled for existing projects.

This slice is therefore *not* a completed "delete the C++ maths" port: the
C++ `Sequence` still contains the block-file implementation for legacy
projects. The port-status ledger records the boundary and what remains.

## Panic policy

`panic = "unwind"` in `Cargo.toml`, and every entry point runs under
`ffi::guard`, which catches a panic and returns a fallback. A bug in the core
must yield a wrong value, not an abort: the user may have unsaved work. Do not
set `panic = "abort"`.

## What CI proves

`.github/workflows/cmake_build.yml` is the only workflow. Its jobs:

| job | what it does |
| --- | --- |
| `build` (3 platforms) | builds wxWidgets from the pinned fork (`audacity/wxWidgets`, branch `audacity-fixes-3.1.3`), then configures and builds the application with CMake - which compiles the Rust cores and links them - and packages it: a 7z, an MSI (WiX) and an installer (Inno Setup) on Windows, a disk image on macOS, an AppImage on Linux |
| `Rust Tests` | adds the `wasm32-wasip1` target (the extension tests build their own WebAssembly fixtures), then `cargo test --workspace` (unit tests, the golden-value tests, the extension engine's tests and the audio backend's tests), `python3 scripts/check_dsp_abi.py`, `python3 scripts/check_backend_abi.py`, and `clippy -D warnings` plus `cargo fmt --check` for `rustacity-backend`, `rustacity-engine` and `rustacity-io` |
| `Goldens` | builds and runs any dumper left in `rust/dsp/goldens/capture/` and uploads what they print as an artifact |
| `Extensions` | builds every variant in `rust/extensions/variants.txt` for `wasm32-wasip1`, checks `rustacity_ext.h` against the host's FFI, and holds the three new crates - and the extension crates themselves - to `clippy -D warnings` and `cargo fmt --check` |
| `Extension manager` | holds `rustacity-ext-manager` and `rustacity-ext-manager-gui` to `clippy -D warnings` and `cargo fmt --check`. Their tests are part of the `Rust Tests` run above; the window is not opened here, because a runner has no display |
| `CLI artifacts` | builds `rustacity-ext-cli` and the extensions for Linux, runs the quick start README.md documents on the committed sample - including that the generated sample is byte for byte the committed one - and packages the binary, the extensions and the contract documents into a tarball |
| `Publish Release` | on a `rustacity-*` tag only: waits for the builds, the tests, the extension engine, the manager and the CLI archive, renames each package the run produced after the platform it holds, adds the source tarball of the tag, and publishes the release |

Because the capture is what makes a goldens comparison meaningful, the build
matrix is also the only check that the ports behave on Windows and macOS;
capture and tests run on Linux only (see [`docs/goldens.md`](goldens.md)).

## Porting a unit to Rust

The rule is **extract, capture, port, delete**, and a port is not finished
until the C++ implementation is deleted. Leaving both copies in place grows
the Rust half without shrinking the C++ half, which turns one implementation
into two to keep in step.

1. **Extract.** Move the maths into a unit that compiles on its own, with no
   wxWidgets, and leave the effect class calling it. Nothing may change in the
   move - the extracted header is a copy, not a revision.
2. **Capture.** Add a dumper to `rust/dsp/goldens/capture/` and append what it
   prints to `rust/dsp/goldens/reference.txt`, while the C++ still exists to
   print it. The dumper mechanics are in
   `rust/dsp/goldens/capture/README.md`; the workflow is in
   [`docs/goldens.md`](goldens.md).
3. **Port.** Write the maths as a module under `rust/dsp/src/` with unit tests
   of its own, then expose it in `ffi.rs` and the header, and thin the C++
   class down to parameter marshalling. Test against the captured values.
4. **Delete.** Remove the C++ maths and the dumper, run `cargo test
   --workspace`, and build.

Where a port has deliberately changed behaviour, or dropped C++ code that was
provably dead, say so in the commit message: the deviation is part of the
port. `src/effects/Biquad.cpp` with `ScienFilter.cpp`, and the four realtime
effects (`BassTreble`, `Phaser`, `Wahwah`, `Distortion`), are the worked
examples - see [`docs/port-status.md`](port-status.md).

The Rust-side detail of each step - crate layout, how to add a module and an
entry point - is in [`rust/README.md`](../rust/README.md).

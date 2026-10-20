# Architecture

Rustacity is two halves joined by a C ABI: a C++ application that owns the
user interface and the project, and a Rust core that owns the audio maths and
runs WebAssembly extensions in a sandbox.

## The C++ shell

`src/` is the Audacity 2.4.1 application. It owns the wxWidgets user
interface, the project and track model, the audio I/O and file formats, the
effect framework and every effect dialog. It is legacy: it is not extended,
and where a unit has been ported, the class that used to hold the maths is now
a caller of the Rust core.

## The Rust core

`rust/dsp/` is the `rustacity-dsp` crate: the audio maths, and the only crate
with no dependencies. Beside it, `rust/ext-api/` holds the extension API's
manifest and error types, and `rust/ext-host/` is the sandbox that runs
WebAssembly extensions. `rust/ext-cli/` is a command-line tool for running one
extension over one WAV file, and is not part of the application.
`rust/ext-manager/` and `rust/ext-manager-gui/` are the extension manager: a
listing and a window that read and rearrange the extensions directory
([`docs/extensions/MANAGER.md`](extensions/MANAGER.md)). The application links
none of the three.

The CMake build names the two crates the application links - `cargo build
--release -p rustacity-dsp -p rustacity-ext-host` - and links both results as
static libraries, so a Rust toolchain is required to build Rustacity at all.
The workspace is also buildable and testable on its own:

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

## The Rust shell (in progress)

`rust/app/` is the iced shell that will replace the wxWidgets frontend. It is a
workspace of its own, excluded from `rust/`, and it is not part of the C++
application's build yet. The decisions are recorded in
[`docs/rust-app/ADR-0001-iced.md`](rust-app/ADR-0001-iced.md) and
[`docs/rust-app/ADR-0002-engine-first.md`](rust-app/ADR-0002-engine-first.md);
the parity matrix that decides when the C++ tree can be retired is
[`docs/rust-app/parity.md`](rust-app/parity.md). The C++ application remains the
shipping application until that matrix is met.

M1 is the first vertical slice: `rustacity-io` decodes WAV, builds the peak
pyramid and drives `cpal`; `rustacity-ui` owns the waveform canvas; and
`rustacity-app` owns loading, transport and the visible time range. The audio
backend and the codec stay behind `rustacity-io`, so the shell never links a
codec directly.

## The ABI

The C interface is `rust/dsp/include/rustacity_dsp.h`; the implementations are
in `rust/dsp/src/ffi.rs`. The extension host has a second one,
`rust/ext-host/include/rustacity_ext.h`, held to the same rule by
`scripts/check_ext_abi.py`. The rules that keep the DSP side safe:

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
- **The header and the exports must agree.** `scripts/check_dsp_abi.py` fails
  if they differ, so an entry point is added to both in the same commit.

The `rustacity_fft_*` entry points are the exception to the handle rule, and
deliberately so: a transform is arithmetic over buffers the caller already
owns, and the tables it runs on (the bit-reversal pointers and the sine values)
belong to the C++ pool `GetFFT` keeps, so they are passed in with each call
rather than held in a handle. The only state on the Rust side is the bit table
of the complex transform, which `rustacity_fft_deinit` releases at shutdown,
exactly as the C++ freed `gFFTBitTable`.

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
| `Rust Tests` | adds the `wasm32-wasip1` target (the extension tests build their own WebAssembly fixtures), then `cargo test --workspace` (unit tests, the golden-value tests and the extension engine's tests) and `python3 scripts/check_dsp_abi.py` |
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

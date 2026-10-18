<p align="center">
  <img src="images/rustacity.svg" alt="Rustacity" width="96" height="96">
</p>

# Rustacity

**Rustacity is a fork of Audacity 2.4.1 that adds a sandboxed WebAssembly extension system and is moving its audio processing into Rust, one unit at a time.**

[![CMake Build](https://github.com/prantosmss/Rustacity/actions/workflows/cmake_build.yml/badge.svg)](https://github.com/prantosmss/Rustacity/actions/workflows/cmake_build.yml)
[![License: GPL v2 or later](https://img.shields.io/badge/license-GPL--2.0--or--later-blue.svg)](LICENSE.txt)

## Status: early, and honest about it

The editor works — it is Audacity 2.4.1 — and the released builds contain the
Rust DSP core. The extension engine is built, tested and sandboxed, but **it is
not yet reachable from the editor's menus**: the C++ class that runs an
extension exists and is linked in, and nothing constructs one. Today the way to
run an extension is the command-line tool in this repository, which is what the
quick start below uses. Version 1 of the extension API is settled; nothing here
is 1.0, and interfaces may still change.

## Why this exists

Audio effects are the part of an editor most likely to come from somewhere else,
and a native plug-in is arbitrary code running in the process that holds your
unsaved work. The interesting question is what an effect is allowed to do, not
how fast it is. So extensions here are WebAssembly modules with a small versioned
ABI, a wall-clock deadline, a memory ceiling, and nothing to read: no files, no
network, no environment, no project. Alongside that, the signal-processing maths
is being moved out of the C++ tree into `rust/dsp/`, where a unit arrives with
tests and reference values and leaves the C++ it replaced deleted behind it.
Nothing about the existing plug-in formats changes: VST, LV2, AU, Nyquist and
the built-in effects load exactly as before.

## What works today

**Done, and tested**

- **The editor.** Audacity 2.4.1 as it was, rebranded, building and shipping on
  Windows x64, Linux x86_64 and macOS x86_64 (Intel).
- **The Rust DSP core**, linked into the released application: biquad design and
  processing, EBU R128 loudness, the bass and treble shelves, the phaser,
  wahwah, distortion, the FFT and window functions, and repair interpolation.
  Eight units, listed with their old C++ homes in [`docs/port-status.md`](docs/port-status.md).
- **The sandboxed extension engine**: manifests, the ABI, load-time validation,
  the deadline, the memory ceiling, the log — and containment tests for ten
  deliberate misbehaviours ([`docs/extensions/SANDBOX.md`](docs/extensions/SANDBOX.md) §2).
- **A reference extension**, `gain-invert`, checked end to end against
  arithmetic worked out outside the repository.
- **The extension manager**, `rustacity-ext-manager` with a window in
  `rustacity-ext-manager-gui`: it lists `<user data directory>/Extensions`, turns
  one on or off by moving it into that directory's `disabled/`, and asks the
  extension host — the one the editor links — whether an extension loads and runs.
  The editor does not read that directory yet
  ([`docs/extensions/MANAGER.md`](docs/extensions/MANAGER.md)).
- **168 tests** in `rust/`, run by `cargo test --workspace` and by CI on every
  push (see [`docs/EVIDENCE.md`](docs/EVIDENCE.md)).

**In progress**

- **Extension discovery and the editor UI.** The effect class, its CMake wiring
  and the C ABI are done; a discovery module, a menu entry per installed
  extension and a parameter dialog are not. This is the work that would make an
  extension usable without a terminal.
- **More of the DSP in Rust.** The FFT landing unblocked `NoiseReduction`,
  `Equalization` and `ClickRemoval`; the ledger is [`docs/port-status.md`](docs/port-status.md).

**Not started**

- Multi-channel extensions (one channel per call today), the `labels.write` and
  `tracks.read` capabilities, anything that persists state or sees the project,
  and an out-of-process host.
- arm64 macOS and 32-bit Windows builds; the triples have never been built.
- Code signing: the installers are unsigned, and Windows and macOS will warn.

## What it looks like

An extension that never returns is stopped at its deadline, and the shell it ran
in is still usable afterwards:

```
$ rust/target/release/rustacity-ext-cli rust/extensions/dist/hostile-loop \
      examples/sine-sweep.wav /tmp/loop.wav
rustacity-ext-cli: the extension timed out: the extension was still working after 5000 ms
$ echo $?
1
```

An extension that goes looking for files, environment, arguments and sockets
finds nothing, and its own log says so: `No such file or directory` for every path
it names, `0 arguments`, a refused `connect`, `obtained 0 bytes it should not
have`. That fixture's log, in full, is in
[`examples/README.md`](examples/README.md).

A parameter outside its range is refused rather than clamped — `invalid argument:
1000 is outside the range of 'gain_db' (-60..24)` — and the extension never sees
the call. The table of ten misbehaviours, and what happened to each, is
[`docs/extensions/SANDBOX.md`](docs/extensions/SANDBOX.md) §2.

## Quick start

Prefer a download? The [Releases page](https://github.com/prantosmss/Rustacity/releases)
has the Windows, macOS and Linux packages CI builds. They contain the Rust DSP
core but **cannot run an extension yet**; that needs the commands below.

This builds the extension engine and its command-line tool, and runs the
reference extension over a generated sample. No wxWidgets and no editor build.
Linux, macOS and Windows (Git Bash or WSL) all work, with three things
worth knowing first: Windows PowerShell needs `bash` for the extension build
script; Python 3 is needed for the two scripts in `examples/` — `python3` on
Linux and macOS, usually `python` on a Windows install; and on Windows rustc
needs a linker it can use, either the MSVC build tools or the GNU toolchain with
a MinGW linker on `PATH`, because the CLI's first build compiles
`wasmtime-internal-fiber`.

```sh
git clone https://github.com/prantosmss/Rustacity
cd Rustacity

rustup target add wasm32-wasip1          # the extensions are built for wasm
scripts/build-extensions.sh              # every example extension

cd rust
cargo build --release -p rustacity-ext-cli

# what the extension says it is, as a user would meet it
./target/release/rustacity-ext-cli --describe ../rust/extensions/dist/gain-invert

# run it at +6 dB, then check the result against arithmetic done outside it
python3 ../examples/make-sine-sweep.py ../examples/sine-sweep.wav
./target/release/rustacity-ext-cli ../rust/extensions/dist/gain-invert \
    ../examples/sine-sweep.wav /tmp/gained.wav --param gain_db=6 --verbose
python3 ../examples/check-output.py ../examples/sine-sweep.wav /tmp/gained.wav --gain-db 6
```

The last command prints:

```
input : 44100 frames, 1 ch, 44100 Hz, 16-bit, format tag 1
output: 44100 frames, 1 ch, 44100 Hz, 32-bit, format tag 3
expected: input x 1.995262315 (6.0 dB)
largest error: 6.399e-08 (tolerance 1e-06)
PASS
```

At 0 dB the output is bit-for-bit the input's values, and the polarity flip is
the same numbers negated; +6 dB is 6.4e-08 off, which is one float multiply's
rounding. Both of those runs, with their output, are in
[`examples/README.md`](examples/README.md).

## Writing an extension

An extension is a directory holding `extension.toml` and `extension.wasm`. The
manifest declares the parameters and what the extension asks for:

```toml
api_version = 1
id = "com.example.template"          # unique, reverse-DNS, never changes
name = "Template"                    # what the user sees
version = "0.1.0"
capabilities = ["audio.read", "audio.write"]

[[parameters]]
id = "amount"
name = "Amount"
kind = "float"
min = 0.0
max = 1.0
default = 0.0
```

and the module exports eight functions. `rx_process` is the extension; the audio
is planar, so channel `c`'s sample `i` is at `c * frames + i`, and `frames` is
not a constant:

```rust
#[no_mangle]
pub extern "C" fn rx_process(in_ptr: i32, out_ptr: i32, frames: i32) -> i32 {
    let frames = frames as usize;
    // SAFETY: the host allocated both buffers for the frame counts in rx_init.
    unsafe {
        let input = in_ptr as *const f32;
        let output = out_ptr as *mut f32;
        for channel in 0..state().channels {
            for index in 0..frames {
                let offset = channel * frames + index;
                *output.add(offset) = *input.add(offset);   // <- the work goes here
            }
        }
    }
    0
}
```

Start from [`rust/extensions/template`](rust/extensions/template) (that code is
its, with the TODOs marked), read the finished one in
[`rust/extensions/gain-invert`](rust/extensions/gain-invert), and follow
[`docs/extensions/WRITING_AN_EXTENSION.md`](docs/extensions/WRITING_AN_EXTENSION.md).
The ABI itself — every export, the manifest's rows, the error numbers — is
[`docs/extensions/API.md`](docs/extensions/API.md).

## The sandbox, and what it does not promise

Each extension runs in Wasmtime with one 256 MiB linear memory, a 5-second
wall-clock deadline per call, at most 65,536 frames at a time, a hollow WASI
preview 1 with no preopened directories, no environment, no arguments and closed
standard streams, and a single import: `rustacity.rx_log`. `random_get` is a
fixed generator and `clock_time_get` a counter, so the same extension on the
same audio gives the same result twice.

What it does **not** promise, plainly:

- **it is not a separate process**: the module runs in the editor's own address
  space, so a bug in Wasmtime or in the glue is a bug in the editor. Running
  extensions out of process is future work;
- **it does not judge audio**: an extension that produces the wrong gain is not
  misbehaving in a way the sandbox notices;
- **it does not make a slow extension fast** — the deadline bounds it at five
  seconds per call, no more;
- **the legacy plug-in formats are not sandboxed at all**: VST, LV2, AU and
  Nyquist load and run exactly as they did in Audacity 2.4.1, native code in
  the same process. This system does not change them;
- **capabilities are a declaration, not a prison**: the ABI hands an extension
  the audio buffers and nothing else, and an extension cannot reach past it.

[`docs/extensions/SANDBOX.md`](docs/extensions/SANDBOX.md) is the long form,
including the table of what each hostile fixture tried and what happened.

## Relationship to Audacity

Rustacity is an independent fork and is **not affiliated with, endorsed by, or
supported by the Audacity team**. Audacity, and the Audacity logo, are
trademarks of their respective owners; this repository uses neither as its
identity. Rustacity is based on Audacity 2.4.1, which is copyright © 1999-2018
the Audacity Team, and the upstream notices and credits are kept throughout the
tree — the About box lists them, and `LICENSE.txt` is upstream's, unchanged.

The fork starts from 2.4.1 rather than a 3.x release. `TODO(owner)`: the reason
for that choice is not written down in the repository and is the maintainer's to
state. Whatever the reason, the intent is to move DSP units *into Rust* rather
than to track upstream's C++ changes, and there is no route in this repository
for bringing 3.x features over; `TODO(owner)` for how newer upstream work is
meant to reach this fork.

Upstream's own documentation still describes this application, because it is
Audacity 2.4.1 underneath: the [manual](https://manual.audacityteam.org/) and
the [wiki](https://wiki.audacityteam.org/) are the references for the editor's
features.

## Building the full application

The application needs a C++ toolchain, CMake, a Rust toolchain (the CMake build
compiles the Rust cores and links them) and wxWidgets from the pinned fork
(`audacity/wxWidgets`, branch `audacity-fixes-3.1.3`). There has been no full
local build of the application: the machine this fork was developed on has
neither wxWidgets nor MSVC. What has built it, on every commit on `main`, is CI.

The canonical recipe is [`.github/workflows/cmake_build.yml`](.github/workflows/cmake_build.yml):
what it runs is what produces the releases. It configures with CMake and builds
the `Release` configuration; Windows uses the Visual Studio 17 2022 generator
with `-A x64`, macOS the Xcode generator with code signing off. The wxWidgets
build and the packaging steps (a 7z, an Inno Setup installer and a WiX MSI on
Windows, a dmg on macOS, an AppImage on Linux) are all in that file.

## Roadmap

- Bring the extension engine into the editor: discovery, a menu entry per
  installed extension, and a parameter dialog generated from the manifest.
- Port more DSP units, each with captured reference values and the C++
  deleted behind it (the rule is in [`docs/architecture.md`](docs/architecture.md)).
- Multi-channel extensions, then the capabilities the ABI deliberately left out
  (`labels.write`, `tracks.read`).
- Run extensions out of process, so a sandbox failure costs a child process
  rather than the editor.

No dates: this is one maintainer's work in the open, and the ledger in
[`docs/port-status.md`](docs/port-status.md) is the honest measure of progress.

## Documentation

| | |
| --- | --- |
| [Repository layout](docs/repository-layout.md) | what each top-level directory is, what is ours, what is vendored, and what was removed |
| [Architecture](docs/architecture.md) | the C++ shell and the Rust core, the C ABI, the panic policy, and how a unit is ported |
| [Port status](docs/port-status.md) | which DSP units are Rust, which are not yet, and why |
| [Golden values](docs/goldens.md) | how each port is checked against the implementation it replaced |
| [Evidence](docs/EVIDENCE.md) | what backs every claim on this page, and how to reproduce it |
| [Extensions](docs/extensions/API.md) | the API contract, the sandbox, how to write one, and what is not built yet |
| [Integration notes](docs/extensions/INTEGRATION_NOTES.md) | what is built and tested, what is not, and every deviation from the original plan |
| [Roadmap](docs/roadmap.md) | where the Rust migration and the engine are going |
| [Audit](docs/HN_AUDIT.md) | a candid pre-publicity audit of this repository's state |
| [Examples](examples/README.md) | the sample, the checker, and real command output |

## Contributing

[`CONTRIBUTING.md`](CONTRIBUTING.md) has the rules: Rust is where audio
processing goes, `lib-src/` is vendored and untouched, renames are out of scope,
one idea per commit, and a port is not finished until the C++ implementation it
replaced is deleted. Bug reports and feature requests have
[templates](.github/ISSUE_TEMPLATE); security problems have a policy of their
own in [`SECURITY.md`](SECURITY.md).

## License

GPL v2 or later, as Audacity is: [LICENSE.txt](LICENSE.txt) is upstream's
licence text, and [`NOTICE`](NOTICE) records the fork and its changes.
Contributions are accepted under the same terms.

## Acknowledgements

Rustacity is a fork of Audacity, and would not exist without the Audacity team
and everyone who has contributed to it since 1999. The extension engine runs on
[Wasmtime](https://wasmtime.dev/) (Apache-2.0/MIT), and the DSP core exists to
keep the loudness, filter and transform maths faithful to the implementations in
Audacity 2.4.1. The vendored libraries in `lib-src/` carry their own licences,
all GPL-compatible.

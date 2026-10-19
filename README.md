<div align="center">

<img src="images/rustacity.svg" alt="Rustacity" width="96" height="96">

# Rustacity

**An Audacity 2.4.1 fork with a Rust audio core and a sandboxed WebAssembly extension engine.**

[![Build](https://github.com/prantosmss/Rustacity/actions/workflows/cmake_build.yml/badge.svg)](https://github.com/prantosmss/Rustacity/actions/workflows/cmake_build.yml)
[![License: GPL v2](https://img.shields.io/badge/license-GPL--2.0-blue.svg)](LICENSE.txt)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#building-from-source)

[Why](#why-rustacity) · [Status](#status) · [How it fits together](#how-it-fits-together) · [Extensions](#extensions) · [Build](#building-from-source) · [Docs](#documentation) · [Contribute](#contributing)

</div>

---

Rustacity is the Audacity editor you already know (multi-track, sample-accurate, scriptable) with two additions underneath it:

1. **New audio code is written in Rust.** DSP moves out of legacy C++ and into the `rustacity-dsp` crate, behind a small C interface, and every ported routine is checked against reference values captured from the original C++.
2. **Extensions run in a sandbox.** A WebAssembly host (`rust/ext-host/`) lets people add functionality that standard Audacity does not have, without handing third-party code the keys to your machine.

Everything else stays Audacity 2.4.1, plug-in support included.

## Why Rustacity

- **Add things Audacity does not do.** The extension engine is the way in. Extensions receive audio blocks and can log, and nothing else: no filesystem, no network, no environment access. A crash or fault inside an extension stops that one call, not the editor.
- **Memory-safe audio code.** New signal processing lands in Rust, where it is tested in isolation with `cargo test` and does not need a C++ toolchain to work on.
- **A careful port.** Ported code is checked against values from the original, and a port is not finished until the old C++ is deleted. Progress is tracked in `docs/port-status.md`.
- **Nothing you rely on is taken away.** The existing plug-in system (VST, LV2, AU, Nyquist) is left intact. The extension engine is built alongside it, not as a replacement.

## Status

Rustacity is early and under active development. Here is where things stand.

| Area | State |
|---|---|
| Audio editor (recording, editing, import/export, effects) | Audacity 2.4.1 |
| Rust DSP core (`rust/dsp/`) | In progress. Parts of the audio processing are in Rust and checked against reference values from the original C++ |
| Extension host (`rust/ext-host/`) | Runs sandboxed WebAssembly extensions |
| Editor integration for extensions | **Not built yet.** Installing extensions, menu entries and parameter dialogs are still to come |
| Prebuilt packages | Produced by the release workflow when a version is tagged. See [Releases](https://github.com/prantosmss/Rustacity/releases); otherwise [build from source](#building-from-source) |

The application still reports the upstream 2.4.x version string. Rustacity releases are numbered separately (for example `rustacity-0.4.0`).

For exactly which routines have moved to Rust, see the port-status document in [Documentation](#documentation).

## How it fits together

```
 Audacity 2.4.1 editor (C++, src/)
          │
          │  C interface:  rust/dsp/include/rustacity_dsp.h
          ▼
 rustacity-dsp  (Rust static library, rust/dsp/)
   ported DSP, checked against goldens/reference.txt


 Extension host (Rust, rust/ext-host/)
   loads  extension.toml + extension.wasm
   gives each extension audio blocks and one logging call
   no filesystem · no network · no environment
```

CMake runs `cargo build --release` during the normal build and links the Rust crate into the application as a static library.

## Features inherited from Audacity 2.4.1

- Recording from real or virtual audio devices
- Import and export of many audio formats, extendable with FFmpeg
- 32-bit float audio processing
- Plug-in support for VST, LV2 and AU
- Macros for chaining commands and batch processing
- Scripting in Python, Perl, or any language that can use named pipes
- Nyquist, a built-in scripting language that can also be used to build plug-ins
- Multi-track editing with sample accuracy and arbitrary sample rates
- Accessibility features for visually impaired users
- Analysis and visualization tools for audio and other signal data

## Extensions

An extension is a folder with two files:

| File | Purpose |
|---|---|
| `extension.toml` | The manifest |
| `extension.wasm` | The module, exporting the eight functions the host expects |

What the host gives an extension: audio blocks to process, and one logging import. What it does not give: filesystem, network or environment access. If an extension fails, only that call stops.

The host exists and runs extensions today. Wiring it into the editor (installing extensions, menu entries, parameter dialogs) is the next piece of work, so for now extensions are a developer-facing feature. Start in `rust/ext-host/`, and see the extensions entry in [Documentation](#documentation).

## Building from source

Rustacity builds with CMake. You need:

- a C++ toolchain
- CMake
- a Rust toolchain (CMake calls `cargo` for you)
- the pinned wxWidgets fork: [`audacity/wxWidgets`](https://github.com/audacity/wxWidgets), branch `audacity-fixes-3.1.3`

The per-platform recipes that CI uses are the canonical build instructions: [`.github/workflows/cmake_build.yml`](.github/workflows/cmake_build.yml). Follow the job for your operating system.

### Working on the Rust code only

You do not need CMake or a C++ compiler to work on the Rust side:

```bash
cd rust
cargo test --workspace
```

Before you push, also run the ABI check, which confirms the C header and the Rust exports agree:

```bash
python3 scripts/check_dsp_abi.py
```

CI captures golden values and builds the whole application on four platforms. A run takes about twenty minutes.

## Documentation

| Topic | Where to look |
|---|---|
| What each directory is for | [`docs/repository-layout.md`](docs/repository-layout.md) |
| Architecture and the C ABI rules | [`docs/architecture.md`](docs/architecture.md) |
| What has been ported to Rust | [`docs/port-status.md`](docs/port-status.md) |
| How reference ("golden") values are captured | [`docs/goldens.md`](docs/goldens.md) |
| Writing extensions | `rust/ext-host/` <!-- link your extensions API doc here --> |
| Roadmap | [`docs/roadmap.md`](docs/roadmap.md) |

Using the editor itself works as it does in Audacity: see the [Audacity website](https://www.audacityteam.org/), [forum](https://forum.audacityteam.org/) and [developer wiki](https://wiki.audacityteam.org/wiki/For_Developers).

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) first. The short version:

- **New DSP is written in Rust**, in `rust/dsp/`. The C++ in `src/` is legacy and is not extended.
- **A port is not done until the C++ is deleted.** Capture reference values while the original still exists, and say in the commit message if behavior intentionally changes.
- **Do not edit `lib-src/`.** It is vendored third-party code.
- **No tree-wide renames.** That is not what this fork is doing.
- **Check [`docs/port-status.md`](docs/port-status.md) and [`docs/roadmap.md`](docs/roadmap.md)** before starting, to avoid duplicate work.
- **Keep commits small:** one idea each, in the imperative mood.
- **Zero compiler warnings.**
- Contributions must be GPL v2 compatible, and per-file license headers stay.

Bug reports should include the steps, what you expected, what happened, the version from **Help > About**, and your platform. For security issues, use **Report a vulnerability** on the Security tab rather than a public issue.

## Releasing

Releases are cut from `main` with an annotated tag named by release number, for example `rustacity-0.4.0`. Pushing the tag starts the **Publish Release** job, which waits for the build matrix and the Rust tests, then attaches:

- a Windows archive and installers
- a macOS disk image
- a Linux AppImage
- a source tarball

## License

Rustacity is a fork of Audacity 2.4.1 and is distributed under the terms of the [GNU General Public License, version 2](LICENSE.txt), as Audacity is.

- Audacity® is copyright © 1999–2016 Audacity Team. Its notices are kept throughout this tree.
- Changes made for the fork are copyright © 2026 Sadik Pranto and are licensed under the same terms.
- Documentation is under CC BY 3.0, and code in `lib-src/` may be under a different GPL-compatible license. See `LICENSE.txt` for the details.

Rustacity is an independent fork and is not affiliated with or endorsed by the Audacity Team. Audacity® is a registered trademark of its owner.

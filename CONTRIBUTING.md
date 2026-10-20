# Contributing

Rustacity is a fork of Audacity 2.4.1, and it lives at
<https://github.com/seven3rs/Rustacity>. Issues and pull requests here are
the place to report a problem or propose a change. The [upstream wiki][wiki]
and [forum][forum] describe upstream Audacity: they remain a useful reference
for the application this forked from, but they cannot help with Rustacity
itself, and they are not our support desk.

## Before you start

* **Rust is where audio processing goes.** The C++ tree in `src/` is the
  legacy shell, and it is not extended: new DSP is written in `rust/dsp/`.
  [docs/architecture.md](docs/architecture.md) has the C ABI rules, the panic
  policy and the porting rule; read it before writing DSP.
* **`lib-src/` is vendored third-party code.** It is never edited to make our
  code fit and never restructured.
* **Renames are out of scope.** Files, classes and functions keep the names
  they have; a patch that renames things across the tree is not what this fork
  is doing.
* **What is ported** is in [docs/port-status.md](docs/port-status.md), and what
  is next is in [docs/roadmap.md](docs/roadmap.md), so that a first
  contribution does not duplicate work in flight.

## Building and testing

Building the application needs a C++ toolchain, CMake, a Rust toolchain (the
CMake build runs `cargo build --release` and links the result) and wxWidgets
from the pinned fork. [README.md](README.md) has the short version and
[`.github/workflows/cmake_build.yml](.github/workflows/cmake_build.yml) is the
canonical per-platform recipe - what CI runs is what produces the releases.

Work inside the Rust crate needs neither CMake nor a C++ compiler:

```sh
cd rust
cargo test --workspace
```

Before pushing, run the checks CI runs first:

```sh
cargo test --workspace             # unit tests and the golden-value tests
python3 scripts/check_dsp_abi.py   # header and exports must agree
python3 scripts/check_ext_abi.py   # extension header and FFI must agree
```

CI then captures golden values, builds the extension crates and every example
extension for `wasm32-wasip1`, and builds the whole application on three
platforms; expect a run to take about twenty minutes.

## Making a change

* Small commits, one area each, written in the imperative mood. The
  `docs:`/`fix:`/`chore:` prefixes in the log are only a convention; the
  size of a commit is not, because a reviewable commit is one idea.
* Zero compiler warnings is the bar. `rust/dsp` is not held to clippy or to
  rustfmt — it carries lints from before the extension work — so match the code
  around you by hand there. The extension crates are held to both in CI:
  `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` over
  `rustacity-ext-api`, `rustacity-ext-host` and `rustacity-ext-cli`, and the
  manager crates, `rustacity-ext-manager` and `rustacity-ext-manager-gui`, are
  held to both as well, and the same two gates over `rust/extensions` for
  `wasm32-wasip1`.
* **A port is not finished until the C++ implementation is deleted.** Capture
  the reference values while the C++ still exists - the loop is in
  [docs/goldens.md](docs/goldens.md) - and say so in the commit message when a
  port knowingly changes behaviour or drops code that was dead.
* Update the documentation a change makes wrong. `docs/` is indexed from the
  README, and `docs/repository-layout.md` says what each directory is for.
* Contributions must be GPL v2 compatible. Keep the per-file licence headers,
  and by opening a pull request you agree to license your work under the same
  terms as the rest of the project ([LICENSE.txt](LICENSE.txt)).

## Writing an extension

An extension is a directory of its own, so it needs neither this tree nor a C++
compiler:

```sh
rustup target add wasm32-wasip1
cp -r rust/extensions/template my-effect
# write my-effect/extension.toml and my-effect/src/lib.rs
cargo build --release --target wasm32-wasip1 --manifest-path my-effect/Cargo.toml
```

[docs/extensions/WRITING_AN_EXTENSION.md](docs/extensions/WRITING_AN_EXTENSION.md)
walks through it, [docs/extensions/API.md](docs/extensions/API.md) is the
contract, and `rust/extensions/gain-invert` is a finished one to read. To have
an extension built and tested by this tree, add a line to
`rust/extensions/variants.txt`: the build script and the host's own test
fixtures both read that file, so they cannot disagree about what a variant is.

## Reporting a bug

Open an issue — the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md)
asks for what is needed — with what you did, what happened, what you expected
instead, the version from *Help > About*, and your platform. A minimal project
file or a short recording is worth more than a description. For a security
problem, [SECURITY.md](SECURITY.md) says how to report it privately rather than
in a public issue.

Unacceptable behaviour is a different matter, and is covered by
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

[wiki]: https://wiki.audacityteam.org/wiki/Contribute
[forum]: https://forum.audacityteam.org/

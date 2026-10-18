# Integration notes

What the extension engine is, where it sits in the tree, what it deviates from
as it was first sketched, what has been built and tested, and what has not. The
contracts themselves are [API.md](API.md), [SANDBOX.md](SANDBOX.md) and
[WRITING_AN_EXTENSION.md](WRITING_AN_EXTENSION.md); this is the honest note
about the seams.

## 1. Where it lives

```
rust/ext-api/           manifests, the error type, the ABI's names and limits
                        (no Wasmtime; the CLI, the host and any tool may use it)
rust/ext-host/          the sandbox: Wasmtime, the deadline, the memory ceiling,
                        the log, and the C ABI
  include/rustacity_ext.h   hand-written C header, checked by scripts/check_ext_abi.py
rust/ext-cli/           rustacity-ext-cli: one extension over one WAV file
rust/extensions/        a workspace of its own (excluded from rust/Cargo.toml)
  gain-invert/          the reference extension
  hostile/              ten deliberate misbehaviours, one cargo feature each
  template/             a starter to copy
  variants.txt          which module comes from which crate and feature
scripts/build-extensions.sh   builds every variant for wasm32-wasip1
scripts/check_ext_abi.py      fails when the header and ffi.rs disagree
docs/extensions/              the documents named above
src/effects/Extension.{h,cpp} the one C++ effect (see section 4)
```

An extension is a directory with `extension.toml` and `extension.wasm`. The
existing plug-in formats are untouched: VST, LV2, AU, Nyquist and the built-in
effects load exactly as before, and nothing here changes their behaviour.

`rust/extensions` is a workspace of its own on purpose. The cargo build inside
the CMake build must never build wasm crates, so that a compiler upgrade for
wasm cannot break the editor, and so that an extension cannot accidentally be
compiled into a platform build that cannot run it.

## 2. Decisions and deviations

Each of these is a place where the implementation chose something the first
sketch did not say, or where a sketch item was dropped.

**No `rx_flush`.** The sketch listed one; nothing in the host ever wanted it,
and a module that exports one is simply never asked. The export table in
API.md no longer mentions it.

**A probe instance at load.** Loading an extension compiles the module, checks
its imports, and then builds one throwaway instance at one channel and one
frame, which is what makes a missing export, an `rx_alloc` that returns a
pointer outside memory, and an `rx_api_version` of 2 fail at load. The sketch
had these checks spread over the first real run; a user should meet them when
they pick the extension, not halfway through a track.

**WASI is linked and hollow.** Every preview 1 function a `std`-built module
imports exists, but the context has no preopens, no environment, no arguments
and closed standard streams, and the two nondeterministic functions are ours:
`clock_time_get` is a counter and `random_get` a fixed generator. Linking our
own versions of WASI functions needs `Linker::allow_shadowing(true)`, which is
worth knowing before someone "cleans it up". A module that imports anything
outside WASI and `rustacity.rx_log` fails to load.

**One hostile crate, ten features.** The sketch said one extension per hostile
behaviour; it is one crate with a cargo feature each, and `variants.txt` is the
single list that both the build script and the test fixtures read, so a new
variant is added in one place and cannot be built by one and forgotten by the
other.

**The CLI is its own crate.** It reads WAV files, which needs `hound`; keeping
it a separate crate keeps `hound` out of the static library the editor links.
The editor's build names the two crates it wants (`-p rustacity-dsp -p
rustacity-ext-host`) for the same reason.

**Lint and format gates are scoped to the new crates.** `rust/dsp` has clippy
lints from before this work (`int_plus_one` and friends under the clippy of the
day), and fixing them is not part of it; the repository's rule has been zero
*compiler* warnings, and CI ran `cargo test --workspace` and nothing else. So
the new Extensions job holds `rustacity-ext-api`, `rustacity-ext-host` and
`rustacity-ext-cli` to `clippy --all-targets -- -D warnings` and `cargo fmt
--check`, and separately holds the extension crates themselves to the same two
gates for `--target wasm32-wasip1` (they are not in the main workspace, so no
other job would).

**Two changes to jobs that already existed**, both disclosed because the
instruction was not to break them:

- the `rust-test` job now adds the `wasm32-wasip1` target, because the
  extension tests build their own WebAssembly fixtures before they run;
- the `release` job now also waits on the new `extensions` job, so a tag does
  not publish past a broken extension engine.

**Statuses are published, not invented.** `ExtError`'s variants each have a
fixed number, `RUSTACITY_EXT_ERROR_*` in the header repeats them, and
`scripts/check_ext_abi.py` fails the build when the two lists drift. `Internal`
is 1; `AbiMismatch` (4) is what an out-of-range pointer from `rx_alloc` becomes,
found at load; a parameter outside its manifest range is refused with
`InvalidArgument` and never clamped; and only `Internal`, `Trap`, `Timeout` and
`OutOfMemory` spend an instance - an extension that returns a code of its own,
or a caller who asked for something impossible, leaves the instance usable.

## 3. What is built and tested

- `rust/ext-api`, `rust/ext-host`, `rust/ext-cli`: unit and integration tests
  in the main workspace, `cargo test --workspace`. The host's tests load real
  modules built for `wasm32-wasip1` and include the containment tests for all
  ten hostile behaviours, from the five-second deadline to the memory ceiling
  to a flood of log messages.
- `rust/extensions/*`: built warning-free for `wasm32-wasip1`;
  `cargo clippy --workspace --all-targets --target wasm32-wasip1 -- -D warnings`
  and `cargo fmt --all --check` are clean.
- `scripts/check_ext_abi.py`: passes, and was checked against deliberate drift
  (a renamed declaration, a renumbered status, a wrong API version) to be sure
  it fails when it should.
- gain-invert end to end on a real WAV, through both the host's tests and the
  CLI, against references worked out in the tests at 1e-6, at several channel
  counts and at block sizes of 1, 64, 4096 and 65536 frames.

No benchmark numbers are quoted anywhere in this work, because none were
measured under conditions worth quoting.

## 4. What has not been built or tested

**The C++ effect has never been compiled.** This machine has no wxWidgets, no
MSVC and no CMake build tree, so `src/effects/Extension.cpp` and `.h` and the
`src/CMakeLists.txt` change are written to the repository's conventions and
unverified in every way a compiler verifies. Specifically unverified:

- that the class compiles against this version of `Effect` (it follows
  `Invert.cpp` and `Reverse.cpp`, which is the closest thing to a check
  available here);
- the CMake pattern of one custom command with two outputs behind two custom
  targets, and the claim that both static libraries end up in the app;
- the library names for the MSVC and Apple triples, which are set by pattern
  from the DSP core's names;
- that the MSVC system library list that was enough for the DSP core
  (`bcrypt userenv ws2_32 ntdll dbghelp psapi`) is enough for Wasmtime. If it
  is not, the linker says which one is missing, and it is a one-line change;
- whether a static Wasmtime links into the application at all on Windows and
  macOS. This is the largest unknown in the whole change.

What *was* checked about the build glue is the cargo command inside it: run by
hand for the GNU Windows triple, with the same `--release --target <triple>
--manifest-path rust/Cargo.toml -p rustacity-dsp -p rustacity-ext-host`, it
produces both `librustacity_dsp.a` and `librustacity_ext.a` in the
`target/<triple>/release` directory the CMake block points at, and nothing
else - in particular not `rustacity-ext-cli`. Everything around that one line
is still CMake that has never run.

**Nothing about other platforms.** The extension engine has been exercised only
through the GNU Windows toolchain here; the Linux CI job is new and unobserved.
arm64, macOS and MSVC builds of the engine are unverified, and the CMake block
sets a target triple for each of them by pattern rather than by measurement.

**The effect is not reachable from a menu.** `EffectExtension` loads an
extension and runs it; nothing constructs one yet. Registering a menu entry per
installed extension means discovering what is installed - the way the LADSPA,
LV2 and Nyquist modules discover what they find - and that needs two
user-visible decisions rather than code: where extensions are installed, and how
a user turns one on and off. `EffectExtension::Load` is written to be the half a
discovery module would call.

**Parameters are not exposed in the editor.** The effect runs the extension at
the defaults its manifest declares. The host's C ABI already describes
parameters and sets them by id, so this is dialog work on the C++ side, not an
engine gap.

**One channel at a time.** The effect asks the framework for one audio channel,
which is what makes it work on any track, but it means an extension that needs
all channels together cannot be run yet. That is a decision in `GetAudioInCount`
and a change to how the tracks are walked, not to the ABI, which takes planar
audio for whatever channel count it is told.

**Signing.** Installer signing needs certificates that are not in this
repository; it stays deferred to a later release, as it was before this work.

## 5. Running it

```
# the whole Rust workspace, engine and CLI included
cd rust && cargo test --workspace

# the extension crates' own build, lint and format gates
rustup target add wasm32-wasip1
scripts/build-extensions.sh
cd rust/extensions && cargo clippy --workspace --all-targets \
   --target wasm32-wasip1 -- -D warnings && cargo fmt --all --check

# the header and the FFI
python3 scripts/check_ext_abi.py

# one extension, from the outside
rust/target/release/rustacity-ext-cli --describe rust/extensions/dist/gain-invert
rust/target/release/rustacity-ext-cli rust/extensions/dist/gain-invert \
   in.wav out.wav --param gain_db=6 --verbose
```

The last two need `cargo build --release -p rustacity-ext-cli` (or
`cargo run -p rustacity-ext-cli --`).

## 6. Toolchain notes

Building and testing this needed two additions to the local Rust toolchain,
which are properties of the machine rather than of the repository:

- `rustup target add wasm32-wasip1`, for the GNU toolchain used here;
- `rustup component add clippy` for the same toolchain (`rustfmt` was already
  installed for the MSVC toolchain, so `cargo fmt` was run with that one).

On Windows with the GNU toolchain, `wasmtime` needs a MinGW linker on `PATH`
for `wasmtime-internal-fiber`; that is what the `F:\Scoop\apps\mingw` prefix in
the build commands was for. None of this is recorded in the repository, and
none of it is needed for the MSVC build CI uses.

## 7. Files touched outside `rust/`

```
.github/workflows/cmake_build.yml   the Extensions job, and the two disclosed
                                    changes to rust-test and release
README.md                           the front page, and the documentation table
docs/architecture.md                the Rust core section, the ABI section and
                                    the CI table
docs/repository-layout.md           what rust/ is, and the language figures
docs/roadmap.md                     the extension engine's section
docs/extensions/API.md              the contract, brought in line with the engine
docs/extensions/SANDBOX.md          what the sandbox is and is not
docs/extensions/WRITING_AN_EXTENSION.md   how to write one
docs/extensions/INTEGRATION_NOTES.md      this file
scripts/build-extensions.sh         builds the variants for wasm32-wasip1
scripts/check_ext_abi.py            header against FFI, and the status numbers
src/CMakeLists.txt                  one include path, one static library, one
                                    cargo build that produces both
src/effects/Extension.h             the one new effect class (unbuilt)
src/effects/Extension.cpp
```

Nothing else in `src/`, `libraries/`, `lib-src/`, `plug-ins/` or `nyquist/` is
touched, and no existing file, class or function was renamed. Inside `rust/`,
`rust/README.md` was brought up to date as well.

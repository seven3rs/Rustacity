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
  counts and at block sizes of 1, 64, 4096 and 65536 frames. It was also checked
  once by hand, against a reference computed outside the repository - the input
  WAV and the expected samples built by a separate script from the manifest's
  own words - where 0 dB reproduced the input (1.5e-08), +6 dB matched
  10^(6/20) after the slew (8.3e-08), the polarity flip matched to the sign, and
  a 16-bit input came back as the same samples under one scale factor.

Those tests now run in CI on Linux too, on a platform other than the one this
was written on: the `Rust Tests` job installs the `wasm32-wasip1` target and
passes `cargo test --workspace`, which builds the WebAssembly fixtures the
containment tests load, and the `Extensions` job builds every variant in
`variants.txt`, checks the header against the FFI, and applies the lint and
format gates to the new crates. Both jobs pass, and so do the three platform
builds of the application itself; what that took is section 4.

No benchmark numbers are quoted anywhere in this work, because none were
measured under conditions worth quoting.

## 4. What has been built, and what that took

This is the part of the work that could not be checked here at all - this
machine has no wxWidgets, no MSVC and no CMake build tree - so it is worth
saying exactly what CI found, because it found three things, and every one of
them was real:

- **`Extension.cpp` had never been compiled.** Written to the shape of
  `Invert.cpp` and `Reverse.cpp`, it had six errors in two groups, reported
  identically by MSVC and by GCC: `Effect` keeps its block size privately, and
  `XO(...).Format(...)` is a `TranslatableString` rather than a `wxString`.
  `SetBlockSize` now hands the smaller size to the base class - which is exactly
  what `Effect::ProcessPass` calls at `Effect.cpp:1451` - and the instance is
  told `GetBlockSize()`; `Fail` takes the type `MessageBox` wants. Nothing else
  was reported, so the class shape, the overrides and every call into the C ABI
  type-check against this `Effect`.
- **The link failed on Linux and only on Linux**, with one undefined symbol:
  `EffectExtension::MaximumBlockFrames`. The repository builds as C++14, where a
  `static constexpr` data member is a declaration until something binds a
  reference to it, and `std::min` takes its argument by reference. MSVC does not
  insist on the definition and GCC does; there is now a definition.
- **The CMake was wrong in two ways that only two generators could see.** A
  custom command with two outputs is two rules, and so two cargo runs, to the
  Visual Studio generator - the first Windows build ran cargo twice, 5m51s and
  then 1m06s - and the Xcode generator refuses a custom command attached to more
  than one target, so the macOS job never reached a compiler: it failed while
  generating the build files, and said so plainly. The sandbox's library is now
  a byproduct of the single command, and one custom target drives it, which is
  the arrangement CMake's own documentation prescribes for a command that more
  than one target needs. The next Windows build asked cargo once.

After those three fixes every job passes, on every platform: the application
compiles, links and packages with the sandbox inside it under MSVC, GCC and
Xcode; `cargo test --workspace` passes on Linux with the containment tests in
it; and the `Extensions` job builds every variant and applies the lint and
format gates. That the wiring works, that the library names are right for these
three triples, that the DSP core's MSVC system libraries (`bcrypt userenv
ws2_32 ntdll dbghelp psapi`) are enough for Wasmtime too, and that a static
Wasmtime links into the application at all are therefore observed rather than
assumed.

The cargo command was also checked by hand here, for the GNU Windows triple,
with the same `--release --target <triple> --manifest-path rust/Cargo.toml
-p rustacity-dsp -p rustacity-ext-host`: it produces both libraries in the
`target/<triple>/release` directory the CMake block points at, and nothing
else - in particular not `rustacity-ext-cli`.

What is still unverified is not the building of it but the running of it:

- **no instance of the effect has ever been created inside the application.**
  Nothing constructs an `EffectExtension` - it is deliberately in no menu, for
  the reason below - so the effect has only ever run through the host's tests
  and the CLI. How the framework treats a failed pass in a real session - the
  message box, the discarded output - follows from `Effect::Process` and is
  untested in place;
- arm64 macOS and 32-bit Windows: their triples are set by pattern from the DSP
  core's, no library name has been seen for either, and no build has been
  attempted anywhere.

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
for `wasmtime-internal-fiber`. None of this is recorded in the repository, and
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
src/CMakeLists.txt                  two include paths, two static libraries,
                                    one cargo build, one driving target
src/effects/Extension.h             the one new effect class, compiled, linked
                                    and packaged for all three platforms
src/effects/Extension.cpp
```

Nothing else in `src/`, `libraries/`, `lib-src/`, `plug-ins/` or `nyquist/` is
touched, and no existing file, class or function was renamed. Inside `rust/`,
`rust/README.md` was brought up to date as well.

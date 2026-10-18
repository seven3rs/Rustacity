# Audit: state of the repository before the front page is rewritten

This is an audit of `prantosmss/Rustacity` as it stood at commit `ee408e94d3`
on `main`, carried out before any change in this branch, so that the front page
can be written from evidence rather than from intention. Everything below was
observed here; commands that were run are quoted, and anything that was not
verified says so.

Findings are reported with the file that holds them. The three sections after
"provenance" become the raw material for `README.md`; the hygiene findings are
worked through in the commits on this branch, and the ones that need a human go
to [`HN_TODO_FOR_OWNER.md`](HN_TODO_FOR_OWNER.md).

## 1. What the project actually is

**A fork of Audacity 2.4.1.** `Audacity-2.4.1` is an ancestor of the current
tree (`git merge-base --is-ancestor Audacity-2.4.1 HEAD` exits 0), so the fork
point is exact: **130 commits** have been made since it, touching 1347 files
(`+42,238 / -194,308`).

**The application is the upstream C++ tree.** `src/` is 793 C++ files and about
297,000 lines, and it owns the wxWidgets interface, the project and track model,
the file formats and every effect dialog. It is not being rewritten; the maths
is being moved out of it, one unit at a time (`docs/architecture.md`).

**A small Rust workspace sits under it**, 60 files and 16,788 lines:

| crate | what it is | in the app build |
| --- | --- | --- |
| `rust/dsp` (`rustacity-dsp`) | the audio maths; the only crate with no dependencies | yes, linked as a static library |
| `rust/ext-api` (`rustacity-ext-api`) | the extension manifest and error types, the ABI's names and limits; no Wasmtime | yes, via the host |
| `rust/ext-host` (`rustacity-ext-host`) | the sandbox: Wasmtime, the deadline, the memory ceiling, the log, and the C ABI in `include/rustacity_ext.h` | yes, linked as a static library |
| `rust/ext-cli` (`rustacity-ext-cli`) | runs one extension over one WAV file, without the editor | **no**, deliberately: it pulls in `hound` |
| `rust/extensions/*` | the example, hostile and template extensions | **no**: a separate workspace, built for `wasm32-wasip1` only |

**The DSP core is genuinely wired in.** `src/CMakeLists.txt` runs `cargo build
--release -p rustacity-dsp -p rustacity-ext-host` and links both results, and the
Linux job checks that a Rust symbol is present in the linked binary
(`nm ... | grep rustacity_biquad_design_butterworth`), so an unreferenced archive
member cannot pass unnoticed. Eight units have moved, per
`docs/port-status.md`: biquad design and processing, EBU R128 loudness, the bass
and treble shelves, the phaser, wahwah, distortion, the FFT and window
functions, and repair interpolation.

**The extension engine works and is tested, but only from outside the editor.**
The engine compiles and runs a module, checks its imports and exports, probes it
at load, and contains ten deliberate misbehaviours
(`docs/extensions/SANDBOX.md` §2 has the table of each one and what happened).
What is **not** built is everything on the editor side of it:

- no discovery, and no menu entry per extension: `src/effects/Extension.{h,cpp}`
  can load and run one, but **nothing constructs it**, so no extension can be
  run from the GUI today;
- no parameter dialog: the effect runs an extension at the defaults its manifest
  declares;
- one audio channel per call, so an extension that needs all channels at once
  cannot be run yet;
- no `labels.write` or `tracks.read` capability, and no persistence, network or
  project access of any kind for an extension;
- no out-of-process host: a bug in Wasmtime or in the host's own glue is a bug
  in the editor (`docs/extensions/SANDBOX.md` §3).

**The test suite, run here and passing.** `cd rust && cargo test --workspace`
exits 0 with **144 tests passed, 0 failed**:

| suite | tests |
| --- | --- |
| `rustacity-dsp` unit tests | 53 |
| DSP golden-value tests (`bass_treble`, `biquad`, `distortion`, `ebur128`, `fft`, `interpolate_audio`, `phaser`, `wahwah`) | 23 |
| `rustacity-ext-api` (unit + `manifest`) | 26 |
| `rustacity-ext-host` (unit, `abi_and_manifest`, `block_sizes`, `gain_invert`, `hostile`) | 35 |
| `rustacity-ext-cli` (`wav_round_trip`) | 7 |

`rust/extensions/` is a separate workspace and is **not** part of that run: the
extensions are checked by the `Extensions` CI job (they build, lint and format
clean for `wasm32-wasip1`), and the host's own tests build the fixtures they
load whether or not the script has been run.

## 2. Build reality

### The Rust workspace — verified here

Prerequisites, in the order they were needed on this machine (Windows 11,
PowerShell):

1. a Rust toolchain — `cargo 1.99.1` / `rustc 1.99.1`;
2. `rustup target add wasm32-wasip1` — the host's tests build WebAssembly
   fixtures before they run, so the target is needed even for `cargo test`;
3. `rustup component add clippy` and `rustfmt`, for the lint and format gates CI
   applies to the new crates;
4. on Windows with the GNU toolchain, a MinGW linker on `PATH`, which
   `wasmtime-internal-fiber` needs to build. This is a property of this machine,
   not of the repository; the MSVC build CI uses does not need it.

The commands, in order:

```sh
cd rust
cargo test --workspace          # 144 tests, 0 failed
cargo build --release -p rustacity-ext-cli
python scripts/check_dsp_abi.py      # header and exports agree
python scripts/check_ext_abi.py      # ext header and FFI agree
scripts/build-extensions.sh          # 12 variants for wasm32-wasip1
```

(The interpreter is `python` here; `python3` on this machine is the Microsoft
Store stub and does not run. The README names both.)

The CLI was then run against a real WAV, and against the hostile fixtures, by
hand; that transcript is what the README's demo section shows.

### The full application — not built here

**This machine has no wxWidgets, no MSVC and no CMake build tree**, so the
application was not built here and the README does not claim it was. The
evidence that it builds is CI: the `build` job runs a three-platform matrix
(Windows x64/MSVC, Ubuntu 24.04/GCC, macOS/Xcode) that builds wxWidgets from the
pinned fork, configures with CMake, builds the application — compiling and
linking the Rust cores — and packages it. That job is green on the commit this
audit names; the run is cited in [`EVIDENCE.md`](EVIDENCE.md).

The consequence for the front page is deliberate: the **quick start is the CLI**,
which was run end to end in a clean clone during the final pass — which is where
the branch's one portability bug turned up, in `scripts/build-extensions.sh`
(section 4) — and the full application build points at the workflow, which is the
only recipe that has ever produced a release.

## 3. Evidence inventory

For every claim the front page makes, this is what backs it. Nothing here is a
benchmark, and no performance claim is made anywhere in this branch.

| claim | evidence | where |
| --- | --- | --- |
| the DSP ports reproduce the C++ they replaced | golden values captured from the C++ while it still existed, checked by `cargo test` | `rust/dsp/goldens/reference.txt`, `rust/dsp/tests/*_golden.rs`, `docs/goldens.md` |
| the comparison is not bit-exact | it is a tolerance, and the document says so | `docs/goldens.md` (closing section) |
| the extension ABI is what the header says | `scripts/check_ext_abi.py`, which was checked against deliberate drift | `rust/ext-host/include/rustacity_ext.h`, the `Extensions` job |
| a hostile extension is contained | ten fixtures, one per behaviour, each with the test that insists on it | `rust/extensions/hostile`, `rust/ext-host/tests/hostile.rs`, `docs/extensions/SANDBOX.md` §2 |
| an extension is deterministic (no clock, no randomness, no environment) | the two nondeterministic WASI functions are replaced, WASI is otherwise hollow | `docs/extensions/SANDBOX.md` §1 |
| gain-invert is correct | tests at 1e-6 over several channel counts and block sizes 1/64/4096/65536, plus one check against a reference computed outside the repository | `rust/ext-host/tests/`, `docs/extensions/INTEGRATION_NOTES.md` §3 |
| the application builds with the engine inside it | the three-platform CI matrix, green | `.github/workflows/cmake_build.yml`, run cited in `EVIDENCE.md` |
| there is no telemetry | see "network" in §4 | this audit |

## 4. Hygiene findings

### Already clean

- **No dead CI.** There is no `.travis.yml`, no `appveyor.yml`, no
  `.gitlab-ci.yml`; `.github/workflows/cmake_build.yml` is the only workflow,
  and it triggers on `push`, `pull_request` and `workflow_dispatch`.
- **No duplicated front page.** `README.txt` was already removed; `README.md` is
  the only one.
- **No committed build output.** `git ls-files` finds no `.wasm`, `.exe`, `.dll`,
  `.a`, `.o`, `.so`, `.dylib`, `.pdb`, `.msi` or `.AppImage`, and no tracked file
  over 1 MB. `rust/extensions/dist/` and `rust/target/` are ignored.
- **No secrets.** A scan for private keys and known token prefixes
  (`-----BEGIN`, `ghp_`, `github_pat_`, `AKIA…`) finds only a public PGP block on
  an inherited libsndfile donation page. Every absolute path in the tree is
  upstream's, in files this fork has not modified:
  `scripts/piped-work/recording_test.py` names a path from the upstream author's
  machine and is byte-identical to the `Audacity-2.4.1` tag, and the vendored
  documentation and autotools files under `lib-src/` carry their usual examples.
  Nothing under `src/`, `include/`, `rust/`, `docs/` or `examples/` names a path
  from anybody's machine.
- **No network code in the application.** There is no HTTP client at all: no
  `curl`/libcurl in `CMakeLists.txt` or `src/CMakeLists.txt`, no `wxHTTP`, no
  `wxWebRequest`. The only socket is the single-instance IPC on a local
  Unix-domain socket (`src/RustacityApp.cpp`, `CreateSingleInstanceChecker`), and
  the three `wxLaunchDefaultBrowser` call sites are links the user clicks. There
  is no update checker and no telemetry of any kind.

### Fixed in this branch (see the commits)

| finding | where |
| --- | --- |
| GitHub reports **`NOASSERTION`** for the licence, because the fork notice was prepended to the GPL text and licensee can no longer identify it | `LICENSE.txt` (notice moved to a new `NOTICE`; the file is restored to upstream's bytes) |
| one personal absolute path in a document | `docs/extensions/INTEGRATION_NOTES.md` §6 |
| the in-app help header says **"Audacity 2.4.1"** and calls the upstream forum "our" support | `src/HelpText.cpp:261-279` |
| the bug-report template asks for an **Audacity** version | `.github/ISSUE_TEMPLATE/bug_report.md` |
| no `SECURITY.md`, so there is nowhere to say how to report a sandbox escape | new file |
| no `examples/`, so the quick start had no input to run on | new directory |
| `scripts/build-extensions.sh` **failed in a fresh Windows checkout** - a CRLF blank line in `variants.txt` read as a field holding a carriage return, so the script built the whole workspace and then reported a missing module. Found by running the quick start in a clean clone, which is the first thing the README tells a reader to run | `scripts/build-extensions.sh` |
| no CI status badge, though the workflow passes | `README.md` |
| topics on GitHub are thin and partly invented (`rustacean`, `rustaceans`) and miss `wasm`, `webassembly`, `dsp`, `sandbox`; the description is vague; there is no `gh` on this machine, so the commands are in the owner TODO | GitHub settings |
| two merged-but-undeleted branches (`dsp/abi-hardening`, `dsp/phaser`) and a stale local `master` | refs; owner only, not deleted here |

### Investigated and withdrawn

- **The icons are not Audacity's artwork under a new name.** The first pass read
  a renamed `AudacityLogo*.xpm → RustacityLogo*.xpm` and a changed
  `images/rustacity.svg` as a recoloured copy; the maintainer confirms the assets
  are the project's own. A diff cannot tell a replacement from an edit, and this
  inference was wrong. The only related requirement that survives is the factual
  one: the front page must state that Rustacity is independent and unaffiliated.
- **The `DarkAudacity` strings are dead code, not a user-visible problem.**
  `src/AboutDialog.cpp`, `src/HelpText.cpp`, `src/Internat.cpp`,
  `src/RustacityApp.cpp`, `src/SplashDialog.cpp` and `win/rustacity.rc` all keep
  `#ifdef EXPERIMENTAL_DA` branches from the fork's ancestry, and
  `src/Experimental.h:53` has `EXPERIMENTAL_DA` **commented out**, so every one of
  those branches is compiled out. The live `#else` branches already say
  "Rustacity" (the About box, the splash logo, the Windows version resource, the
  application name). They are noise to a reader grepping the tree, and removing
  them would be a large diff across inherited files, so they are left alone and
  noted here.

## 5. Fork provenance, in numbers

| | |
| --- | --- |
| fork point | the `Audacity-2.4.1` tag, an ancestor of the current tree |
| commits since | 130 |
| files changed since | 1347 (`+42,238 / -194,308`) |
| the application | `src/`, 793 C++ files (386 `.cpp` and 407 `.h`), about 297,000 lines — upstream's |
| the Rust workspace | 60 files, 16,788 lines, of which `rust/dsp/src` is 10 files and 5,387 lines |
| the one new effect | `src/effects/Extension.h` (103 lines) and `Extension.cpp` (309 lines) |
| GitHub's byte count | Rust is about 0.8% of the tree it counts (440 KB of 54 MB), because `lib-src/` and the vendored C are most of the bytes — the honest measure of progress is `docs/port-status.md` |

The deletions are not a rewrite: they are the second build system and the IDE
projects, both of which nothing referenced (`docs/repository-layout.md` §
"Inherited infrastructure that was removed" lists each commit), plus the theme
caches the fork regenerates with `scripts/brand/regenerate_theme_caches.py`.

## 6. What this audit changes about the plan

Two things, both in the direction of saying less:

1. the front page cannot present the extension engine as something a user can
   run from the application, because nothing constructs the effect;
2. the front page cannot present the DSP ports as bit-exact, because they are
   checked to a tolerance.

The demo the front page leads with is therefore the CLI: a real WAV through
`gain-invert`, and a hostile extension being stopped while the host stays usable.
Both were run by hand for this audit.

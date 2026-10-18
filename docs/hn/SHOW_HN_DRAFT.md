# Show HN draft

Not posted. This is a draft for the maintainer to edit, date, and post.

## Titles (pick one, or edit)

1. `Show HN: Rustacity – an Audacity fork with sandboxed WebAssembly extensions`
2. `Show HN: I'm moving Audacity's audio processing into Rust, one unit at a time`
3. `Show HN: Rustacity – an Audacity 2.4.1 fork with a Rust DSP core and a wasm plugin sandbox`

## Body

> I forked Audacity 2.4.1 to do two things: move its audio processing into Rust
> one unit at a time, and add an extension system whose effects are sandboxed
> WebAssembly modules instead of native code loaded into the editor's process.
>
> The DSP half is real. Eight units are Rust now — biquad design and filtering,
> EBU R128 loudness, the bass and treble shelves, phaser, wahwah, distortion, the
> FFT and its windows, and repair interpolation — and they are linked into the
> application that CI builds and packages for Windows, macOS and Linux. Each one
> was ported by capturing reference values from the C++ while it still existed
> and deleting the C++ behind it, so the tests compare against the code I
> replaced rather than against the new code's own output.
>
> The extension half is real too, but it stops somewhere specific. An extension
> is a WebAssembly module plus a `extension.toml` manifest: a versioned ABI,
> planar `f32` audio, no host calls beyond a log line. It runs in Wasmtime with
> one 256 MiB memory, a 5-second wall-clock deadline per call, and a WASI that is
> linked but hollow — no files, no network, no environment, no arguments, no
> clock, no randomness — and there are ten deliberately hostile fixtures in the
> repo that say what happens when a module tries each of those. What does not
> exist yet is the editor side: the C++ class that runs an extension compiles,
> links and ships, and nothing constructs one, so there is no menu entry and no
> parameter dialog. Today you drive an extension from the command line
> (`cargo build --release -p rustacity-ext-cli`, point it at a WAV, pass
> parameters).
>
> 168 tests pass in the Rust workspace, and the quick start is a short
> sequence of commands that needs no wxWidgets and no editor build. Two things I
> want to be plain about: the sandbox is containment inside the editor's process,
> not an operating-system boundary — a bug in Wasmtime or in my glue is a bug in
> the editor — and the application itself is still Audacity 2.4.1's C++ tree, not
> a rewrite. This is one person's work in the open.
>
> The part I'd most like torn apart: the ABI and the sandbox limits. If you have
> written a plugin host, what did you get wrong first?

## First comment to post immediately after submitting

> A few things that do not fit in the post:
>
> **How to try it without building the editor.** `rustup target add
> wasm32-wasip1`, `scripts/build-extensions.sh`, `cd rust && cargo build
> --release -p rustacity-ext-cli`, then point the binary at the sample in
> `examples/`. That sample is generated (a one-second sweep, no third-party
> audio) and `examples/check-output.py` checks the result against arithmetic
> worked out outside the extension: 0 dB comes back exact, +6 dB is off by
> 6.4e-08, which is one float multiply's rounding.
>
> **What is newest.** The C++ side of the engine — the ABI header it checks
> against, the effect that will one day drive it, and the hostile fixtures —
> landed most recently, and a `cli` job in CI now builds the command-line tool
> and the example extensions on every push and runs the quick start as a test.
>
> **Known limits, in one place.** No GUI path to an extension; one channel per
> call; no `labels.write` or `tracks.read` capability; no persistence, no
> project access; the ports are checked to a tolerance, not bit-exactly, and the
> golden values were captured on Linux only; the macOS and Windows installer
> packages are unsigned, so both platforms will warn; arm64 macOS and 32-bit
> Windows are not built at all. All of that is in the README and in
> `docs/extensions/INTEGRATION_NOTES.md`, and the audit at `docs/HN_AUDIT.md` is
> what the README was written from.
>
> **Feedback I'm looking for.** (1) The ABI in `docs/extensions/API.md` —
> anything an effect writer would need that is missing, given that adding it
> later is a version bump. (2) What the sandbox should do that it does not:
> it is currently a set of checks in the editor's own process, and I would
> rather be told now what that costs than after the menu UI exists. (3) If you
> have kept a C++ DSP alive while porting it, how you decided a tolerance was
> tight enough to delete the original.

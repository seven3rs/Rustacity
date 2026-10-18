# Answers to the questions this is likely to get

Written before posting, so the answers are the ones the repository can support.
Two have a `TODO(owner)`: the reason the fork is based on 2.4.1 rather than a
3.x release, and the AI-disclosure answer. Neither is written anywhere in this
tree, and neither is mine to state.

### "So it's Audacity with a new name?"

It is Audacity 2.4.1, the real application, with three changes: the fork's own
name and artwork; eight DSP units moved into Rust and linked in; and a
WebAssembly extension engine added alongside — not instead of — the existing
VST, LV2, AU and Nyquist support, which is untouched. It is not a rewrite, and
the README says so before it says anything else.

### "Why 2.4.1 and not Audacity 3.x?"

`TODO(owner)`: this is not documented anywhere in the repository, and a guess
from me would be worse than the gap.

### "Why not just write a VST/CLAP/LV2 plug-in?"

Those formats load native code into whatever host is running them, and the point
here is the opposite: an effect with a small, versioned contract, no access to
the filesystem, the network, the project or the clock, and a hard bound on how
long it can run. An extension here also gets to know what the user's parameters
are, because the manifest declares them.

### "Isn't WebAssembly in a desktop app much slower than native code?"

Possibly, and I have not measured it, so I am not claiming either way. There is
no benchmark in this repository: the millisecond figures the CLI prints are what
came out on one machine, and they are not published as a comparison. If the
overhead turns out to matter for long chains, that is worth knowing early.

### "What stops an extension from reading my files or phoning home?"

WASI is linked and hollow: the sandbox builds a `WasiCtx` with no preopened
directories, no environment, no arguments, and closed standard streams, and the
one import an extension gets is `rustacity.rx_log`. The fixtures in
`rust/extensions/hostile` try each of those routes; the table of what they tried
and what happened is in `docs/extensions/SANDBOX.md` §2. `random_get` is a fixed
generator and `clock_time_get` a counter, so a run is reproducible, and that is a
deliberate trade: an effect that wants real randomness cannot have it yet.

### "What happens if an extension loops forever or allocates 30 GB?"

A defective or hostile module is stopped: a 5-second wall-clock deadline per call
(checked every 10 ms by an epoch tick), a 256 MiB memory ceiling counted by the
store's limiter, a limit on frames per call, and a bound on how many instances a
module may create for itself. The host reports the failure and poisons that
instance; the editor carries on.

### "Is it a security boundary, then?"

No, and the README says so in the same section it describes the sandbox. The
module runs in the editor's own address space, so a bug in Wasmtime or in the
host glue is a bug in the editor. §3 of `docs/extensions/SANDBOX.md` is the list
of promises this sandbox does *not* make — that one first, then the fact that it
cannot tell good audio from bad, that its deadline is a wall-clock budget, that
`capabilities` is a declaration rather than a restriction, and that nothing
persists. Running extensions out of process is future work.

### "Does it phone home, or check for updates?"

There is no HTTP client in the application at all: no curl/libcurl in either
`CMakeLists.txt`, no `wxHTTP`, no `wxWebRequest`, no update checker, no
telemetry. The only socket is the single-instance IPC on a local Unix-domain
socket, and the three `wxLaunchDefaultBrowser` calls are links a user clicks.

### "Can I still use my VSTs?"

Yes — unchanged. The extension system is additive. Extensions are a second kind
of effect with much less power, and the existing plug-in formats are native code
exactly as they were in Audacity 2.4.1, with the same lack of sandboxing as
before.

### "Why not Lua/monkey-patched DSP/Python?"

There is already Nyquist for scripting. The choice here was about the safety
story: a WebAssembly module has a memory model that cannot reach outside its own
linear memory, and Wasmtime gives a deadline and a memory cap for free. A
scripting language would need those built by hand and would still run in the
editor's process.

### "Why is the DSP in Rust?"

Because it is the part where a bug is silent. The porting rule is: capture the
values the C++ prints *first*, then reimplement, then delete the C++. The ledger
and the golden-value conventions are in `docs/port-status.md` and
`docs/goldens.md`. Eight units have moved so far.

### "Are the ports bit-identical?"

No, and no document here claims it. The comparison in `docs/goldens.md` is a
tolerance, and the goldens were captured on Linux only. "Matches the values the
C++ printed, to a tolerance, on one platform" is the whole claim.

### "How much of the app is actually Rust?"

Very little, by any measure. Around 16,800 lines in `rust/` against ~298,000
lines of C++ in `src/`, and GitHub's own byte count puts Rust at under one
percent, because `lib-src/` is most of the bytes. The honest measure of progress
is the eight units in `docs/port-status.md`, not a percentage.

### "Can I run an extension from the editor yet?"

No. The C++ class that runs one exists, is linked into the shipped binary, and
nothing constructs it — no discovery, no menu entry, no parameter dialog. Today
the CLI is the way, and the README's quick start uses it. This is the next piece
of work, not a thing I am hiding.

### "Which platforms?"

Windows x64, Linux x86_64 and macOS x86_64 (Intel), built and packaged by CI
from
[`.github/workflows/cmake_build.yml`](../.github/workflows/cmake_build.yml).
arm64 macOS and 32-bit Windows are not built at all — their triples are set by
pattern from the DSP core's and no build has been attempted. The macOS and
Windows installers are unsigned, so both platforms warn on install; that is
listed as a known limit, not a bug to report.

### "Why is the released installer missing all this?"

The engine's C++ side (`src/effects/Extension.cpp`) is newer than the latest
release, so no published install has ever contained it. It compiles and links
into the application — `src/CMakeLists.txt` names it, and CI builds and packages
it on three platforms — but a build from `main` still has no menu entry for an
extension, so the CLI remains the way to run one.

### "Is this GPL-compatible / what licence are extensions under?"

Rustacity is GPL v2 or later, exactly as Audacity 2.4.1 is; `LICENSE.txt` is
upstream's file, unchanged, and `NOTICE` records the fork. An extension that
links nothing but the ABI and is a `no_std`-style module is still writing against
a GPL program's interface, so if you want a permissive licence for your own
extension, that is a question to ask a lawyer, not me — the example extensions in
this tree are GPL-2.0-or-later because they live in it.

### "Who made this? Was any of it written by an AI?"

`TODO(owner)`: state what you want to say about how the code was written. The
repository is honest about its own limits in `docs/HN_AUDIT.md`, and this answer
is the one thing a reader cannot verify from the tree, so it should be yours.

### "How can I help?"

Bug reports and feature requests have templates; the rules are in
[CONTRIBUTING.md](../CONTRIBUTING.md) — one idea per commit, no renames,
`lib-src/` untouched, a port is finished only when the C++ it replaced is gone.
The most useful contributions right now are probably: a review of the extension
ABI before it hardens, an extension written against the template, and golden
values captured on a platform other than Linux.

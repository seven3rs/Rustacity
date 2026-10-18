# The extension sandbox

An extension is third-party code. The sandbox's job is to keep a broken or
hostile one from taking the editor, the project, or the machine down with it.
This document says what the sandbox is made of, what has been shown to hold, and
what it does not protect against. The contract an extension is written against
is [API.md](API.md); this is the other side of it.

## 1. What the sandbox is made of

A module runs inside Wasmtime, one `Store` per instance, with:

| | |
| --- | --- |
| WebAssembly features | one memory, 32-bit addressing, no threads, no shared memory, no components |
| linear memory | 256 MiB, counted by the store's limiter |
| time, per call into the module | 5 s of wall clock, checked every 10 ms by an epoch tick |
| a call's audio | at most 65,536 frames, and never more than the instance was prepared for |
| instances | one per effect invocation, and 64 that a module may create for itself |
| imports | WASI preview 1, hollow, and `rustacity.rx_log` |

**WASI is linked and empty.** Every preview 1 function a `std`-built module
imports is present, because a missing function would fail the load of perfectly
reasonable code, but the context behind it has nothing in it:

- no preopened directories, so no path can be opened, created or listed;
- no environment variables and no arguments;
- standard input, output and error are closed: writing to them fails;
- no sockets, because preview 1 has none, and nothing here opens one;
- `random_get` is a fixed generator and `clock_time_get` a counter that starts
  at zero with the instance, so a run cannot depend on the host, and the same
  extension on the same audio twice gives the same result twice.

The module is not even given a way to see whether it is being tested or used.

**Nothing is shared between instances.** Each has its own memory, its own log,
its own counter, and no access to another's. A trap, a timeout or running out of
memory spends that one instance: later calls on it return `Poisoned`, and a
fresh `instantiate` on the same loaded extension starts again from nothing.

**Extensions are built to abort.** The extension workspace sets `panic = "abort"`
in its release profile, so a Rust panic becomes a WebAssembly trap rather than an
unwind looking for somewhere to go. The host never depends on that — it treats
any trap the same way — but it is what makes a panic in an extension an ordinary
`Trap` instead of something worse.

## 2. What has been shown to hold

`rust/extensions/hostile` is a crate of deliberate misbehaviour, one cargo
feature per kind, built into one module each by
`scripts/build-extensions.sh`. The host's tests load every one of them and check
the result. This table is the whole of it; the test in brackets is the one that
insists on it.

| fixture | what it does | what happened |
| --- | --- | --- |
| `hostile-benign` | copies input to output | runs normally: the control, so that the others mean something |
| `hostile-loop` | spins inside `rx_process` | `Timeout` after the 5 s budget, the instance is spent, and a good extension still loads and still runs afterwards (`an_infinite_loop_is_stopped_within_the_deadline`) |
| `hostile-memory` | allocates until it is refused | `OutOfMemory` (or a trap from the failed growth), memory bounded, instance spent (`unbounded_allocation_is_stopped_by_the_memory_limit`) |
| `hostile-trap` | divides by zero | `Trap`, status 6, instance spent, host usable (`a_trap_is_contained`) |
| `hostile-wasi` | reads files, lists a directory, reads the environment and the arguments, resolves a hostname, opens a socket | obtained **zero** bytes, and the log it wrote says so; the audio still passes through (`the_sandbox_gives_the_extension_nothing_to_read`) |
| `hostile-pointers` | `rx_alloc` returns a pointer past the end of memory | `AbiMismatch` at load, before any audio moves |
| `hostile-code` | `rx_process` returns 5 | `ModuleReportedError(5)`, and the instance is **not** spent: the extension said no, the sandbox did not have to (`a_result_code_the_host_does_not_know_is_reported_not_hidden`) |
| `hostile-abi` | `rx_api_version` returns 2 | `AbiMismatch` at load |
| `hostile-import` | imports `env.rx_evil` | `ModuleInvalid` at load, naming the import |
| `hostile-log` | writes 600 messages per call | the call is unaffected, the log stays inside its budget, and the host can say how much was dropped (`a_flood_of_log_messages_stays_within_its_budget`) |

The fixtures are test material, not extensions to install: their manifest id is
`com.rustacity.hostile`, and they exist so that the claims above are checked
rather than asserted.

## 3. What this does not protect against

A sandbox is a set of promises; these are the ones this one does not make.

**It is not a separate process.** The module runs in Rustacity's own address
space, inside Wasmtime, with the host's copies and checks around it. There is no
operating-system boundary here: a bug in Wasmtime, or in the host's own glue, is
a bug in the editor. Running an extension out of process, so that a crash is a
dead child and an error message, is future work, and it is the change that would
make this a containment boundary rather than a set of checks.

**It cannot tell good audio from bad.** An extension that produces silence,
noise, clipping or the wrong gain is not misbehaving in a way the sandbox
notices: it is doing exactly what it was asked. The host checks that a block is
the shape it claims to be, that a pointer is inside memory, and that a call
returns in time; it does not judge a single sample. Extensions carry the same
trust as a built-in effect in that respect, and a user who installs a bad one
hears it.

**It does not make a slow extension fast, or a short one long.** An extension may
use its whole 5 s budget on every call. The audio is then late, and the operation
is slow, but nothing hangs and nothing is lost. Conversely, the deadline is a
wall-clock budget, not an instruction budget: a machine under load gives an
extension fewer instructions in its 5 s. Deterministic instruction counting
(fuel) would trade that for a per-call cost on every extension and a limit that
does not correspond to a sentence a user can act on, so the deadline is time,
and the resolution of "in time" is one 10 ms tick.

**It does not enforce the manifest's capabilities.** `capabilities` is a
declaration, checked at load: a manifest that asks for `labels.write` is refused
because version 1 has no such thing, and an extension that asks for nothing gets
no audio. But an extension that asks for `audio.read` and `audio.write` is given
the audio buffers, and the sandbox is otherwise identical for every extension. A
manifest cannot restrict an extension below what the ABI allows, and no extension
can reach anything the ABI does not hand it.

**It does not persist anything.** Version 1 has no way for an extension to store
anything, to talk to the network, or to see the project: its parameters, its
audio and its log are the whole of its interface, and its memory lasts one
invocation. That is a real limit on what an extension can be — a label editor or
a streaming analyzer needs capability the ABI does not have yet — and it is also
why the sandbox is small enough to describe completely.

## 4. Checking it yourself

From the repository root:

```
scripts/build-extensions.sh                       # build every fixture
python3 scripts/check_ext_abi.py                  # header and FFI still agree
```

and the tests, which build their own fixtures whether or not the above was run:

```
cd rust
cargo test -p rustacity-ext-host --test hostile   # the containment tests
cargo test --workspace                            # all of it, dsp included
```

and from the outside, without the tests:

```
rustacity-ext-cli --describe rust/extensions/dist/gain-invert
rustacity-ext-cli rust/extensions/dist/hostile-wasi  in.wav out.wav --verbose
rustacity-ext-cli rust/extensions/dist/hostile-loop  in.wav out.wav
```

The second prints the log the extension wrote about everything it could not
reach; the third waits five seconds, says the extension timed out, and exits
non-zero. Neither leaves anything running.

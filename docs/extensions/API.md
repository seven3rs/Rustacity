# Rustacity extension API, version 1

An extension is a WebAssembly module plus a manifest. The host loads it into a
sandbox, gives it audio, and takes audio back. This document is the contract:
what an extension file contains, what the module must export, what the host
provides, and what the host guarantees. It describes API version 1, which is
deliberately small.

Nothing here changes the existing plug-in formats. VST, LV2, AU, Nyquist and the
built-in effects are untouched and load exactly as before; the extension engine
is a second, sandboxed path that runs beside them.

- [1. Packaging](#1-packaging)
- [2. The manifest](#2-the-manifest)
- [3. The module ABI](#3-the-module-abi)
- [4. Host imports](#4-host-imports)
- [5. Capabilities](#5-capabilities)
- [6. The sandbox and its limits](#6-the-sandbox-and-its-limits)
- [7. The error model](#7-the-error-model)
- [8. Versioning](#8-versioning)
- [9. Worked example](#9-worked-example)

## 1. Packaging

An extension is a directory:

```
com.example.gain-invert/
  extension.toml     the manifest
  extension.wasm     a core WebAssembly module
```

Both files are required. The directory name is not significant — the manifest's
`id` is what identifies the extension — but by convention it is the `id`. A
directory that is missing either file, or whose manifest does not parse, is
rejected with the reason; it is not installed half-way.

The module is a **core WebAssembly module**, not a component, and it is
compiled for `wasm32-wasip1` or for a bare `wasm32-unknown-unknown` target. See
[6](#6-the-sandbox-and-its-limits) for what WASI is and is not given.

## 2. The manifest

`extension.toml`, read with TOML, all fields required unless marked optional.

```toml
api_version = 1                  # integer, required
id = "com.example.gain-invert"   # reverse-DNS, required, unique
name = "Gain / Invert"           # required
version = "0.1.0"                # semver, required
description = "..."              # optional
author = "..."                   # optional
license = "GPL-2.0-or-later"     # optional, SPDX identifier

capabilities = ["audio.read", "audio.write"]

[[parameters]]
id = "gain_db"
name = "Gain (dB)"
kind = "float"                   # "float" | "bool" | "choice"
min = -60.0                      # float only, required for float
max = 24.0                       # float only, required for float
default = 0.0                    # required for every kind
unit = "dB"                      # optional, display only
description = "..."              # optional, display only

[[parameters]]
id = "invert"
name = "Invert polarity"
kind = "bool"
default = false

[[parameters]]
id = "mode"
name = "Mode"
kind = "choice"
default = 0
choices = ["Add", "Replace"]     # required for choice, index-aligned
```

`description`, `author` and `license` are optional; `capabilities` and
`parameters` may be left out, and then there are none.

Rules the host enforces, each with its own message. Rejections are
`ManifestInvalid` unless the row says otherwise, and the message names the
field and the rule rather than just refusing:

| rule | rejection |
| --- | --- |
| unknown fields are refused, at the top level and inside `[[parameters]]`, so a typo cannot silently do nothing | "unknown field `capabilites`, expected one of `api_version`, `id`, ..." |
| `api_version` must be `1` | "api_version 2 is not supported; this host speaks version 1" |
| `id` must contain a dot and only `A-Za-z0-9`, `.`, `-`, `_`, with no empty part | "id 'gain-invert' must be reverse-DNS, like com.example.my-effect" |
| `name` must not be blank | "name must not be empty" |
| `version` must be a semantic version, digits first, a `-`/`+` suffix allowed | "version 'one' must be a semantic version, like 0.1.0" |
| `capabilities` entries must be known (see [5](#5-capabilities)) | `CapabilityDenied`: "'fs.read' is not a known capability; API version 1 has audio.read, audio.write" / "'labels.write' is reserved and not supported in API version 1" |
| parameter ids must be unique | "duplicate parameter id 'gain_db'" |
| a parameter needs `id`, `name`, `kind` and `default` | "parameter 'mode' needs a name" |
| parameter `kind` must be one of the three | "parameter 'mode' has unknown kind 'enum'; the kinds are float, bool and choice" |
| `float` needs `min` and `max`, and `min` must be smaller | "parameter 'gain_db' of kind float needs min and max" / "parameter 'gain_db' has min 24 and max -60; min must be smaller" |
| `float` and `bool` must not have `choices`; `choice` must not have `min`/`max` | "parameter 'invert' is a bool and cannot have choices" / "parameter 'mode' is a choice and cannot have min or max" |
| `choice` needs a non-empty `choices` list, without repeats | "parameter 'mode' of kind choice needs a non-empty choices list" / "parameter 'mode' lists the choice 'Add' twice" |
| `default` must be a number within `min`/`max`, a bool, or an index into `choices` | "default for 'gain_db' (48) is outside min..max (-60..24)" / "parameter 'mode' has default 7, but there are 2 choices" |
| a `min`/`max` that is not a number is refused: `!(min < max)` covers NaN too | "parameter 'gain_db' has min NaN and max 24; min must be smaller" |

**Parameter order is the order they appear in the manifest.** That order is the
`index` passed to `rx_set_param`, and the order the host's parameter UI shows.
Reordering parameters in a later version of an extension is therefore a breaking
change: it silently reinterprets saved settings.

## 3. The module ABI

The module exports the following. The host checks every one of them, with the
exact signature, and refuses the module if anything is missing or mistyped
(`AbiMismatch`). Names are exact; there is no name mangling.

| export | signature | required |
| --- | --- | --- |
| `memory` | a linear memory | yes |
| `rx_api_version` | `() -> i32` | yes |
| `rx_alloc` | `(size: i32) -> i32` | yes |
| `rx_free` | `(ptr: i32, size: i32) -> ()` | yes |
| `rx_init` | `(sample_rate: f32, max_block_frames: i32, channels: i32) -> i32` | yes |
| `rx_set_param` | `(index: i32, value: f32) -> ()` | yes |
| `rx_process` | `(in_ptr: i32, out_ptr: i32, frames: i32) -> i32` | yes |
| `rx_reset` | `() -> ()` | no — absence is treated as a no-op |

If the module exports `_initialize` — what a Rust `cdylib` for a WASI target
exports when it has runtime setup to do — the host calls it once per instance,
as soon as the instance exists and before anything else.

Loading an extension is, in order:

1. read and validate `extension.toml`;
2. read `extension.wasm` and compile it (`ModuleInvalid` if it is not a module);
3. examine its imports: only WASI preview 1 and `rustacity.rx_log` are allowed
   (`ModuleInvalid` naming the import that is not);
4. build one throwaway instance — the **probe** — with one channel and a block
   of one frame, and call `rx_api_version`, `rx_alloc` and `rx_init` on it.

The probe is what turns a missing export, a mistyped one, an `rx_alloc` that
returns a pointer outside the module's memory, or an `rx_api_version` that is
not 1 into an error at load, where the user picked the extension, rather than
in the middle of a track. It follows that an extension must accept a block of
one frame, as [3.1](#31-audio-layout) requires anyway.

Return codes: `rx_api_version` must return `1`; `rx_init` and `rx_process`
return `0` for success and a non-zero code otherwise, which the host reports as
`ModuleReportedError(code)` — the extension's own number, passed through
unchanged.

### 3.1 Audio layout

Audio is **non-interleaved (planar) `f32`** in the module's own linear memory:

```
in_ptr  -> [ch0: frames samples][ch1: frames samples]...[chN-1]
out_ptr -> the same shape, channels * frames samples total
```

Both buffers are `channels * max_block_frames * 4` bytes, allocated by the host
through `rx_alloc` when the instance is created and reused for every
`rx_process` call. An extension may write `out_ptr` in place of `in_ptr` if it
prefers; the host copies `out_ptr` back afterwards and never reads `in_ptr`
again.

`rx_alloc` returns a pointer into the module's own memory. A null, negative, or
out-of-range pointer is refused (`AbiMismatch`), as is one that starts inside
memory but whose block would run past the end: the host checks both ends before
it copies a single sample. An `rx_alloc` that returns 0 is `OutOfMemory`.

The host calls `rx_process` with `frames` between 1 and `max_block_frames`
inclusive; the last block of a track may be shorter than the others. An
extension must not assume a fixed block size.

On the host's side, a block with the wrong number of channels, channels of
unequal length, or more frames than the instance was prepared for is refused
with `InvalidArgument` and no call into the module.

### 3.2 Call order

```
rx_api_version()
rx_alloc(input_bytes) -> in_ptr
rx_alloc(output_bytes) -> out_ptr
rx_init(sample_rate, max_block_frames, channels)
rx_set_param(i, v) ...            any number of times, before the first block
rx_process(in_ptr, out_ptr, frames)   repeated, in order
rx_free(in_ptr, input_bytes)
rx_free(out_ptr, output_bytes)
```

`rx_set_param` may be called again between blocks: an extension that offers a
parameter should apply a short slew to avoid clicks, and say in its README how
long the slew is. `rx_reset` returns the extension to its state as after
`rx_init` and is what the host calls when it seeks or restarts a track.

Parameter values are checked against the manifest before the module sees them. A
float outside `min..max`, a switch that is not 0 or 1, a choice index that does
not exist, a parameter index or id that does not exist, and a value that is not
a number at all are all refused with `InvalidArgument`, naming the parameter and
the range. Nothing is clamped: a refused parameter leaves the extension at the
value it already had.

The host never calls into a module concurrently, and never calls `rx_process`
after `rx_free`.

## 4. Host imports

Version 1 provides **no custom host imports other than `rx_log`**. A module that
imports anything outside WASI and `rx_log` fails to load (`ModuleInvalid`),
naming the offending import: its module name and field, whether it is a
function, and the signature the host expects.

| import | signature | behaviour |
| --- | --- | --- |
| `rustacity.rx_log` | `(level: i32, ptr: i32, len: i32) -> ()` | debug logging, see below |

`level` is 0 = trace, 1 = debug, 2 = info, 3 = warn, 4 = error; out-of-range
levels are treated as 2. `ptr`/`len` must lie inside the module's memory or the
message is dropped. If the bytes are not valid UTF-8 the message is dropped. The
host rate-limits logging to **64 messages and 8 KiB per `rx_process` call**, and
truncates a single message at 512 bytes; a module that exceeds the budget simply
loses the later messages, and the host can say how many were dropped. Logging
never fails the call.

The host does not print logging by default. The CLI shows it, with the count of
dropped records, unless the extension logged nothing.

## 5. Capabilities

A capability is a promise by the manifest about what the extension intends to do.
The host checks the list at load time and denies anything it does not recognise.

| capability | meaning | v1 |
| --- | --- | --- |
| `audio.read` | may read the input buffer | supported |
| `audio.write` | may write the output buffer | supported |
| `labels.write`, `tracks.read` | reserved for later | **rejected**: "not supported in API version 1" |

An extension with no `capabilities` at all is loaded but is not given audio: the
host refuses to run an instance whose manifest lacks both `audio.read` and
`audio.write`. Capabilities are a check, not a sandbox switch — the sandbox is
the same for every extension; see the next section.

## 6. The sandbox and its limits

The module runs inside Wasmtime, in one `Store` per instance, with:

| limit | v1 default | how |
| --- | --- | --- |
| linear memory | 256 MiB | `StoreLimitsBuilder::memory_size` |
| memories per instance | 1 | `StoreLimitsBuilder::memories` |
| tables per instance | 1, of at most 128 elements | `tables` / `table_elements` |
| instances the module may create | 64 | `StoreLimitsBuilder::instances` |
| time per call into the module | 5 s wall clock, ±10 ms | epoch interruption, 10 ms tick |
| frames per call | 65,536 | host rejects larger |
| instances of the extension | one per effect invocation | one `Store` per instance |

The memory ceiling counts everything the module allocates, including the two
audio buffers the host asks it for. A module that grows past it gets
`OutOfMemory`, not a mystery: the limiter records that it was the memory limit
and not something else. The clock is real for the deadline and virtual for the
extension: `clock_time_get` is a counter, but the deadline is measured in wall
time, which is why an endless loop can be stopped at all.

**WASI is linked but hollow.** The module may import WASI preview 1 functions —
a module built with `std` does — and every one of them is present, but the
context behind them is empty:

- no preopened directories, so `path_open` cannot succeed;
- no environment variables, no arguments;
- stdio is closed: `fd_write` to 1 or 2 fails;
- no sockets: WASI preview 1 offers none, and nothing here opens one;
- no entropy and no real clock: `random_get` returns a fixed pattern and
  `clock_time_get` a counter that starts at zero when the instance is created,
  so an extension cannot observe the host or the wall clock, and runs are
  reproducible.

Consequences worth stating plainly: an extension cannot read files, reach the
network, look at the environment, or talk to the host other than through its
exports and `rx_log`. See [SANDBOX.md](SANDBOX.md) for what that does and does
not protect against.

## 7. The error model

Every host call returns `Result<_, ExtError>` in Rust and an `int` status in the
C ABI. No panic crosses a boundary: every `extern "C"` function runs inside
`catch_unwind` and converts a panic into a status code.

| variant | meaning | C status |
| --- | --- | --- |
| `Internal` | a bug in the host, or a panic caught at a boundary | 1 |
| `ManifestInvalid` | the manifest is missing, unreadable, or violates section 2 | 2 |
| `ModuleInvalid` | the file is not a module, or imports something not allowed | 3 |
| `AbiMismatch` | an export is missing, mistyped, or `rx_api_version` is not 1 | 4 |
| `CapabilityDenied` | a capability is unknown, reserved, or missing for the call | 5 |
| `Trap` | the module trapped: `unreachable`, out-of-bounds, divide by zero, `proc_exit` | 6 |
| `Timeout` | the epoch deadline for one call passed | 7 |
| `OutOfMemory` | the module asked for more memory than the limit allows | 8 |
| `ModuleReportedError(code)` | a required export returned non-zero | 9 |
| `Io` | reading the extension's files failed | 10 |
| `Poisoned` | a previous call in this instance failed; the instance is spent | 11 |
| `InvalidArgument` | a parameter, block or sample rate the host does not allow | 12 |

0 is not an error and has a name of its own, `STATUS_OK`: it is what a C
function returns when it worked. The C constants are
`RUSTACITY_EXT_ERROR_*` in `rust/ext-host/include/rustacity_ext.h`, and
`scripts/check_ext_abi.py` fails if they and `ExtError` disagree.

An internal failure, a trap, a timeout or running out of memory **poisons the
instance**: every later call on it returns `Poisoned`, saying what happened
first, and the host tells the user to undo, because the operation left the
project unchanged. A result code the extension chose (`ModuleReportedError`) and
a refused argument (`InvalidArgument`) do not poison anything: those are the
extension and the caller doing their jobs, and the instance keeps running. A
fresh `instantiate` on the same loaded extension always works — a spent instance
never affects the host or another instance.

## 8. Versioning

`api_version` is a single integer. Adding a capability, an export or a host
import is a change to a later version; within version 1 the ABI is frozen once
anything ships. The host rejects a manifest whose `api_version` it does not
implement, and a module whose `rx_api_version()` disagrees with the manifest.

The parameter list is not part of the ABI version: an extension may add
parameters in a new release, and the host's parameter UI follows the manifest.
Reordering or removing parameters is a breaking change for saved settings.

## 9. Worked example

`rust/extensions/gain-invert` implements this document in about 120 lines. Its
manifest declares `gain_db` (float, -60..24, default 0) and `invert` (bool,
default false); its `rx_process` multiplies each sample of each channel by the
linear gain, or by the negated gain when `invert` is 1, slewing to the new gain
over 128 frames so that a parameter change during playback does not click.

```rust
#[no_mangle]
pub extern "C" fn rx_process(in_ptr: i32, out_ptr: i32, frames: i32) -> i32 { ... }
```

[WRITING_AN_EXTENSION.md](WRITING_AN_EXTENSION.md) walks through building one
from the template.

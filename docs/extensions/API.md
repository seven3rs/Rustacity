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

Rules the host enforces, each with its own message:

| rule | rejection |
| --- | --- |
| `api_version` must be `1` | `ManifestInvalid`: "api_version 2 is not supported; this host speaks version 1" |
| `id` must be non-empty, reverse-DNS, and charset `[A-Za-z0-9.\-]` | `ManifestInvalid`: "id must be reverse-DNS, like com.example.my-effect" |
| `name`, `version` must be non-empty; `version` must be dotted numeric | `ManifestInvalid`: "version must be a semver string like 0.1.0" |
| `capabilities` entries must be known (see [5](#5-capabilities)) | `CapabilityDenied` or `ManifestInvalid`: "capability 'fs.read' is not known" / "'labels.write' is reserved and not supported in API version 1" |
| parameter ids must be unique | `ManifestInvalid`: "duplicate parameter id 'gain_db'" |
| parameter `kind` must be one of the three | `ManifestInvalid`: "parameter 'mode' has unknown kind 'enum'" |
| `float` needs `min`, `max`, `min < max` | `ManifestInvalid`: "parameter 'gain_db' needs min and max" |
| `default` must lie within `min`/`max` (float), be a bool (bool), or index an existing choice (choice) | `ManifestInvalid`: "default for 'gain_db' (48) is outside min..max (-60..24)" |
| `choice` needs a non-empty `choices` list | `ManifestInvalid`: "parameter 'mode' of kind choice needs a non-empty choices list" |

**Parameter order is the order they appear in the manifest.** That order is the
`index` passed to `rx_set_param`, and the order the host's parameter UI shows.
Reordering parameters in a later version of an extension is therefore a breaking
change: it silently reinterprets saved settings.

## 3. The module ABI

The module exports the following. The host checks every one of them, with the
exact signature, at load time, and refuses the module if anything is missing or
mistyped (`AbiMismatch`). Names are exact; there is no name mangling.

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
| `rx_flush` | `() -> ()` | no — host only requires it after the last block |

Return codes: `rx_api_version` must return `1`; `rx_init` and `rx_process`
return `0` for success and a non-zero code otherwise, which the host reports as
`ModuleReportedError(code)`.

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

The host calls `rx_process` with `frames` between 1 and `max_block_frames`
inclusive; the last block of a track may be shorter than the others. An
extension must not assume a fixed block size.

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

The host never calls into a module concurrently, and never calls `rx_process`
after `rx_free`.

## 4. Host imports

Version 1 provides **no custom host imports other than `rx_log`**. A module that
imports anything outside WASI and `rx_log` fails to load (`ModuleInvalid`),
naming the offending import.

| import | signature | behaviour |
| --- | --- | --- |
| `rx_log` | `(level: i32, ptr: i32, len: i32) -> ()` | debug logging, see below |

`level` is 0 = trace, 1 = debug, 2 = info, 3 = warn, 4 = error; out-of-range
levels are treated as 2. `ptr`/`len` must lie inside the module's memory or the
message is dropped. If the bytes are not valid UTF-8 the message is dropped. The
host rate-limits logging to **64 messages and 8 KiB per `rx_process` call**, and
truncates a single message at 512 bytes; a module that exceeds the budget simply
loses the later messages. Logging never fails the call.

The host does not print logging by default. Tests and the CLI show it when asked.

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
| tables / instances | small fixed caps | `StoreLimitsBuilder` |
| time per `rx_process` call | 5 s wall clock | epoch interruption, 10 ms tick |
| frames per call | 65,536 | host rejects larger |
| instances | one per effect invocation | one `Store` per instance |

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
| `InvalidArgument` | a parameter index or value the manifest does not allow | 12 |

A trap or a timeout **poisons the instance**: every later call on it returns
`Poisoned`, and the host tells the user to undo, because the operation left the
project unchanged. A fresh `instantiate` on the same loaded extension works — a
spent instance never affects the host or another instance.

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

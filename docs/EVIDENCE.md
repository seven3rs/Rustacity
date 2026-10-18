# Evidence

What backs the claims this repository makes, how to reproduce each one, and what
is deliberately not claimed. Nothing here is a benchmark: no performance
comparison between Rust and C++, and none between WebAssembly and native code,
is published anywhere in this repository, because none was measured under
conditions worth quoting.

## Reproducing all of it

```sh
cd rust
cargo test --workspace              # 144 tests
cd ..
python3 scripts/check_dsp_abi.py    # the DSP header against the exports
python3 scripts/check_ext_abi.py    # the extension header against the FFI
```

The containment tests build their own WebAssembly fixtures, so the first run
needs `rustup target add wasm32-wasip1`. The extension crates have gates of
their own:

```sh
scripts/build-extensions.sh         # 12 variants, wasm32-wasip1
cd rust/extensions && cargo clippy --workspace --all-targets \
    --target wasm32-wasip1 -- -D warnings && cargo fmt --all --check
```

And the CLI, end to end on a real file:

```sh
python3 examples/make-sine-sweep.py examples/sine-sweep.wav
rust/target/release/rustacity-ext-cli rust/extensions/dist/gain-invert \
    examples/sine-sweep.wav /tmp/gained.wav --param gain_db=6
python3 examples/check-output.py examples/sine-sweep.wav /tmp/gained.wav --gain-db 6
```

Every command above was run on a **clean clone of this branch** before it was
written down here, and the sample the checker is given was regenerated in that
clone first — `git status` afterwards was empty, so the committed WAV is byte for
byte what the generator produces. Two substitutions have to be named rather than
hidden: the interpreter on this machine is `python`, because its `python3` is the
Microsoft Store stub and does not run, and the host toolchain is
`x86_64-pc-windows-gnu`, because this machine has no MSVC linker at all.

That clean clone is also where the branch's one portability bug turned up:
`scripts/build-extensions.sh` did not survive a checkout with CRLF endings, which
is what Git for Windows produces by default, because the blank lines in
`rust/extensions/variants.txt` then read as a field holding a carriage return.
It is fixed in that script, and the fix was verified by running the whole quick
start again in a second fresh clone.

## 1. The test suite

`cd rust && cargo test --workspace` — **144 passed, 0 failed**:

| suite | tests |
| --- | --- |
| `rustacity-dsp` unit tests | 53 |
| DSP golden-value tests (`bass_treble`, `biquad`, `distortion`, `ebur128`, `fft`, `interpolate_audio`, `phaser`, `wahwah`) | 23 |
| `rustacity-ext-api` (unit + `manifest`) | 26 |
| `rustacity-ext-host` (unit, `abi_and_manifest`, `block_sizes`, `gain_invert`, `hostile`) | 35 |
| `rustacity-ext-cli` (`wav_round_trip`) | 7 |

`rust/extensions/` is a separate workspace and is not covered by that command;
its crates are held to the lint and format gates above, and the host's tests
build the fixtures they load.

## 2. Golden values

`rust/dsp/goldens/reference.txt` holds **556 records** of **25 kinds** across
669 lines (`design`, `chebypoly`, `process-design`, `process-weighting`,
`bt-coeffs`, `bt-block`, `ph-block`, `wah-block`, `dis-block`, `ebur128`, the
`fft-*` family, `interp-*`), captured from the C++ implementation **while it
still existed**, at commit `c1eb1c9218`, printed on Ubuntu 24.04 (GCC/glibc) by
the capture harness. The file's own header names the CI runs that produced each
group (`39008051779`, `39030237929`); the loop that keeps this honest is
[`goldens.md`](goldens.md).

Two limits, stated in that document and repeated here:

- the comparison is a **tolerance, not bit equality** — a port has to match the
  values the C++ printed, which is a weaker claim than "identical output";
- capture and the golden tests run on **Linux only**, so what the ports do on
  Windows and macOS is exercised by the application builds, not by these tests.

## 3. The extension engine

- **ABI agreement.** `scripts/check_ext_abi.py` reports 25 exports and 25
  declarations, 13 error statuses each named in the header, 3 parameter kinds,
  API version 1; `scripts/check_dsp_abi.py` reports 39 and 39. Each gate was then
  made to fail on purpose, by renaming one declaration in its own header: the
  extension gate exits 1 with *"exported by `rust/ext-host/src/ffi.rs` but not
  declared in `rust/ext-host/include/rustacity_ext.h`: `rustacity_ext_api_version`"*,
  the DSP gate exits 1 with the mirror message, and both pass again once the file
  is put back.
- **Containment.** `rust/ext-host/tests/hostile.rs` loads ten deliberately
  misbehaving modules and insists on what happened to each: the 5-second
  deadline (`an_infinite_loop_is_stopped_within_the_deadline`), the memory
  ceiling (`unbounded_allocation_is_stopped_by_the_memory_limit`), a trap
  (`a_trap_is_contained`), the hollow WASI
  (`the_sandbox_gives_the_extension_nothing_to_read`), a bad result code
  (`a_result_code_the_host_does_not_know_is_reported_not_hidden`), a log flood
  (`a_flood_of_log_messages_stays_within_its_budget`), an out-of-range
  `rx_alloc` pointer, a wrong API version, an unknown import, and a control that
  runs normally. The table is [`extensions/SANDBOX.md`](extensions/SANDBOX.md) §2.
- **Determinism.** The two nondeterministic WASI functions are replaced by a
  counter and a fixed generator, and the context has no preopens, no
  environment and no arguments — so the same extension on the same audio twice
  gives the same result twice.

## 4. gain-invert, from the outside

The input is the committed sample (`examples/sine-sweep.wav`: mono, 44.1 kHz,
16-bit, one second, generated by `examples/make-sine-sweep.py`, SHA-256
`144f9ec395fd57901a95b29e10d9134ebc73d4989314827cec072eec0a4a81f9`). The
expectation — input times `10^(dB/20)`, negated if the polarity was flipped —
is worked out by `examples/check-output.py`, not by the extension.

| run | largest error | command |
| --- | --- | --- |
| `gain_db=0` | **0.000e+00** (exact) | `--param gain_db=0` |
| `gain_db=6` | **6.399e-08** | `--param gain_db=6` |
| `gain_db=0`, `invert=true` | **0.000e+00** (exact) | `--param gain_db=0 --param invert=true` |

A 16-bit input divides exactly by 32768 into `f32`, so 0 dB and the polarity
flip are exact; +6 dB is one float multiply, and 6.4e-08 is its rounding. The
parameters were set before the first block, which the ABI says are taken
exactly rather than slewed to.

The two hostile runs the front page describes were made here as well:
`hostile-loop` exits 1 with "the extension was still working after 5000 ms"
after **5.54 s** of wall clock, and `hostile-wasi` logs that it obtained
**zero** bytes from everything it tried — that log, in full, is in
[`../examples/README.md`](../examples/README.md).

## 5. The application

**Not built on the machine that wrote this branch** — no wxWidgets, no MSVC, no
CMake tree. The evidence is CI, on the commit this branch starts from:

- run [`39109395290`](https://github.com/prantosmss/Rustacity/actions/runs/39109395290),
  `push` to `main`, **all six jobs success**: `Extensions`, `Goldens`,
  `Windows_64bit` (Visual Studio 17 2022, MSVC), `macOS` (Xcode, Intel runner),
  `Rust Tests`, `Ubuntu_24.04` (GCC). `Publish Release` is skipped, as it only
  runs for tags.
- the Linux job additionally proves the Rust core is *linked* into the binary,
  not merely built (`nm … | grep rustacity_biquad_design_butterworth`).

What that does **not** establish: that any effect has been run inside the
application. Nothing constructs `src/effects/Extension.cpp` yet, so the C++
side of the engine has been compiled, linked and packaged, and never invoked
there. That is stated on the front page and in
[`extensions/INTEGRATION_NOTES.md`](extensions/INTEGRATION_NOTES.md) §4.

## 6. The machines

Where the numbers above were produced here:

| | |
| --- | --- |
| machine | Intel Core i5-8250U (4 cores / 8 threads), 15.4 GiB RAM, Windows 11 Pro |
| toolchain | `cargo 1.99.1 (5f94df478 2026-08-27)`, `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, the `x86_64-pc-windows-gnu` host |
| other tools | Python 3.15.0, git 2.56.0.windows.2 |
| needed for tests | `rustup target add wasm32-wasip1`; a MinGW linker on `PATH` for `wasmtime-internal-fiber` on the GNU toolchain |

CI runs Ubuntu 24.04, Windows Server 2022 and macOS 15 (Intel) runners; the
golden values were captured on Ubuntu 24.04 with GCC/glibc.

## 7. Not claimed

- **No performance numbers.** The millisecond figures the CLI prints are shown
  because they are what came out, and they are not a benchmark: one machine, no
  warm-up discipline, no repetition, no comparison.
- **No bit-exactness with the C++.** The ports are checked to a tolerance
  against values captured on one platform.
- **No claim about arm64 macOS or 32-bit Windows**: their triples are set by
  pattern from the DSP core's, no library name has been hashed out for either,
  and no build has been attempted.
- **No claim that the extension engine is a security boundary.** It is a set of
  checks inside the editor's process; [`extensions/SANDBOX.md`](extensions/SANDBOX.md) §3
  lists exactly what it does not protect against.
- **No telemetry claim is a feature claim**: the application contains no HTTP
  client at all (no `curl`/libcurl, no `wxHTTP`, no `wxWebRequest`), no update
  checker, and its only socket is the local single-instance IPC. That is a
  statement about the code in this tree, not a promise about anything else.

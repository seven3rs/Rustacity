# The Rust half of Rustacity

`rustacity-dsp` is the DSP core of Rustacity: audio maths that used to live in
`src/`, ported to Rust and linked into the C++ application as a static
library. The project-level picture - the two halves, the ABI contract, the
panic policy and the porting rule - is in
[`docs/architecture.md`](../docs/architecture.md); this page is mostly that
crate.

The same workspace also holds the sandboxed WebAssembly extension engine
(`ext-api`, `ext-host` and the `ext-cli` tool), which is a separate subject with
separate documentation: [`docs/extensions/API.md`](../docs/extensions/API.md),
[`docs/extensions/SANDBOX.md`](../docs/extensions/SANDBOX.md),
[`docs/extensions/WRITING_AN_EXTENSION.md`](../docs/extensions/WRITING_AN_EXTENSION.md)
and [`docs/extensions/INTEGRATION_NOTES.md`](../docs/extensions/INTEGRATION_NOTES.md).

## Layout

| path | what it is |
| --- | --- |
| `Cargo.toml` | the workspace; `panic = "unwind"` on purpose, see below (the `extensions/` workspace sets `panic = "abort"` instead, for the reason in [`docs/extensions/SANDBOX.md`](../docs/extensions/SANDBOX.md)) |
| `dsp/src/lib.rs` | the module list |
| `dsp/src/bass_treble.rs` | EffectBassTreble's low/high shelf filters |
| `dsp/src/biquad.rs` | biquad filter design and processing |
| `dsp/src/ebur128.rs` | EBU R128 loudness measurement |
| `dsp/src/phaser.rs` | EffectPhaser's allpass filter |
| `dsp/src/wahwah.rs` | EffectWahwah's wah filter |
| `dsp/src/distortion.rs` | EffectDistortion's waveshaper |
| `dsp/src/fft.rs` | the FFT and the window functions |
| `dsp/src/interpolate_audio.rs` | Repair's LSAR interpolation and the algebra under it |
| `dsp/src/ffi.rs` | the C ABI: every entry point, each under a panic guard |
| `dsp/include/rustacity_dsp.h` | the C header the C++ side includes |
| `dsp/goldens/reference.txt` | reference values captured from the original C++ |
| `dsp/goldens/capture/` | dumpers that print those values, one per port in progress |
| `dsp/tests/` | golden-value tests that read that file |
| `ext-api/src/` | the extension API's manifest, error type and limits |
| `ext-host/src/ffi.rs` | the extension host's C ABI, matching `ext-host/include/rustacity_ext.h` |
| `ext-host/src/` | the sandbox: Wasmtime, the deadline, the memory ceiling, the log |
| `ext-cli/src/` | `rustacity-ext-cli`, running one extension over one WAV file |
| `extensions/` | a workspace of its own: the example extensions and the hostile fixtures, built for `wasm32-wasip1` |

## Building and testing

```sh
cd rust
cargo test --workspace
```

The DSP crate has no dependencies, so this needs nothing but a Rust toolchain.
The extension host is the other way round: it bundles Wasmtime and Cranelift,
which is why the C++ build asks for the two crates it links and nothing else
(`cargo build --release -p rustacity-dsp -p rustacity-ext-host`, in
`src/CMakeLists.txt`), and why a Rust toolchain is required to build Rustacity
at all.

`scripts/check_dsp_abi.py` compares this crate's exports with the C header and
is run by CI; run it yourself after touching either side.
`scripts/check_ext_abi.py` does the same for the extension host.

## Adding a module or an entry point

1. Write the maths as a module in `dsp/src/` with unit tests of its own, and
   add it to `lib.rs`.
2. Expose it: a `#[no_mangle] pub extern "C"` function in `ffi.rs` under
   `ffi::guard`, plus a declaration in `dsp/include/rustacity_dsp.h`.
3. Run `cargo test --workspace` and `python3 scripts/check_dsp_abi.py`.

The contract those functions have to keep - an opaque handle instead of one
call per sample (the `rustacity_fft_*` entry points pass the caller-owned
tables in instead, and the reasoning is in
[`docs/architecture.md`](../docs/architecture.md)), Rust owning the loop,
arguments validated before use, and no panic crossing the boundary - is in
[`docs/architecture.md`](../docs/architecture.md). Do not set
`panic = "abort"`: a panic must become a wrong value, not an abort that costs
the user their unsaved work.

## Adding a golden test

Capture the values while the C++ still exists (that is the porting rule, and
the step-by-step is in [`docs/goldens.md`](../docs/goldens.md) with the dumper
mechanics in `dsp/goldens/capture/README.md`), append them to
`dsp/goldens/reference.txt`, then add a test as `dsp/tests/<unit>_golden.rs`.
`dsp/tests/common/mod.rs` loads the reference file for every test in the
directory.

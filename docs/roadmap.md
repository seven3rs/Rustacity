# Roadmap

## Rust-first

Audio processing is moving to Rust one unit at a time; the C++ tree is the
shell being retired, and it is not extended. The plan that makes each move
safe is in [`docs/architecture.md`](architecture.md), the ledger of what has
moved is in [`docs/port-status.md`](port-status.md), and the numbers that keep
a port honest are in [`docs/goldens.md`](goldens.md).

The **FFT in Rust** has landed (`rust/dsp/src/fft.rs`, the arithmetic of
`src/RealFFTf.cpp` and `src/FFT.cpp`), so `NoiseReduction`, `NoiseRemoval`,
`Equalization`/`Equalization48x` and `ClickRemoval`, which all work in the
frequency domain, are no longer waiting on one. The crate still has no
dependencies.

The **repair interpolation** has landed as well
(`rust/dsp/src/interpolate_audio.rs`, the arithmetic of
`src/InterpolateAudio.cpp` and `src/Matrix.{h,cpp}`): the first unit outside
`src/effects/` to follow the FFT, and the first that drew on the C library's
random numbers, which it now draws from a generator of its own so that a repair
is reproducible.

## The audio backend

The **first backend slice has landed**: `rust/engine`, `rust/io` and
`rust/backend` own the samples and edits behind a WAV-imported clip, while the
existing wxWidgets project, track, clip and TrackPanel code stays in place.
`Sequence` has a Rust path for reads, writes, append, insert, delete, silence,
slice, min/max, RMS and waveform columns; legacy `.aup` block-file sequences
keep the original path.

**Rust clips now persist without being materialized.** A Rust sequence is saved
to a versioned `.rsq` sidecar through `RustBlockFile`, a real `BlockFile` that
`DirManager` can copy, lock and clean up. The `.aup` XML names the sidecar
directly under `<sequence>`, so save/reload preserves the Rust samples and the
next save does not create `.au` blocks. The sidecar's version-2 header now
carries the clip record (`frames`, `rate`, `min`, `max`, `rms`, sample format),
so a clip can be described without decoding its samples; version-1 sidecars
still load. Existing projects and every non-WAV importer still use the legacy
block-file path.

The next backend steps, in order:

- turn the `.rsq` sidecar into a Rust project format, so clip metadata and the
  project itself can move without an `.aup` wrapper; the design and its
  incremental slices are in [`docs/project-format.md`](project-format.md);
- the remaining importers (FLAC, Ogg, FFmpeg, ...) either on the Rust decoder or
  explicitly left on libsndfile;
- project, track and undo metadata in Rust, so `Sequence` no longer has to
  dispatch between two stores;
- only then delete the C++ block-file implementation.

## The extension engine

The point of moving the DSP to Rust is the extension engine named on the
front page: a Rust-based layer for adding functionality to the editor. The C
ABI, the panic policy and the golden-value discipline exist to make a Rust
component something that can be added to the application without fear.

**Version 1 of the API has landed** (`rust/ext-api`, `rust/ext-host`,
`rust/ext-cli`, and the extension workspace under `rust/extensions/`): a
manifest, eight exported functions, planar audio, one import for logging, and a
sandbox that gives an extension five seconds of wall clock per call, 256 MiB of
memory, a hollow WASI and nothing else. The contract is
[`docs/extensions/API.md`](extensions/API.md), what the sandbox is and is not is
[`docs/extensions/SANDBOX.md`](extensions/SANDBOX.md), and what has been built
and tested - and what has not - is
[`docs/extensions/INTEGRATION_NOTES.md`](extensions/INTEGRATION_NOTES.md).

What the editor side still needs, in the order it becomes possible:

- where installed extensions live, and a menu entry per extension: the
  discovery that the LADSPA, LV2 and Nyquist modules already do for their own
  formats. `src/effects/Extension.cpp` loads and runs one; nothing constructs
  one yet;
- a dialog for the parameters a manifest declares, which the host's C ABI
  already describes;
- audio that needs all channels at once, rather than one channel per call;
- the capabilities the ABI deliberately does not have yet (`labels.write`,
  `tracks.read`), which is what a label editor or a streaming analyzer would
  need;
- running an extension out of process, so that a sandbox failure costs a child
  process rather than the editor.

## What is not going anywhere

`lib-src/` is vendored third-party code and is never touched. The wxWidgets
effect framework stays C++ by design - it is the interface the user sees, not
signal processing. And the classes in `src/effects/` stay even after their
maths has moved, because they are what the application's effect list, dialogs
and undo history are built from.

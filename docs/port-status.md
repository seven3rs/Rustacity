# Port status

The audio maths moves from `src/` to `rust/dsp/src/`, one unit at a time,
under the rule in [`docs/architecture.md`](architecture.md): a port is
finished only when the C++ implementation is deleted. This is the ledger.

## The audio backend boundary

The first slice that moves more than maths is **WAV import and the samples
behind a clip**. `rust/backend` owns one mono `Sequence` per WAV channel:
storage, read/write, append, insert, delete, silence, slice, min/max, RMS and
waveform columns. `PCMImportFileHandle` asks `RustBackend::ImportWav` for a
WAV; the C++ `Sequence` adopts the resulting handle and delegates its sample
and peak operations. The C++ project, track, clip, TrackPanel and dialogs are
unchanged, so the visible application is still the wxWidgets editor.

This is deliberately a **dual path**, not a finished port. Existing `.aup`
block-file sequences and every non-WAV importer still use the original C++
`BlockArray` implementation. A Rust-imported WAV is stored as float samples,
regardless of the old preference that could choose `int16Sample`; playback and
export are unaffected, but the clip reports the float format.

Save no longer materializes a Rust clip. `Sequence::WriteXML` writes a
`<sequence rust="1">` with one `<rustblockfile>` child, and `RustBlockFile`
owns the versioned `.rsq` sidecar. `DirManager` sees it as an ordinary
`BlockFile` for locking, copying, project checks and orphan cleanup; a dirty
sequence writes a new sidecar instead of overwriting one that a saved project
still references, which is also what keeps AutoSave from changing the audio
behind a saved project. While a save or AutoSave replaces a sidecar, the old
one is retained and locked until the new project or autosave file is committed,
so the previous autosave stays recoverable. A missing or corrupt sidecar is
replaced with silence.
Until a Rust project format replaces the `.aup` wrapper, the sidecar is still
referenced from the legacy XML, and the undo manager's disk-space estimate
still does not count the Rust buffer. What remains before the legacy C++
sequence path can be deleted:

- a Rust project/clip format beyond the `.rsq` sidecar, so the `.aup` wrapper
  and its clip metadata can move too;
- moving the remaining importers (FLAC, Ogg, FFmpeg and the rest) onto the
  Rust decoder, or keeping them on libsndfile deliberately;
- project, track and undo metadata in Rust, so the C++ `Sequence` is no longer
  the object that has to switch between two stores.

## Ported

| unit | C++ maths it replaced | Rust | reference values |
| --- | --- | --- | --- |
| biquad filter design and processing | `src/effects/Biquad.cpp` (the filter `ScienFilter` uses) | `rust/dsp/src/biquad.rs` | `design`, `chebypoly`, `process-design`, `process-weighting` |
| EBU R128 loudness measurement | `src/effects/EBUR128.cpp` (the meter `Loudness` and `Contrast` call) | `rust/dsp/src/ebur128.rs` | `ebur128` |
| bass and treble shelves | `src/effects/BassTreble.cpp` | `rust/dsp/src/bass_treble.rs` | `bt-coeffs`, `bt-block` |
| phaser allpass chain | `src/effects/Phaser.cpp` | `rust/dsp/src/phaser.rs` | `ph-block` |
| wahwah | `src/effects/Wahwah.cpp` | `rust/dsp/src/wahwah.rs` | `wah-block` |
| distortion waveshaper | `src/effects/Distortion.cpp` | `rust/dsp/src/distortion.rs` | `dis-block` |
| FFT and window functions | `src/RealFFTf.cpp` and `src/FFT.cpp` (both transforms, the tables, the reorders and the window functions) | `rust/dsp/src/fft.rs` | the twelve `fft-*` kinds |
| repair interpolation | `src/InterpolateAudio.cpp` and `src/Matrix.{h,cpp}` (the LSAR interpolation `Repair` calls, and the linear algebra under it) | `rust/dsp/src/interpolate_audio.rs` | `interp-rand`, `interp-lsar`, `interp-linear` |

In every case the class in `src/effects/` stays. It owns the dialog, the
parameter marshalling, the track iteration and the buffer copying, and calls a
Rust handle per channel; the maths exists in exactly one place. The FFT is the
same shape from outside `src/effects/`: `src/RealFFTf.cpp` keeps the pool of
tables and `src/FFT.cpp` the translated window names, and both hand their tables
to the Rust core. The repair interpolation is that shape again:
`src/InterpolateAudio.cpp` keeps the entry point `Repair` calls, and the algebra
is in the crate. The record formats in `rust/dsp/goldens/reference.txt` name
each unit's original C++ source, which is the record of what was deleted.

The four realtime effects are done and the set is closed: `SupportsRealtime`
is overridden by exactly four built-in effects - BassTreble, Phaser, Wahwah
and Distortion - so no further effect can be a realtime candidate without the
framework itself changing.

## Not ported

**The effect framework stays C++.** `Effect.cpp`, `EffectUI.cpp`,
`EffectManager.cpp`, `LoadEffects.cpp` and `RealtimeEffectManager.cpp` are
bound to wxWidgets and to the application's event and dialog machinery. They
are the shell the Rust core is called from, not audio maths.

**The frequency-domain effects are no longer waiting on anything.** The FFT
`NoiseReduction`, `NoiseRemoval`, `Equalization` with `Equalization48x` and
`ClickRemoval` need is now in the crate (`rust/dsp/src/fft.rs`), so any of
them can be the next port. The crate still has no dependencies.

**Too little maths to be worth a boundary.** `Amplify`, `Normalize`, `Fade`,
`Invert`, `Reverse`, `Repeat`, `Silence`, `StereoToMono` and the tone, click
and noise generators are a few lines each; a handle, an ABI entry point and a
golden pass would cost more than they save.

**Offline only.** `Reverb`, `Echo` and `Compressor` have no realtime path -
the framework renders them offline - so porting them buys less than a realtime
effect does.

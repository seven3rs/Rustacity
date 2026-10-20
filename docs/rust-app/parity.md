# Rust shell parity matrix

The Rust shell is the `rust/app` workspace. It starts as an MVP and grows
towards the feature set of the C++ application in `src/`. A row is only "done"
when the Rust path is tested and the C++ path is either deleted or explicitly
kept as a compatibility shim.

**Status:** M1 — WAV vertical slice. Decode, waveform, playback and seek work;
editing and project I/O are still ahead.

## M1 vertical slice

The first slice is deliberately one file and one track:

- `rustacity-io` reads 8/16/24/32-bit integer and 32-bit float WAV files with
  `hound`, normalizes to interleaved `f32`, and builds a mono min/max peak
  pyramid for the view. `AudioPlayer` adapts the file to the default `cpal`
  output device (channel layout and sample rate) and exposes play, pause, stop,
  seek and a pollable position.
- `rustacity-ui` draws the waveform, ruler and playhead on a custom canvas.
  Left-click seeks, the wheel zooms around the cursor, and horizontal wheel
  scroll pans.
- `rustacity-app` owns the loaded track, view range and transport. `Open…`
  uses `rfd`; a WAV passed as the first command-line argument opens at boot.
- The playback path is covered by unit tests and one `#[ignore]`d
  hardware-only test (`cargo test -p rustacity-io -- --ignored`).

Known limits: WAV only, one track, no editing or selection, no volume control
in the UI, and no recording. FLAC/MP3, `.aup` and preferences remain M4.

## MVP (M1–M5)

| Feature | C++ reference | Rust status | Notes |
| --- | --- | --- | --- |
| Open WAV | `src/import/` | working (M1) | 8/16/24/32-bit PCM and f32 |
| Waveform view | `src/tracks/` | working (M1) | peak cache + custom canvas |
| Play / seek | `src/` audio I/O | working (M1) | `cpal`, click-to-seek |
| Open FLAC / MP3 | `src/import/` | planned (M4) | `symphonia` |
| Tracks, mute, solo, gain | `src/tracks/` | planned (M2) | |
| Select, cut, copy, paste | `src/` commands | planned (M2) | |
| Undo / redo | `src/` history | planned (M2) | |
| Effects (Amplify, Normalize, Fade, Bass/Treble, Distortion, Phaser, Wahwah) | `src/effects/` | planned (M3) | DSP already in `rust/dsp` |
| Export WAV / FLAC | `src/export/` | planned (M3) | |
| Open / save `.aup` | `src/xml/`, blockfiles | planned (M4) | round-trip tests required |
| Preferences | `src/prefs/` | planned (M4) | device and theme first |
| Plugins | `src/effects/` | out of MVP | LADSPA/LV2/VST/Nyquist later |

## Full parity (post-MVP)

The full matrix is expanded during M0's inventory task from the menus, dialogs,
effects, formats and preference pages in `src/`. It is the exit condition for
deleting the C++ tree; it is not a wish list.

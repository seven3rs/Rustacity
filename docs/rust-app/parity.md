# Rust shell parity matrix

The Rust shell is the `rust/app` workspace. It starts as an MVP and grows
towards the feature set of the C++ application in `src/`. A row is only "done"
when the Rust path is tested and the C++ path is either deleted or explicitly
kept as a compatibility shim.

**Status:** M0 — skeleton only. No audio or project I/O yet.

## MVP (M1–M5)

| Feature | C++ reference | Rust status | Notes |
| --- | --- | --- | --- |
| Open WAV | `src/import/` | planned (M1) | first vertical slice |
| Waveform view | `src/tracks/` | planned (M1) | peak cache + custom canvas |
| Play / seek | `src/` audio I/O | planned (M1) | `cpal` |
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

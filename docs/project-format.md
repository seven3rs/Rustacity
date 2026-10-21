# The Rust project format

This document is the design for the next backend step in
[`docs/roadmap.md`](roadmap.md): a Rust-owned project and clip format, so the
`.aup` wrapper can eventually be dropped for projects that this build writes.
It is a design, not a finished format; the first implementation slices are
listed at the end so that the work can land one step at a time.

## Why

The audio is already Rust-owned. A WAV-imported clip keeps its samples in a
`.rsq` sidecar and the C++ `Sequence` delegates every edit to `rust/backend`.
Everything around those samples is still the legacy `.aup` XML: the project
settings, the track list, clip offsets, colours, envelopes, and the
`<sequence>` / `<rustblockfile>` attributes that describe the sidecar.

That split is the reason the C++ path cannot be deleted. As long as the
project is an XML file that the C++ `ProjectFileIO` writes, the C++ `Sequence`,
`WaveClip`, `WaveTrack` and `ProjectSettings` classes have to keep a complete
serializer, and the Rust backend can never be the thing that knows what a
project is.

The destination is a project that Rust can save and load on its own: one
versioned file that holds the project, its tracks and clips, their metadata,
and the audio. The C++ shell then becomes a reader and writer of that model
through the backend ABI, exactly as it is today for samples and edits.

## What has to move

| today | lives in | eventual owner |
| --- | --- | --- |
| mono sample buffer | `.rsq` sidecar / `RustSequence` | Rust clip record (already Rust-owned) |
| sequence fields (`maxsamples`, `sampleformat`, `numsamples`, `rust`) | `.aup` `<sequence>` | Rust clip record |
| sidecar fields (`len`, `min`, `max`, `rms`, `rate`) | `.aup` `<rustblockfile>` | Rust clip record |
| clip fields (`offset`, `colorindex`, cut lines) | `.aup` `<waveclip>` | Rust clip record |
| envelope (`numpoints`, `<controlpoint>`) | `.aup` `<envelope>` | Rust clip record |
| track fields (`name`, view, controls, `channel`, `linked`, `rate`, `gain`, `pan`, `colorindex`) | `.aup` `<wavetrack>` | Rust track record |
| project fields (`rate`, `snapto`, formats, view info) | `.aup` `<project>` | Rust project record |
| tags | `.aup` `<tags>` | Rust project record |
| label, note, time and MIDI tracks | `.aup` track elements | later Rust track records |
| undo history | memory / AutoSave | later Rust history record |

The clip record is the first target because Rust already owns the samples and
the fields around them are small and well-bounded.

## Constraints

The format has to fit the rest of the project:

- The C++ shell, the wxWidgets TrackPanel and every dialog stay. There is no
  Rust GUI and no new C++ UI code.
- The ABI rules in [`docs/architecture.md`](architecture.md) apply: opaque
  handles, scalar and pointer arguments only, no C++ type across the boundary,
  input validation, and a `guard` around every entry point so a core bug
  cannot abort the editor.
- The header and `ffi.rs` change together; `scripts/check_backend_abi.py`
  enforces that.
- Legacy `.aup` projects, `.au` blocks and every non-WAV importer keep
  working. A project that contains anything the new format cannot represent is
  written as `.aup`, not silently downgraded.
- The sidecar lifecycle stays: immutable audio files, copy-on-write sequence
  buffers, AutoSave retention and orphan cleanup.
- The backend gains no dependency. The existing `.rsq` writer and the WAV
  decoder use only `std`, and the project format should too: a small,
  hand-written, little-endian binary encoding with explicit lengths, not a
  serialization framework.

## File and layout

The project file is a single versioned binary container. The proposal is the
extension **`.rsp`** and the eight-byte magic **`RUSTPRJ1`**; both are easy to
change before the first slice lands.

```
header
  magic         8 bytes   "RUSTPRJ1"
  version       u32       format version, 1
  flags         u32       reserved, zero
  section count u32
  section table count * { tag:4, version:u32, length:u64, offset:u64 }
sections
  PROJ  project settings and view
  TAGS  tags
  TRKS  track directory
  CLPS  clip directory
  ENVS  envelope control points
  AUDX  sidecar references (see "Audio")
  EXTN  unknown vendor data, copied through untouched
```

Every section is length-delimited and skipped if its tag is unknown, so a
newer writer can add fields without an older reader failing. A record inside a
section is a tag-length-value item: `id:u16`, `type:u8`, reserved `u8`,
`length:u32`, then the value. Integers are little-endian; strings are UTF-8
with an explicit length and no terminating NUL; floats are IEEE-754. The rules
are deliberately the same shape as the `.rsq` header so there is one encoding
style in the backend.

Records:

- **PROJ**: format version, sample rate, snap setting, selection/frequency/
  bandwidth formats, zoom and scroll view state, and the project tags.
- **TRKS**: one record per track: a stable id, kind, name, selected flag, view
  fields (height, minimised, VRuler), control fields (mute, solo, gain, pan),
  and the wave fields (`channel`, `linked`, `rate`, `colorindex`) for wave
  tracks.
- **CLPS**: one record per clip: a stable id, the track id, `offset`,
  `colorindex`, optional clip name, cut-line links, the envelope reference,
  and an audio reference. The sequence fields (`maxsamples`, `sampleformat`,
  `numsamples`, and the `len`/`min`/`max`/`rms`/`rate` of the audio) live with
  the audio itself, in the `.rsq` metadata section.
- **ENVS**: one record per envelope: the owning clip id and the ordered
  `(t, val)` control points.
- **AUDX**: the audio references, described next.

### Audio

Version 1 of the container **references** the immutable `.rsq` files rather
than embedding the samples:

- `AUDX` holds, for each sequence, the clip id it belongs to and the relative
  path to the `.rsq` sidecar, plus the frame count and sample rate for
  validation.
- The existing `RustBlockFile`, `DirManager` locking, `ProjectSetter` copying,
  AutoSave retention and orphan cleanup keep working, because the audio is
  still the same sidecars in the same project data directory.
- Save As copies the referenced sidecars exactly as it does now. A project
  that moves between machines is a project file plus its `_data` directory,
  which is what an `.aup` project already is.

Embedding the audio in `AUDO` sections is the natural follow-up, but it is
deliberately not version 1: a self-contained project file rewrites all audio
on every save and needs a content-addressed blob store to stay incremental.
The reader can accept an `AUDO` section later without a format break.

## ABI shape

The C++ model is the source of truth while the format is introduced, so the
backend gets a project handle that the model builds and reads:

```c
typedef struct rustacity_project rustacity_project;

rustacity_project *rustacity_project_new(uint32_t sample_rate);
void rustacity_project_free(rustacity_project *project);

/* project fields */
int rustacity_project_set_rate(rustacity_project *project, uint32_t rate);
int rustacity_project_set_view(rustacity_project *project, ...);
int rustacity_project_add_tag(rustacity_project *project, const char *name,
                              const char *value);

/* tracks */
size_t rustacity_project_track_count(const rustacity_project *project);
int rustacity_project_add_track(rustacity_project *project, uint32_t kind);
int rustacity_project_track_set_name(rustacity_project *project, size_t track,
                                     const char *utf8_name);
int rustacity_project_track_set_view(rustacity_project *project, size_t track,
                                     ...);
int rustacity_project_track_set_controls(rustacity_project *project,
                                         size_t track, ...);

/* clips */
int rustacity_project_add_clip(rustacity_project *project, size_t track);
int rustacity_project_clip_set_offset(rustacity_project *project, size_t track,
                                      size_t clip, double offset);
int rustacity_project_clip_set_envelope(rustacity_project *project,
                                        size_t track, size_t clip,
                                        const double *points, size_t count);
int rustacity_project_clip_set_sequence(rustacity_project *project,
                                        size_t track, size_t clip,
                                        const rustacity_sequence *sequence,
                                        const char *utf8_sidecar_path);

/* save and load */
int rustacity_project_save(const rustacity_project *project,
                           const char *utf8_path);
rustacity_project *rustacity_project_load(const char *utf8_path);

/* getters for the return path, one per field the C++ model needs */
uint32_t rustacity_project_track_kind_at(const rustacity_project *project,
                                         size_t track);
...
```

The names and the exact split are illustrative; the rule is that every field
the C++ model reads or writes gets one entry point, the same way the DSP
modules expose one handle and a block call. The alternative - letting Rust
emit and parse a canonical XML manifest - was rejected because it keeps the
metadata in XML and only renames the wrapper; if a short bridge is ever
needed, it should be temporary and private to the backend.

## Compatibility and migration

- **Open.** A path ending in `.rsp` is loaded through
  `rustacity_project_load`; everything else keeps the existing `.aup` path.
  Unknown major versions, truncated containers and missing `AUDX` sidecars
  fail the load and leave the project untouched; the caller reports the same
  error it reports for a corrupt `.aup`.
- **Save.** A project that this build can represent entirely through the Rust
  records is written as `.rsp`. A project that still contains an unsupported
  element - a label, note, time or MIDI track, an alias, a legacy `.au` block,
  an unknown XML element carried through from a fork - is written as `.aup` as
  today. The decision is made by the C++ model, which is the only side that
  knows whether it built a complete project handle.
- **AutoSave.** The autosave path writes the same container through a
  `.rsp.tmp` and renames it, and reuses `DirManager`'s AutoSave retention for
  the sidecars the container references.
- **Legacy removal.** Only after the reader and writer cover every track type
  the application can create, and the undo manager can restore through the
  Rust model, can the C++ `.aup` writer and the `BlockArray` path be deleted.

## Incremental plan

Each step is a commit that leaves the tree building and the legacy path
unchanged.

1. **Clip record v2.** Extend `.rsq` with a metadata section carrying the
   sequence and sidecar fields (`len`, `rate`, `min`, `max`, `rms`,
   `maxsamples`, sample format), keep the version-1 reader, and let
   `RustBlockFile` read that section without loading the samples. This gives
   the clip record its on-disk home and is testable entirely in the backend
   crate.
2. **Project records and round trip.** Add `rust/backend/src/project.rs` with
   the container and the record types, save/load functions, and Rust unit
   tests for empty, truncated, unknown-section and Unicode cases. No C++ yet.
3. **C++ write path.** Teach `ProjectFileIO` to build a project handle for a
   project whose tracks are all wave tracks with Rust clips, save it as
   `.rsp`, and fall back to `.aup` for anything else. Keep writing `.aup` as
   well until the read path is proven.
4. **C++ read path.** Load `.rsp` into `WaveTrack`, `WaveClip`, `Sequence`,
   `Envelope` and `ProjectSettings` through the getters, and stop writing the
   `.aup` for fully supported projects.
5. **Fold in the remaining track types and project state.** Labels, notes,
   time tracks, aliases and the tag/view details, until the C++ model no longer
   needs its XML serializers for the common cases.
6. **Undo and removal.** Move the undo/history state into the Rust model, then
   delete the C++ block-file path and the `.aup` writer, keeping the `.aup`
   reader as an importer.

## Testing

- Backend unit tests: section round trip, version 1 and version 2 `.rsq`,
  truncated header, bad length, unknown section, duplicate ids, missing clip,
  missing sidecar, empty project, maximum frame counts, Unicode track and tag
  names.
- A golden byte dump of one small project, compared like the DSP goldens, so
  the encoding cannot drift silently.
- `scripts/check_backend_abi.py` for the new entry points, `cargo test
  --workspace`, `clippy -D warnings` and `cargo fmt --check` for the backend
  crate, and the CMake build on all three platforms.
- Manual smoke test before the format becomes the default: WAV import, edit,
  save, reopen, Save As, Save Copy, AutoSave recovery, a legacy `.aup` project
  opening unchanged, and a project with one unsupported track falling back to
  `.aup`.

## Open questions

- The extension and magic (`rsp` versus something else).
- Referenced sidecars in version 1 versus an embedded content-addressed audio
  store from the start.
- How a stereo pair shares one sequence record, and how external alias files
  are represented.
- Whether compression is ever added, and if so at the container level
  (no dependency exists today).
- How the undo manager's disk-space estimate learns about the new container.
- Where the "unsupported element" boundary is drawn for the first writer, so
  the fallback is uncommon but the format stays small.

/* Rustacity: A Digital Audio Editor
 *
 * Copyright (C) 2026 Sato
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * C ABI of the Rust audio backend.
 *
 * The C++ frontend keeps its project, track and Sequence classes.  A `Sequence`
 * imported through this header stores its samples in Rust and delegates its
 * edit and peak operations here.  Pointers returned by this header are owned
 * by the caller unless stated otherwise; every `_free` function accepts NULL.
 *
 * Contributor: Sato <seven3rs@gmail.com>
 */

#ifndef __RUSTACITY_BACKEND_H__
#define __RUSTACITY_BACKEND_H__

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* One mono sample buffer.  Create one with rustacity_import_take_sequence or
 * rustacity_sequence_clone, and release it with rustacity_sequence_free. */
typedef struct rustacity_sequence rustacity_sequence;

/* A decoded WAV.  The importer takes one sequence per channel, then frees the
 * container with rustacity_import_free. */
typedef struct rustacity_import rustacity_import;

/* Decode a UTF-8 WAV path.  Returns NULL on a missing file, an unsupported
 * encoding, or a malformed container. */
rustacity_import *rustacity_wav_import_open(const char *utf8_path);
void rustacity_import_free(rustacity_import *import);

/* Number of mono channel sequences in the decoded file. */
size_t rustacity_import_channel_count(const rustacity_import *import);

/* Sample rate of the decoded file, or zero for a NULL handle. */
uint32_t rustacity_import_sample_rate(const rustacity_import *import);

/* Transfer ownership of one channel's sequence to the caller.  A second call
 * for the same channel returns NULL. */
rustacity_sequence *rustacity_import_take_sequence(
   rustacity_import *import, size_t channel);

/* Deep copy; the caller owns the returned handle. */
rustacity_sequence *rustacity_sequence_clone(const rustacity_sequence *sequence);

/* Copy a frame range; the caller owns the returned handle. */
rustacity_sequence *rustacity_sequence_slice(
   const rustacity_sequence *sequence, uint64_t start, uint64_t len);

void rustacity_sequence_free(rustacity_sequence *sequence);

/* Frames in the sequence. */
uint64_t rustacity_sequence_frames(const rustacity_sequence *sequence);

/* Sample rate stored with the sequence, or zero for a NULL handle. */
uint32_t rustacity_sequence_sample_rate(const rustacity_sequence *sequence);

/* Build a silent sequence; the caller owns the returned handle. */
rustacity_sequence *rustacity_sequence_new_silence(
   uint64_t frames, uint32_t sample_rate);

/* Write/read the versioned `.rsq` sidecar used by the C++ project format.
 * Both use a UTF-8 path.  Loading returns NULL for a missing, truncated or
 * unsupported file.  Returns 1 on success. */
int rustacity_sequence_save(
   const rustacity_sequence *sequence, const char *utf8_path);
rustacity_sequence *rustacity_sequence_load(const char *utf8_path);

/* Copy up to `frames` mono samples into `out`; returns frames written. */
size_t rustacity_sequence_read_f32(
   const rustacity_sequence *sequence, uint64_t start, size_t frames, float *out);

/* Replace `frames` samples at `start`, growing the sequence if needed.
 * Returns 1 on success. */
int rustacity_sequence_write_f32(
   rustacity_sequence *sequence, uint64_t start, size_t frames, const float *input);

/* Append `frames` samples.  Returns 1 on success. */
int rustacity_sequence_append_f32(
   rustacity_sequence *sequence, size_t frames, const float *input);

/* Insert `frames` samples at `start`.  Returns 1 on success. */
int rustacity_sequence_insert_f32(
   rustacity_sequence *sequence, uint64_t start, size_t frames, const float *input);

/* Remove / insert / zero a frame range.  Return 1 on success. */
int rustacity_sequence_delete(
   rustacity_sequence *sequence, uint64_t start, uint64_t len);
int rustacity_sequence_insert_silence(
   rustacity_sequence *sequence, uint64_t start, uint64_t len);
int rustacity_sequence_set_silence(
   rustacity_sequence *sequence, uint64_t start, uint64_t len);

/* Min/max over a frame range.  Returns 1 on success. */
int rustacity_sequence_min_max(
   const rustacity_sequence *sequence, uint64_t start, uint64_t len,
   float *min, float *max);

/* RMS over a frame range; zero for a NULL handle or an empty range. */
float rustacity_sequence_rms(
   const rustacity_sequence *sequence, uint64_t start, uint64_t len);

/* Fill `width + 1` entries of `where` and one column of min/max/rms/bl.
 *
 * `where` is in clip-local sample frames.  The backend computes it from `t0`
 * and `pixels_per_second`; `t0` is clip-relative seconds.  Returns 1 on
 * success. */
int rustacity_sequence_wave_display(
   const rustacity_sequence *sequence, double t0, double pixels_per_second,
   size_t width, uint64_t *where, float *min, float *max, float *rms, int *bl);

/* Same as rustacity_sequence_wave_display, but uses the caller's `where`
 * array (the fisheye path).  Returns 1 on success. */
int rustacity_sequence_wave_display_where(
   const rustacity_sequence *sequence, const uint64_t *where, size_t width,
   float *min, float *max, float *rms, int *bl);

#ifdef __cplusplus
}
#endif

#endif /* __RUSTACITY_BACKEND_H__ */

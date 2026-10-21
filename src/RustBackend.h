/**********************************************************************

  Rustacity: A Digital Audio Editor

  RustBackend.h

  Copyright (C) 2026 Sato

  SPDX-License-Identifier: GPL-2.0-or-later

  Thin C++ ownership wrappers around rust/backend's C ABI.  The wxWidgets
  frontend keeps its project, track and Sequence classes; a Sequence that
  came from the Rust WAV importer holds a RustSequence here instead of
  block files.

**********************************************************************/

#ifndef __RUSTACITY_RUSTBACKEND__
#define __RUSTACITY_RUSTBACKEND__

#include "rustacity/Types.h"

#include "rustacity_backend.h"

#include <cstddef>
#include <cstdint>
#include <memory>
#include <utility>

class RustSequence final
{
public:
   explicit RustSequence(rustacity_sequence *handle);
   ~RustSequence();

   RustSequence(const RustSequence &) = delete;
   RustSequence &operator=(const RustSequence &) = delete;

   rustacity_sequence *Handle() const { return mHandle; }

   std::shared_ptr<RustSequence> Clone() const;
   std::shared_ptr<RustSequence> Slice(uint64_t start, uint64_t len) const;

   uint64_t Frames() const;
   uint32_t SampleRate() const;
   size_t Read(uint64_t start, size_t len, float *out) const;
   bool Save(const FilePath &path) const;
   void Write(uint64_t start, const float *input, size_t len);
   void Append(const float *input, size_t len);
   void Insert(uint64_t start, const float *input, size_t len);
   void Delete(uint64_t start, uint64_t len);
   void InsertSilence(uint64_t start, uint64_t len);
   void SetSilence(uint64_t start, uint64_t len);
   std::pair<float, float> MinMax(uint64_t start, uint64_t len) const;
   float Rms(uint64_t start, uint64_t len) const;
   bool WaveDisplay(double t0, double pixelsPerSecond, size_t width,
      uint64_t *where, float *min, float *max, float *rms, int *bl) const;
   bool WaveDisplayWhere(const uint64_t *where, size_t width,
      float *min, float *max, float *rms, int *bl) const;

private:
   rustacity_sequence *mHandle{};
};

class RustImport final
{
public:
   explicit RustImport(rustacity_import *handle);
   ~RustImport();

   RustImport(const RustImport &) = delete;
   RustImport &operator=(const RustImport &) = delete;

   size_t ChannelCount() const;
   uint32_t SampleRate() const;
   std::shared_ptr<RustSequence> TakeSequence(size_t channel);

private:
   rustacity_import *mHandle{};
};

class RustBackend final
{
public:
   // Decode a WAV into one RustSequence per channel.  Returns null when the
   // file is not a WAV or cannot be decoded, so the caller can fall back to
   // the legacy libsndfile path.
   static std::unique_ptr<RustImport> ImportWav(const FilePath &path);

   // Load a `.rsq` sidecar written by RustSequence::Save.  Returns null for a
   // missing, truncated or unsupported file.
   static std::shared_ptr<RustSequence> LoadSequence(const FilePath &path);

   // Build a silent sequence for callers that need an empty clip.
   static std::shared_ptr<RustSequence> SilenceSequence(
      uint64_t frames, uint32_t sampleRate);
};

#endif // __RUSTACITY_RUSTBACKEND__

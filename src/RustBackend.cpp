/**********************************************************************

  Rustacity: A Digital Audio Editor

  RustBackend.cpp

  Copyright (C) 2026 Sato

  SPDX-License-Identifier: GPL-2.0-or-later

**********************************************************************/

#include "RustBackend.h"

#include <wx/string.h>

RustSequence::RustSequence(rustacity_sequence *handle)
   : mHandle(handle)
{
}

RustSequence::~RustSequence()
{
   rustacity_sequence_free(mHandle);
}

std::shared_ptr<RustSequence> RustSequence::Clone() const
{
   auto *handle = rustacity_sequence_clone(mHandle);
   if (!handle)
      return nullptr;
   return std::make_shared<RustSequence>(handle);
}

std::shared_ptr<RustSequence> RustSequence::Slice(
   uint64_t start, uint64_t len) const
{
   auto *handle = rustacity_sequence_slice(mHandle, start, len);
   if (!handle)
      return nullptr;
   return std::make_shared<RustSequence>(handle);
}

uint64_t RustSequence::Frames() const
{
   return rustacity_sequence_frames(mHandle);
}

size_t RustSequence::Read(uint64_t start, size_t len, float *out) const
{
   return rustacity_sequence_read_f32(mHandle, start, len, out);
}

void RustSequence::Write(uint64_t start, const float *input, size_t len)
{
   rustacity_sequence_write_f32(mHandle, start, len, input);
}

void RustSequence::Append(const float *input, size_t len)
{
   rustacity_sequence_append_f32(mHandle, len, input);
}

void RustSequence::Insert(uint64_t start, const float *input, size_t len)
{
   rustacity_sequence_insert_f32(mHandle, start, len, input);
}

void RustSequence::Delete(uint64_t start, uint64_t len)
{
   rustacity_sequence_delete(mHandle, start, len);
}

void RustSequence::InsertSilence(uint64_t start, uint64_t len)
{
   rustacity_sequence_insert_silence(mHandle, start, len);
}

void RustSequence::SetSilence(uint64_t start, uint64_t len)
{
   rustacity_sequence_set_silence(mHandle, start, len);
}

std::pair<float, float> RustSequence::MinMax(
   uint64_t start, uint64_t len) const
{
   float min = 0.0f;
   float max = 0.0f;
   if (!rustacity_sequence_min_max(mHandle, start, len, &min, &max))
      return { 0.0f, 0.0f };
   return { min, max };
}

float RustSequence::Rms(uint64_t start, uint64_t len) const
{
   return rustacity_sequence_rms(mHandle, start, len);
}

bool RustSequence::WaveDisplay(
   double t0, double pixelsPerSecond, size_t width,
   uint64_t *where, float *min, float *max, float *rms, int *bl) const
{
   return rustacity_sequence_wave_display(
      mHandle, t0, pixelsPerSecond, width, where, min, max, rms, bl) != 0;
}

bool RustSequence::WaveDisplayWhere(
   const uint64_t *where, size_t width,
   float *min, float *max, float *rms, int *bl) const
{
   return rustacity_sequence_wave_display_where(
      mHandle, where, width, min, max, rms, bl) != 0;
}

RustImport::RustImport(rustacity_import *handle)
   : mHandle(handle)
{
}

RustImport::~RustImport()
{
   rustacity_import_free(mHandle);
}

size_t RustImport::ChannelCount() const
{
   return rustacity_import_channel_count(mHandle);
}

uint32_t RustImport::SampleRate() const
{
   return rustacity_import_sample_rate(mHandle);
}

std::shared_ptr<RustSequence> RustImport::TakeSequence(size_t channel)
{
   auto *handle = rustacity_import_take_sequence(mHandle, channel);
   if (!handle)
      return nullptr;
   return std::make_shared<RustSequence>(handle);
}

std::unique_ptr<RustImport> RustBackend::ImportWav(const FilePath &path)
{
   const auto utf8 = path.utf8_str();
   auto *handle = rustacity_wav_import_open(utf8.data());
   if (!handle)
      return nullptr;
   return std::make_unique<RustImport>(handle);
}

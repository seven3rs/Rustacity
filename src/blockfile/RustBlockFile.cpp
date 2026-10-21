/**********************************************************************

  Rustacity: A Digital Audio Editor

  RustBlockFile.cpp

  Copyright (C) 2026 Sato

  SPDX-License-Identifier: GPL-2.0-or-later

**********************************************************************/

#include "../Rustacity.h"
#include "RustBlockFile.h"

#include "../DirManager.h"
#include "../FileException.h"
#include "../InconsistencyException.h"
#include "../Internat.h"
#include "../RustBackend.h"

#include <algorithm>
#include <cstring>
#include <limits>
#include <utility>
#include <vector>

#include <wx/file.h>
#include <wx/log.h>

namespace {
void CopyFloatToSamples(
   const std::vector<float> &samples, samplePtr data, sampleFormat format)
{
   if (samples.empty())
      return;
   if (format == floatSample)
      std::memcpy(data, samples.data(), samples.size() * sizeof(float));
   else
      CopySamples((samplePtr)samples.data(), floatSample, data, format,
         static_cast<unsigned int>(samples.size()));
}
}

RustBlockFile::RustBlockFile(
   wxFileNameWrapper &&baseFileName, std::shared_ptr<RustSequence> sequence)
   : BlockFile{
        (baseFileName.SetExt(wxT("rsq")), std::move(baseFileName)),
        sequence ? static_cast<size_t>(sequence->Frames()) : size_t{ 0 }
     }
{
   if (!sequence)
      throw FileException{ FileException::Cause::Write, mFileName };

   mSampleRate = sequence->SampleRate();
   SaveToDisk(sequence);
}

RustBlockFile::RustBlockFile(
   wxFileNameWrapper &&existingFile, size_t len,
   float min, float max, float rms, uint32_t sampleRate)
   : BlockFile{ std::move(existingFile), len }
   , mSampleRate{ sampleRate ? sampleRate : 44100 }
{
   mMin = min;
   mMax = max;
   mRMS = rms;

   if (mFileName.HasName())
      mSequence = RustBackend::LoadSequence(mFileName.GetFullPath());
   if (mSequence) {
      const auto frames = static_cast<size_t>(mSequence->Frames());
      if (frames != mLen) {
         const auto name = mFileName.GetFullName();
         wxLogWarning(
            wxT("Rust sequence file %s has %llu frames, but the project says %llu. Using the file."),
            name.wx_str(),
            static_cast<unsigned long long>(frames),
            static_cast<unsigned long long>(mLen));
         mLen = frames;
         mSummaryInfo = SummaryInfo(mLen);
      }
      mSampleRate = mSequence->SampleRate();

      // Derive the summary from the file, so a stale project attribute cannot
      // disagree with the samples that are actually loaded.
      const auto extent = mSequence->MinMax(0, mSequence->Frames());
      mMin = extent.first;
      mMax = extent.second;
      mRMS = mSequence->Rms(0, mSequence->Frames());
   }
}

RustBlockFile::~RustBlockFile() = default;

std::shared_ptr<RustSequence> RustBlockFile::LoadedSequence() const
{
   if (mSequence)
      return mSequence;
   return mFileName.HasName()
      ? RustBackend::LoadSequence(mFileName.GetFullPath())
      : nullptr;
}

std::shared_ptr<RustSequence> RustBlockFile::TakeSequence()
{
   if (!mSequence)
      mSequence = LoadedSequence();
   return std::move(mSequence);
}

bool RustBlockFile::ReadSummary(ArrayOf<char> &data)
{
   data.reinit(mSummaryInfo.totalSummaryBytes);
   if (mLen == 0 || !data) {
      if (data)
         std::memset(data.get(), 0, mSummaryInfo.totalSummaryBytes);
      return false;
   }

   const auto sequence = LoadedSequence();
   if (!sequence) {
      std::memset(data.get(), 0, mSummaryInfo.totalSummaryBytes);
      return false;
   }

   std::vector<float> samples(mLen);
   if (sequence->Read(0, mLen, samples.data()) != mLen) {
      std::memset(data.get(), 0, mSummaryInfo.totalSummaryBytes);
      return false;
   }

   ArrayOf<char> cleanup;
   const auto *summary =
      static_cast<const char *>(CalcSummary(
         (samplePtr)samples.data(), mLen, floatSample, cleanup));
   std::memcpy(data.get(), summary, mSummaryInfo.totalSummaryBytes);
   return true;
}

size_t RustBlockFile::ReadData(
   samplePtr data, sampleFormat format, size_t start, size_t len,
   bool mayThrow) const
{
   const auto safeStart = std::min(start, mLen);
   const auto available = std::min(len, mLen - safeStart);
   std::vector<float> samples(available);
   size_t got = 0;
   if (available > 0) {
      const auto sequence = LoadedSequence();
      if (sequence)
         got = sequence->Read(safeStart, available, samples.data());
   }

   if (got > 0)
      CopyFloatToSamples(samples, data, format);

   if (got < len) {
      ClearSamples(data, format, got, len - got);
      if (mayThrow)
         throw FileException{ FileException::Cause::Read, mFileName };
   }
   return got;
}

BlockFilePtr RustBlockFile::Copy(wxFileNameWrapper &&newFileName)
{
   const auto sequence = LoadedSequence();
   if (!sequence)
      THROW_INCONSISTENCY_EXCEPTION;

   return make_blockfile<RustBlockFile>(
      std::move(newFileName), sequence);
}

void RustBlockFile::SaveXML(XMLWriter &xmlFile)
// may throw
{
   xmlFile.StartTag(wxT("rustblockfile"));

   xmlFile.WriteAttr(wxT("filename"), mFileName.GetFullName());
   xmlFile.WriteAttr(wxT("len"), mLen);
   xmlFile.WriteAttr(wxT("min"), mMin);
   xmlFile.WriteAttr(wxT("max"), mMax);
   xmlFile.WriteAttr(wxT("rms"), mRMS);
   xmlFile.WriteAttr(wxT("rate"), static_cast<size_t>(mSampleRate));

   xmlFile.EndTag(wxT("rustblockfile"));
}

auto RustBlockFile::GetSpaceUsage() const -> DiskByteCount
{
   if (!mFileName.HasName())
      return 0;

   wxFile file(mFileName.GetFullPath());
   if (!file.IsOpened())
      return 0;
   const auto length = file.Length();
   return length > 0 ? static_cast<DiskByteCount>(length) : 0;
}

void RustBlockFile::Recover()
{
   auto sequence = LoadedSequence();
   if (!sequence)
      sequence = RustBackend::SilenceSequence(mLen, mSampleRate);
   if (!sequence)
      return;

   SaveToDisk(sequence);
}

void RustBlockFile::SaveToDisk(const std::shared_ptr<RustSequence> &sequence)
{
   if (!sequence)
      THROW_INCONSISTENCY_EXCEPTION;

   if (mFileName.HasName() &&
       !sequence->Save(mFileName.GetFullPath()))
      throw FileException{ FileException::Cause::Write, mFileName };

   mLen = static_cast<size_t>(sequence->Frames());
   mSummaryInfo = SummaryInfo(mLen);
   mSampleRate = sequence->SampleRate();

   const auto extent = sequence->MinMax(0, sequence->Frames());
   mMin = extent.first;
   mMax = extent.second;
   mRMS = sequence->Rms(0, sequence->Frames());
}

BlockFilePtr RustBlockFile::BuildFromXML(DirManager &dm, const wxChar **attrs)
{
   wxFileNameWrapper fileName;
   float min = 0.0f, max = 0.0f, rms = 0.0f;
   size_t len = 0;
   uint32_t rate = 44100;
   double dblValue = 0.0;

   while (*attrs) {
      const wxChar *attr = *attrs++;
      const wxChar *value = *attrs++;
      if (!value)
         break;

      const wxString strValue = value;
      if (!wxStricmp(attr, wxT("filename")) &&
          XMLValueChecker::IsGoodFileString(strValue) &&
          (strValue.length() + 1 + dm.GetProjectDataDir().length()
              <= PLATFORM_MAX_PATH)) {
         if (!dm.AssignFile(fileName, strValue, false))
            fileName.Clear();
      }
      else if (!wxStrcmp(attr, wxT("len")) &&
               XMLValueChecker::IsGoodInt64(strValue)) {
         long long nValue = 0;
         if (strValue.ToLongLong(&nValue) && nValue >= 0 &&
             static_cast<unsigned long long>(nValue)
                <= std::numeric_limits<size_t>::max())
            len = static_cast<size_t>(nValue);
      }
      else if (!wxStrcmp(attr, wxT("rate")) &&
               XMLValueChecker::IsGoodInt(strValue)) {
         long nValue = 0;
         if (strValue.ToLong(&nValue) && nValue > 0 &&
             static_cast<unsigned long long>(nValue) <=
                std::numeric_limits<uint32_t>::max())
            rate = static_cast<uint32_t>(nValue);
      }
      else if (XMLValueChecker::IsGoodString(strValue) &&
               Internat::CompatibleToDouble(strValue, &dblValue)) {
         if (!wxStricmp(attr, wxT("min")))
            min = dblValue;
         else if (!wxStricmp(attr, wxT("max")))
            max = dblValue;
         else if (!wxStricmp(attr, wxT("rms")) && dblValue >= 0.0)
            rms = dblValue;
      }
   }

   return make_blockfile<RustBlockFile>(
      std::move(fileName), len, min, max, rms, rate);
}

static DirManager::RegisteredBlockFileDeserializer sRegistration{
   "rustblockfile",
   [](DirManager &dm, const wxChar **attrs) {
      return RustBlockFile::BuildFromXML(dm, attrs);
   }
};

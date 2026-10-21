/**********************************************************************

  Rustacity: A Digital Audio Editor

  RustBlockFile.h

  Copyright (C) 2026 Sato

  SPDX-License-Identifier: GPL-2.0-or-later

**********************************************************************/

#ifndef __RUSTACITY_RUST_BLOCKFILE__
#define __RUSTACITY_RUST_BLOCKFILE__

#include "../BlockFile.h"

#include <cstdint>
#include <memory>

class DirManager;
class RustSequence;

/// A BlockFile that persists a Rust-backed Sequence as a `.rsq` sidecar.
///
/// The live samples stay in RustSequence.  This class owns the file name and
/// the BlockFile lifecycle, so DirManager, ProjectSetter, ProjectFSCK and the
/// orphan cleanup keep working without materializing legacy `.au` files.
class RustBlockFile final : public BlockFile {
public:
   /// Create the sidecar from an existing Sequence and write it to disk.
   RustBlockFile(wxFileNameWrapper &&baseFileName,
                 std::shared_ptr<RustSequence> sequence);

   /// Refer to an existing sidecar.  The sequence is loaded eagerly; a missing
   /// or malformed file leaves it null so Sequence can fall back to silence.
   RustBlockFile(wxFileNameWrapper &&existingFile, size_t len,
                 float min, float max, float rms, uint32_t sampleRate);

   ~RustBlockFile() override;

   bool ReadSummary(ArrayOf<char> &data) override;
   size_t ReadData(samplePtr data, sampleFormat format,
                   size_t start, size_t len, bool mayThrow = true) const override;

   BlockFilePtr Copy(wxFileNameWrapper &&newFileName) override;
   void SaveXML(XMLWriter &xmlFile) override;

   DiskByteCount GetSpaceUsage() const override;
   void Recover() override;

   /// Hand the loaded samples to the live Sequence.  The sidecar itself is a
   /// file reference after this, so the two stores never alias.
   std::shared_ptr<RustSequence> TakeSequence();

   uint32_t GetSampleRate() const { return mSampleRate; }

   static BlockFilePtr BuildFromXML(DirManager &dm, const wxChar **attrs);

private:
   std::shared_ptr<RustSequence> LoadedSequence() const;
   void SaveToDisk(const std::shared_ptr<RustSequence> &sequence);

   /// Filled only while a project is being opened, until Sequence takes it.
   std::shared_ptr<RustSequence> mSequence;
   uint32_t mSampleRate{ 44100 };
};

#endif // __RUSTACITY_RUST_BLOCKFILE__

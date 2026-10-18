/**********************************************************************

  Rustacity: A Digital Audio Editor

  Extension.cpp

  Contributor: Sadik Pranto <pranto.smss.dev@gmail.com>

*******************************************************************//**

\class EffectExtension
\brief Runs a sandboxed WebAssembly extension over the selected audio.

The extension engine is Rust (rust/ext-host), linked in through the C ABI
in rust/ext-host/include/rustacity_ext.h.  What an extension is, and what
the sandbox promises about one that misbehaves, is in
docs/extensions/{API,SANDBOX}.md.

Three things about this integration are worth knowing up front.

One engine serves the whole process, because loading an extension compiles
its module: the second instance of an effect with the same extension
should not compile it again.  Extensions and instances are not shared
beyond that; each effect invocation gets its own instance, which is what
the ABI means by "one instance per effect invocation".

An extension is handed this effect's blocks, with one channel at a time,
which is the shape GetAudioInCount() below asks the framework for.  An
extension that needs all channels at once - a stereo widener, anything
that looks across channels - cannot be run this way, and would need the
effect to declare two audio ports instead.

Parameters are not exposed yet: the extension runs at the defaults its
manifest declares, which is what makes this an effect with no dialog.  The
host's C ABI already has everything a dialog needs
(rustacity_ext_extension_parameter to describe one,
rustacity_ext_instance_set_param_by_id to set it), so that is a change to
this file rather than to the engine.

What is deliberately not here is registration: one menu entry per
installed extension, the way the LADSPA, LV2 and Nyquist modules register
what they find.  That needs two decisions that are user-visible rather
than mechanical - where extensions are installed, and how a user turns
one on - and inventing them here would be worse than leaving the gap
visible.  Everything else in this file is what such a module would
construct, and Load() below is written for it.

*//********************************************************************/

#include "../Rustacity.h"

#include "Extension.h"

#include <wx/intl.h>

#include <algorithm>
#include <utility>

#include "rustacity_ext.h"

namespace
{

// One engine for the whole process.  It is deliberately never destroyed:
// destroying it requires every extension and instance to be gone first,
// and it holds nothing but compiled modules - no audio, no file handles -
// so there is nothing to release before the process ends.
rustacity_ext_engine *TheEngine()
{
   static rustacity_ext_engine *const engine = []() -> rustacity_ext_engine * {
      rustacity_ext_engine *created = nullptr;
      if (rustacity_ext_engine_create(nullptr, &created) != RUSTACITY_EXT_OK)
         return nullptr;
      return created;
   }();
   return engine;
}

// What the host last said went wrong.  The pointer belongs to the host and
// stays valid until the next call on this thread, so this copies it.
wxString LastError()
{
   const char *message = rustacity_ext_last_error();
   return message ? wxString::FromUTF8(message) : wxString{};
}

}

EffectExtension::EffectExtension(const std::string &directory)
   : mDirectory(directory)
{
   auto engine = TheEngine();
   if (!engine)
   {
      mError = XO("The extension engine could not be started.").Translation();
      return;
   }

   if (rustacity_ext_engine_load(engine, mDirectory.c_str(), &mExtension) != RUSTACITY_EXT_OK)
   {
      mError = LastError();
      mExtension = nullptr;
      return;
   }

   const char *text = nullptr;
   if (rustacity_ext_extension_id(mExtension, &text) == RUSTACITY_EXT_OK && text)
      // The manifest's id, not its display name: this one persists in
      // configuration files and must not follow a rename.
      mIdentifier = wxString::FromUTF8(text);
   if (rustacity_ext_extension_name(mExtension, &text) == RUSTACITY_EXT_OK && text)
      mName = wxString::FromUTF8(text);
   if (rustacity_ext_extension_description(mExtension, &text) == RUSTACITY_EXT_OK && text)
      mDescription = wxString::FromUTF8(text);
}

EffectExtension::~EffectExtension()
{
   StopInstance();
   if (mExtension)
      rustacity_ext_extension_destroy(mExtension);
}

std::unique_ptr<EffectExtension>
EffectExtension::Load(const std::string &directory, TranslatableString &error)
{
   auto effect = std::make_unique<EffectExtension>(directory);
   if (!effect->Loaded())
   {
      error = Verbatim(effect->mError);
      return nullptr;
   }
   return effect;
}

// ComponentInterface implementation

ComponentInterfaceSymbol EffectExtension::GetSymbol()
{
   return ComponentInterfaceSymbol{ Identifier{ mIdentifier }, Verbatim(mName) };
}

TranslatableString EffectExtension::GetDescription()
{
   // An extension that would not load is described by why.
   if (!Loaded())
      return Verbatim(mError);
   return mDescription.empty() ? Verbatim(mName) : Verbatim(mDescription);
}

// EffectDefinitionInterface implementation

EffectType EffectExtension::GetType()
{
   return EffectTypeProcess;
}

bool EffectExtension::IsInteractive()
{
   // No dialog: see the note about parameters at the top.
   return false;
}

// EffectClientInterface implementation

unsigned EffectExtension::GetAudioInCount()
{
   return 1;
}

unsigned EffectExtension::GetAudioOutCount()
{
   return 1;
}

size_t EffectExtension::SetBlockSize(size_t maxBlockSize)
{
   // The module ABI caps a call at MaximumBlockFrames, and an instance is
   // told the largest block it will be given when it is created, so this is
   // where the framework's preference meets that limit.
   mBlockSize = std::min(maxBlockSize, MaximumBlockFrames);
   return mBlockSize;
}

size_t EffectExtension::ProcessBlock(float **inBlock, float **outBlock, size_t blockLen)
{
   if (mFailed)
      return 0;

   if (blockLen == 0)
      return 0;

   if (blockLen > MaximumBlockFrames)
   {
      Fail(XO("This effect cannot process %lld frames at a time.").Format(
         static_cast<long long>(blockLen)));
      return 0;
   }

   if (!StartInstance())
      return 0;

   const float *inputs[1]{ inBlock[0] };
   float *outputs[1]{ outBlock[0] };

   if (rustacity_ext_instance_process(
          mInstance, inputs, outputs, 1, static_cast<int>(blockLen)) != RUSTACITY_EXT_OK)
   {
      Fail(LastError());
      return 0;
   }

   return blockLen;
}

// Effect implementation

bool EffectExtension::Process()
{
   // A fresh run.  An extension keeps state between blocks, so the instance
   // is put back where it started rather than continued from the last run,
   // whether or not the framework reuses this object.
   StopInstance();

   if (!Loaded())
   {
      Fail(mError);
      return false;
   }

   return Effect::Process();
}

bool EffectExtension::ProcessPass()
{
   const bool result = Effect::ProcessPass();

   // A block that could not be processed leaves the output for one part of
   // the selection in no state to keep, and the framework's way of saying
   // so is a false from here: it discards everything this run produced.
   return result && !mFailed;
}

// EffectExtension implementation

bool EffectExtension::StartInstance()
{
   const auto rate = static_cast<float>(mSampleRate);
   if (mInstance && (mInstanceRate != rate || mInstanceBlock != mBlockSize))
      StopInstance();

   if (mInstance)
      return true;

   if (rustacity_ext_instance_create(
          mExtension, rate, static_cast<int>(mBlockSize), 1, &mInstance) != RUSTACITY_EXT_OK)
   {
      mInstance = nullptr;
      Fail(LastError());
      return false;
   }

   mInstanceRate = rate;
   mInstanceBlock = mBlockSize;
   return true;
}

void EffectExtension::StopInstance()
{
   if (mInstance)
   {
      // An instance that trapped or timed out is dropped without being
      // asked anything else, which is what the host expects.
      rustacity_ext_instance_destroy(mInstance);
      mInstance = nullptr;
   }
}

void EffectExtension::Fail(const wxString &reason)
{
   if (mFailed)
      return;

   mFailed = true;
   MessageBox(
      Verbatim(reason.empty()
         ? XO("The extension could not finish this effect.").Translation()
         : reason),
      wxOK | wxICON_ERROR);
}

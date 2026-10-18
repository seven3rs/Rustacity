/**********************************************************************

  Rustacity: A Digital Audio Editor

  Extension.h

  Contributor: Sadik Pranto <pranto.smss.dev@gmail.com>

  Runs a sandboxed WebAssembly extension over the selected audio.  The
  engine is the Rust library built from rust/ext-host, reached through
  rust/ext-host/include/rustacity_ext.h; the contract an extension is
  written against is docs/extensions/API.md, and what the sandbox allows
  is docs/extensions/SANDBOX.md.

**********************************************************************/

#ifndef __RUSTACITY_EFFECT_EXTENSION__
#define __RUSTACITY_EFFECT_EXTENSION__

#include "Effect.h"

#include <memory>
#include <string>

// The handles of the extension host, which this effect owns.
struct rustacity_ext_extension;
struct rustacity_ext_instance;

class EffectExtension final : public Effect
{
public:
   // The most frames one call into a module may carry, as
   // rust/ext-api/src/abi.rs sets it.  SetBlockSize below clamps to this,
   // so the instance the extension is told about is the one it gets.
   static constexpr size_t MaximumBlockFrames = 65536;

   // `directory` names an extension directory - the two files
   // extension.toml and extension.wasm - as a UTF-8 path.  A directory
   // that cannot be loaded leaves the effect refusing to run and holding
   // the reason; Load() is the same thing for a caller that has somewhere
   // to put the reason and would rather not build the object at all.
   explicit EffectExtension(const std::string &directory);
   virtual ~EffectExtension();

   // Null, and `error` set, when the extension cannot be loaded.  This is
   // the form the plug-in modules use when they discover what is
   // installed, and the form a module for extensions will use.
   static std::unique_ptr<EffectExtension>
   Load(const std::string &directory, TranslatableString &error);

   // ComponentInterface implementation

   ComponentInterfaceSymbol GetSymbol() override;
   TranslatableString GetDescription() override;

   // EffectDefinitionInterface implementation

   EffectType GetType() override;
   bool IsInteractive() override;

   // EffectClientInterface implementation

   unsigned GetAudioInCount() override;
   unsigned GetAudioOutCount() override;
   size_t SetBlockSize(size_t maxBlockSize) override;
   size_t ProcessBlock(float **inBlock, float **outBlock, size_t blockLen) override;

private:
   // Effect implementation

   bool Process() override;
   bool ProcessPass() override;

   // EffectExtension implementation

   bool Loaded() const { return mExtension != nullptr; }

   // One running copy of the extension, for the sample rate and block size
   // the framework has settled on, created at the first block and kept for
   // the rest of the run.
   bool StartInstance();
   void StopInstance();

   // Remembers that this run cannot go on, tells the user once, and makes
   // ProcessPass report it: what the framework does with a failure is
   // discard the output, which is what the API promises an extension that
   // traps or times out.
   void Fail(const TranslatableString &reason);

   std::string mDirectory;
   wxString mIdentifier;
   wxString mName;
   wxString mDescription;
   wxString mError;

   rustacity_ext_extension *mExtension{ nullptr };
   rustacity_ext_instance *mInstance{ nullptr };
   float mInstanceRate{ 0.0f };
   size_t mInstanceBlock{ 0 };
   bool mFailed{ false };
};

#endif

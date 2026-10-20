/**********************************************************************

  Rustacity: A Digital Audio Editor

  rustacity_ext.h

  Contributor: Sato <seven3rs@gmail.com>

  C ABI of the sandboxed extension host (crate rustacity-ext-host under
  rust/ext-host).  The declarations below mirror the implementations in
  rust/ext-host/src/ffi.rs; scripts/check_ext_abi.py fails the build
  when the two drift apart.

  An extension is a directory holding extension.toml and extension.wasm.
  docs/extensions/API.md is the contract those two are written against;
  this header is only the way the editor talks to the host that runs
  them.

  Rules that hold for every function here:

    * The return value is 0 on success, or one of the
      RUSTACITY_EXT_ERROR_* codes below.
    * After a failure, rustacity_ext_last_error() returns a message
      describing it.  The pointer belongs to the library; copy it if you
      need it after the next host call on the same thread.
    * Handles are opaque.  Destroy each with the function named after it,
      and destroy instances before the extension they came from.
    * An extension runs offline, in one instance per invocation, with a
      wall-clock deadline and a memory ceiling.  Nothing it can reach
      through WASI is real: see docs/extensions/SANDBOX.md.

***********************************************************************/

#ifndef __RUSTACITY_EXT_H__
#define __RUSTACITY_EXT_H__

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Status codes, mirroring ExtError::status() in rust/ext-api/src/error.rs.
 * They are published: docs/extensions/API.md section 8 has the table. */
#define RUSTACITY_EXT_OK 0
#define RUSTACITY_EXT_ERROR_INTERNAL 1
#define RUSTACITY_EXT_ERROR_MANIFEST_INVALID 2
#define RUSTACITY_EXT_ERROR_MODULE_INVALID 3
#define RUSTACITY_EXT_ERROR_ABI_MISMATCH 4
#define RUSTACITY_EXT_ERROR_CAPABILITY_DENIED 5
#define RUSTACITY_EXT_ERROR_TRAP 6
#define RUSTACITY_EXT_ERROR_TIMEOUT 7
#define RUSTACITY_EXT_ERROR_OUT_OF_MEMORY 8
#define RUSTACITY_EXT_ERROR_MODULE_REPORTED 9
#define RUSTACITY_EXT_ERROR_IO 10
#define RUSTACITY_EXT_ERROR_POISONED 11
#define RUSTACITY_EXT_ERROR_INVALID_ARGUMENT 12

/* Parameter kinds, for rustacity_ext_parameter_t::kind. */
#define RUSTACITY_EXT_PARAM_FLOAT 0
#define RUSTACITY_EXT_PARAM_BOOL 1
#define RUSTACITY_EXT_PARAM_CHOICE 2

/* The API version this host implements.  A module that does not answer
 * the same number when its rx_api_version() is called is refused. */
#define RUSTACITY_EXT_API_VERSION 1

/* Four limits, because those are the four an application might have a
 * reason to change.  A field left at zero means "the documented
 * default": 256 MiB of memory, a 5000 ms deadline for one call, blocks
 * of up to 65536 frames, and 256 collected log records.
 *
 * The deadline is a true wall clock: the engine interrupts the module at
 * the next check after the budget is spent, and an interrupted instance
 * is spent with it.  Asking for a longer one is a decision, not a
 * tuning knob. */
typedef struct rustacity_ext_host_config
{
   size_t max_memory_bytes;
   uint64_t timeout_ms;
   int max_block_frames;
   int log_capacity;
} rustacity_ext_host_config_t;

/* What an extension says about one of its parameters.
 *
 * Every pointer here belongs to the extension handle and stays valid
 * until it is destroyed: do not free them.  unit and description are
 * NULL when the manifest leaves them out; choices is NULL for anything
 * that is not a choice. */
typedef struct rustacity_ext_parameter
{
   const char *id;
   const char *name;
   const char *unit;
   const char *description;
   int kind;                    /* RUSTACITY_EXT_PARAM_* */
   float minimum;               /* float only */
   float maximum;               /* float only */
   float default_value;         /* a float, 0.0/1.0, or a choice index */
   int choice_count;            /* choice only */
   const char *const *choices;  /* choice only */
} rustacity_ext_parameter_t;

typedef struct rustacity_ext_engine rustacity_ext_engine_t;
typedef struct rustacity_ext_extension rustacity_ext_extension_t;
typedef struct rustacity_ext_instance rustacity_ext_instance_t;

/* ------------------------------------------------------------------
 * The engine
 * ------------------------------------------------------------------ */

/* Creates an engine.  config may be NULL for every default.  One engine
 * serves the whole application: loading an extension from it compiles
 * the module, so it is worth keeping. */
int rustacity_ext_engine_create(const rustacity_ext_host_config_t *config,
                                rustacity_ext_engine_t **out);

/* Destroys an engine and everything loaded from it.  All extensions and
 * instances made from it must be destroyed first. */
void rustacity_ext_engine_destroy(rustacity_ext_engine_t *engine);

/* Loads the extension in `directory`: reads and validates
 * extension.toml, compiles extension.wasm, checks its imports, and
 * builds a probe instance so that a missing export, a mistyped one, or a
 * module claiming another API version is refused here rather than in the
 * middle of an effect.
 *
 * `directory` is a UTF-8 path, on every platform. */
int rustacity_ext_engine_load(const rustacity_ext_engine_t *engine,
                              const char *directory,
                              rustacity_ext_extension_t **out);

/* Destroys a loaded extension.  Instances made from it must be
 * destroyed first. */
void rustacity_ext_extension_destroy(rustacity_ext_extension_t *extension);

/* ------------------------------------------------------------------
 * What a loaded extension says about itself
 *
 * The strings written out are owned by the extension: they stay valid
 * until it is destroyed.
 * ------------------------------------------------------------------ */

int rustacity_ext_extension_id(const rustacity_ext_extension_t *extension,
                               const char **out);
int rustacity_ext_extension_name(const rustacity_ext_extension_t *extension,
                                 const char **out);
int rustacity_ext_extension_version(const rustacity_ext_extension_t *extension,
                                    const char **out);
/* NULL when the manifest has no description. */
int rustacity_ext_extension_description(const rustacity_ext_extension_t *extension,
                                        const char **out);
/* The directory it was loaded from. */
int rustacity_ext_extension_path(const rustacity_ext_extension_t *extension,
                                 const char **out);

/* How many parameters the extension has.  This is the length of the
 * arrays that set_param() and parameter() index, and the order in the
 * manifest is the order the module sees in rx_set_param(). */
int rustacity_ext_extension_parameter_count(const rustacity_ext_extension_t *extension,
                                            int *out);

/* Describes parameter `index`, which must be in 0..count. */
int rustacity_ext_extension_parameter(const rustacity_ext_extension_t *extension,
                                      int index,
                                      rustacity_ext_parameter_t *out);

/* ------------------------------------------------------------------
 * Running an extension
 * ------------------------------------------------------------------ */

/* Prepares one running copy of the extension for `sample_rate` and
 * `channels`, with blocks of at most `max_block` frames.  One instance
 * per effect invocation, and destroy it when the effect is done: the
 * extension's own buffers live inside it.
 *
 * The extension must hold the audio.read and audio.write capabilities,
 * or this fails with RUSTACITY_EXT_ERROR_CAPABILITY_DENIED. */
int rustacity_ext_instance_create(const rustacity_ext_extension_t *extension,
                                  float sample_rate,
                                  int max_block,
                                  int channels,
                                  rustacity_ext_instance_t **out);

/* Destroys an instance, giving the extension's buffers back to it.  An
 * instance that trapped or timed out is dropped without being asked. */
void rustacity_ext_instance_destroy(rustacity_ext_instance_t *instance);

/* Sets parameter `index` to `value`, in the manifest's order.  Values
 * outside the manifest's range, or of the wrong shape for the kind, are
 * refused with RUSTACITY_EXT_ERROR_INVALID_ARGUMENT: the host does not
 * clamp, so that a caller can tell the difference between "as asked" and
 * "as allowed".  A switch takes 0.0 or 1.0, a choice its index. */
int rustacity_ext_instance_set_param(const rustacity_ext_instance_t *instance,
                                     int index,
                                     float value);
int rustacity_ext_instance_set_param_by_id(const rustacity_ext_instance_t *instance,
                                           const char *id,
                                           float value);
/* Every parameter to the default the manifest declares. */
int rustacity_ext_instance_set_default_parameters(const rustacity_ext_instance_t *instance);

/* Sends one block through the extension: `channels` pointers to
 * `frames` floats in, and `channels` pointers to `frames` floats out.
 * Both are planar, and the two arrays may name the same buffers.
 *
 * The extension is told the sample rate, this channel count and this
 * maximum block at creation; frames must be 1..that maximum. */
int rustacity_ext_instance_process(const rustacity_ext_instance_t *instance,
                                   const float *const *input,
                                   float *const *output,
                                   int channels,
                                   int frames);

/* Returns the extension to the state it was created in.  A module that
 * does not export rx_reset has nothing to reset, and this succeeds. */
int rustacity_ext_instance_reset(const rustacity_ext_instance_t *instance);

/* Sets *out to 1 when a trap or a timeout has spent this instance, and
 * it must be destroyed rather than used again.  Everything it knew is
 * still there for the purpose of describing what happened. */
int rustacity_ext_instance_is_poisoned(const rustacity_ext_instance_t *instance,
                                       int *out);

/* ------------------------------------------------------------------
 * What the extension logged
 *
 * rx_log is a debugging aid and is rate-limited.  These functions
 * collect what the extension has written since the last collection and
 * then hand it out; the text stays valid until the next collection.
 * ------------------------------------------------------------------ */

/* Collects the new records and sets *out to how many there now are. */
int rustacity_ext_instance_collect_logs(const rustacity_ext_instance_t *instance,
                                        int *out);

/* The same, as a convenient way to ask "how many are there now". */
int rustacity_ext_instance_log_count(const rustacity_ext_instance_t *instance,
                                     int *out);

/* One collected record: 0 trace, 1 debug, 2 info, 3 warn, 4 error. */
int rustacity_ext_instance_log(const rustacity_ext_instance_t *instance,
                               int index,
                               int *level,
                               const char **text);

/* How many records were dropped for want of budget or room. */
int rustacity_ext_instance_dropped_logs(const rustacity_ext_instance_t *instance,
                                        uint64_t *out);

/* ------------------------------------------------------------------
 * Diagnostics
 * ------------------------------------------------------------------ */

/* The message from the last call that failed on this thread, or an empty
 * string.  The pointer belongs to the library and stays valid until the
 * next host call on this thread sets or clears it. */
const char *rustacity_ext_last_error(void);

/* RUSTACITY_EXT_API_VERSION, for a build that would rather ask than
 * assume. */
int rustacity_ext_api_version(void);

#ifdef __cplusplus
}
#endif

#endif /* __END__ */

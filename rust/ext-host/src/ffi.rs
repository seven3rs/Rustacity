// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The C ABI the application links against.
//!
//! The header that declares all of this is `include/rustacity_ext.h`, and the
//! two are checked against each other by `scripts/check_ext_abi.py`. The rules
//! are the same on both sides:
//!
//! * every function returns an `int` status - 0 for success, otherwise the
//!   number from [`ExtError::status`];
//! * a failing call leaves a message for [`rustacity_ext_last_error`];
//! * a panic never crosses the boundary: each function runs inside
//!   `catch_unwind` and a panic becomes status 1, the internal error;
//! * handles are opaque and are destroyed by the function named after them.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rustacity_ext_api::{ExtError, ParameterKind};

use crate::engine::{Engine, Extension, Instance};
use crate::limits::HostConfig;

/// The parameter is a number.
pub const RUSTACITY_EXT_PARAM_FLOAT: c_int = 0;
/// The parameter is a switch: 0.0 or 1.0.
pub const RUSTACITY_EXT_PARAM_BOOL: c_int = 1;
/// The parameter is an index into a list of names.
pub const RUSTACITY_EXT_PARAM_CHOICE: c_int = 2;

/// The limits an engine runs its extensions under, and the room its log gets.
///
/// A field left at zero means "the host's default", which is what
/// `docs/extensions/API.md` documents: 256 MiB, 5000 ms, 65536 frames, and 256
/// log records.
#[repr(C)]
pub struct RustacityExtHostConfig {
    pub max_memory_bytes: usize,
    pub timeout_ms: u64,
    pub max_block_frames: c_int,
    pub log_capacity: c_int,
}

/// What an extension says about one of its parameters.
///
/// Every pointer here belongs to the extension handle and stays valid until it
/// is destroyed; nothing here is to be freed by the caller.
#[repr(C)]
pub struct RustacityExtParameter {
    pub id: *const c_char,
    pub name: *const c_char,
    /// May be null.
    pub unit: *const c_char,
    /// May be null.
    pub description: *const c_char,
    /// `RUSTACITY_EXT_PARAM_FLOAT`, `_BOOL` or `_CHOICE`.
    pub kind: c_int,
    /// Meaningful for `_FLOAT` only.
    pub minimum: f32,
    /// Meaningful for `_FLOAT` only.
    pub maximum: f32,
    /// A float, 0.0/1.0 for a switch, an index for a choice.
    pub default_value: f32,
    /// Meaningful for `_CHOICE` only.
    pub choice_count: c_int,
    /// `choice_count` names, for `_CHOICE` only; may be null.
    pub choices: *const *const c_char,
}

/// An engine: opaque to C, created by [`rustacity_ext_engine_create`].
pub struct RustacityExtEngine {
    inner: Engine,
}

/// A loaded extension, with everything C needs already converted.
pub struct RustacityExtExtension {
    inner: Cached,
}

/// One running copy of an extension.
pub struct RustacityExtInstance {
    inner: Mutex<Instance>,
    /// The log records collected for C, filled on demand from the instance.
    logs: Mutex<Vec<CachedLog>>,
}

struct CachedLog {
    level: c_int,
    text: CString,
}

/// A [`Extension`] with its strings in C form.
struct Cached {
    extension: Extension,
    id: CString,
    name: CString,
    version: CString,
    description: Option<CString>,
    unit_of_path: CString,
    parameters: Vec<CachedParameter>,
}

struct CachedParameter {
    id: CString,
    name: CString,
    unit: Option<CString>,
    description: Option<CString>,
    kind: c_int,
    minimum: f32,
    maximum: f32,
    default_value: f32,
    choices: Vec<CString>,
    choice_pointers: Vec<*const c_char>,
}

impl Cached {
    fn new(extension: Extension) -> Result<Self, ExtError> {
        let manifest = extension.manifest().clone();
        let mut parameters = Vec::with_capacity(manifest.parameters.len());
        for parameter in &manifest.parameters {
            let (kind, minimum, maximum, choices) = match &parameter.kind {
                ParameterKind::Float { min, max } => (
                    RUSTACITY_EXT_PARAM_FLOAT,
                    *min as f32,
                    *max as f32,
                    Vec::new(),
                ),
                ParameterKind::Bool => (RUSTACITY_EXT_PARAM_BOOL, 0.0, 1.0, Vec::new()),
                ParameterKind::Choice { choices } => (
                    RUSTACITY_EXT_PARAM_CHOICE,
                    0.0,
                    (choices.len().saturating_sub(1)) as f32,
                    choices.clone(),
                ),
            };
            let cstrings = choices
                .into_iter()
                .map(CString::new)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| ExtError::ManifestInvalid(format!("{error}")))?;
            // Built after the last push, so the pointers stay put.
            let choice_pointers = cstrings.iter().map(|choice| choice.as_ptr()).collect();
            parameters.push(CachedParameter {
                id: CString::new(parameter.id.as_str())
                    .map_err(|error| ExtError::ManifestInvalid(format!("{error}")))?,
                name: CString::new(parameter.name.as_str())
                    .map_err(|error| ExtError::ManifestInvalid(format!("{error}")))?,
                unit: optional_cstring(parameter.unit.as_deref())?,
                description: optional_cstring(parameter.description.as_deref())?,
                kind,
                minimum,
                maximum,
                default_value: parameter.default,
                choices: cstrings,
                choice_pointers,
            });
        }
        Ok(Self {
            id: CString::new(manifest.id.as_str())
                .map_err(|error| ExtError::ManifestInvalid(format!("{error}")))?,
            name: CString::new(manifest.name.as_str())
                .map_err(|error| ExtError::ManifestInvalid(format!("{error}")))?,
            version: CString::new(manifest.version.as_str())
                .map_err(|error| ExtError::ManifestInvalid(format!("{error}")))?,
            description: optional_cstring(manifest.description.as_deref())?,
            unit_of_path: CString::new(extension.path().to_string_lossy().as_bytes())
                .map_err(|error| ExtError::Io(format!("{error}")))?,
            parameters,
            extension,
        })
    }
}

fn optional_cstring(text: Option<&str>) -> Result<Option<CString>, ExtError> {
    match text {
        Some(text) => CString::new(text)
            .map(Some)
            .map_err(|error| ExtError::ManifestInvalid(format!("{error}"))),
        None => Ok(None),
    }
}

/// A pointer for a string that may be absent.
fn or_null(text: Option<&CString>) -> *const c_char {
    match text {
        Some(text) => text.as_ptr(),
        None => std::ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// The last error
// ---------------------------------------------------------------------------

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_last_error(message: &str) {
    let text = CString::new(message.replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|cell| *cell.borrow_mut() = text);
}

fn clear_last_error() {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = CString::default());
}

/// Runs a C entry point, turning an error or a panic into a status code and
/// leaving the message where `rustacity_ext_last_error` finds it.
fn guard(body: impl FnOnce() -> Result<(), ExtError>) -> c_int {
    // AssertUnwindSafe: the closures here move raw pointers around, which is
    // not auto-UnwindSafe, but nothing they hold is left in a broken state by a
    // panic that this function itself absorbs.
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => {
            clear_last_error();
            0
        }
        Ok(Err(error)) => {
            set_last_error(&error.to_string());
            error.status()
        }
        Err(payload) => {
            let detail = panic_message(&payload);
            set_last_error(&format!("internal error: {detail}"));
            1
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic crossed the extension boundary".to_string()
    }
}

/// Checks an out-parameter and writes through it.
///
/// # Safety
///
/// `out` must be null or point to a writable, properly aligned place.
unsafe fn write_out<T>(out: *mut T, value: T) -> Result<(), ExtError> {
    if out.is_null() {
        return Err(ExtError::InvalidArgument(
            "a null pointer was passed for a result".to_string(),
        ));
    }
    // SAFETY: the caller promises out points to writable memory for a T.
    unsafe { out.write(value) };
    Ok(())
}

/// # Safety
///
/// `handle` must be null or a pointer from the matching create function that
/// has not been destroyed.
unsafe fn required<'a, T>(handle: *const T, what: &str) -> Result<&'a T, ExtError> {
    // SAFETY: the caller's promise is what makes this sound.
    unsafe { handle.as_ref() }.ok_or_else(|| {
        ExtError::InvalidArgument(format!("a null {what} handle was passed to the host"))
    })
}

/// # Safety
///
/// `text` must be null or a NUL-terminated string that stays valid for the call.
unsafe fn c_text(text: *const c_char, what: &str) -> Result<String, ExtError> {
    if text.is_null() {
        return Err(ExtError::InvalidArgument(format!("the {what} is null")));
    }
    // SAFETY: the caller promises a NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(text) };
    bytes
        .to_str()
        .map(|text| text.to_string())
        .map_err(|error| ExtError::InvalidArgument(format!("the {what} is not UTF-8: {error}")))
}

/// # Safety
///
/// `instance` must be null or a live instance from
/// [`rustacity_ext_instance_create`].
unsafe fn instance_of<'a>(
    instance: *const RustacityExtInstance,
) -> Result<&'a RustacityExtInstance, ExtError> {
    // SAFETY: the caller's promise is what makes this sound.
    unsafe { required(instance, "instance") }
}

/// Locks an instance for one call. A poisoned lock is taken back: a panic that
/// reached this far has already been turned into a status code, and the
/// instance inside is not shared with anyone.
fn locked(instance: &RustacityExtInstance) -> MutexGuard<'_, Instance> {
    instance
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Creates an engine. `config` may be null for every default.
///
/// # Safety
///
/// `out` must point to a writable engine pointer.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_engine_create(
    config: *const RustacityExtHostConfig,
    out: *mut *mut RustacityExtEngine,
) -> c_int {
    guard(|| {
        let mut host = HostConfig::default();
        if !config.is_null() {
            // SAFETY: the caller promises config points to a readable struct.
            let config = unsafe { &*config };
            if config.max_memory_bytes > 0 {
                host.limits.max_memory_bytes = config.max_memory_bytes;
            }
            if config.timeout_ms > 0 {
                host.limits.timeout = std::time::Duration::from_millis(config.timeout_ms);
            }
            if config.max_block_frames > 0 {
                host.limits.max_block_frames = config.max_block_frames as usize;
            }
            if config.log_capacity > 0 {
                host.log_capacity = config.log_capacity as usize;
            }
        }
        let engine = Engine::new(host)?;
        // SAFETY: out was checked by write_out.
        unsafe {
            write_out(
                out,
                Box::into_raw(Box::new(RustacityExtEngine { inner: engine })),
            )
        }
    })
}

/// Destroys an engine and everything loaded from it.
///
/// # Safety
///
/// `engine` must be null or a pointer from [`rustacity_ext_engine_create`], not
/// already destroyed, with no extension or instance from it still in use.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_engine_destroy(engine: *mut RustacityExtEngine) {
    guard(|| {
        if !engine.is_null() {
            // SAFETY: the caller promises a live pointer from create.
            drop(unsafe { Box::from_raw(engine) });
        }
        Ok(())
    });
}

/// Loads the extension in `directory`.
///
/// # Safety
///
/// `engine` must be alive, `directory` a NUL-terminated UTF-8 path to an
/// extension directory, and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_engine_load(
    engine: *const RustacityExtEngine,
    directory: *const c_char,
    out: *mut *mut RustacityExtExtension,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let engine = unsafe { required(engine, "engine") }?;
        let directory = unsafe { c_text(directory, "extension directory") }?;
        let extension = engine.inner.load(Path::new(&directory))?;
        let cached = Cached::new(extension)?;
        // SAFETY: out was checked by write_out.
        unsafe {
            write_out(
                out,
                Box::into_raw(Box::new(RustacityExtExtension { inner: cached })),
            )
        }
    })
}

/// Destroys a loaded extension. Instances made from it must be destroyed first.
///
/// # Safety
///
/// `extension` must be null or a pointer from [`rustacity_ext_engine_load`], not
/// already destroyed.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_destroy(extension: *mut RustacityExtExtension) {
    guard(|| {
        if !extension.is_null() {
            // SAFETY: the caller promises a live pointer from load.
            drop(unsafe { Box::from_raw(extension) });
        }
        Ok(())
    });
}

/// The manifest id.
///
/// # Safety
///
/// `extension` must be alive and `out` writable; the string stays valid until
/// the extension is destroyed.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_id(
    extension: *const RustacityExtExtension,
    out: *mut *const c_char,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        unsafe { write_out(out, extension.inner.id.as_ptr()) }
    })
}

/// The manifest name, for menus.
///
/// # Safety
///
/// See [`rustacity_ext_extension_id`].
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_name(
    extension: *const RustacityExtExtension,
    out: *mut *const c_char,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        unsafe { write_out(out, extension.inner.name.as_ptr()) }
    })
}

/// The extension's own version.
///
/// # Safety
///
/// See [`rustacity_ext_extension_id`].
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_version(
    extension: *const RustacityExtExtension,
    out: *mut *const c_char,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        unsafe { write_out(out, extension.inner.version.as_ptr()) }
    })
}

/// The description, or null if the manifest has none.
///
/// # Safety
///
/// See [`rustacity_ext_extension_id`].
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_description(
    extension: *const RustacityExtExtension,
    out: *mut *const c_char,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        unsafe { write_out(out, or_null(extension.inner.description.as_ref())) }
    })
}

/// The directory the extension was loaded from.
///
/// # Safety
///
/// See [`rustacity_ext_extension_id`].
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_path(
    extension: *const RustacityExtExtension,
    out: *mut *const c_char,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        unsafe { write_out(out, extension.inner.unit_of_path.as_ptr()) }
    })
}

/// How many parameters the extension has.
///
/// # Safety
///
/// `extension` must be alive and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_parameter_count(
    extension: *const RustacityExtExtension,
    out: *mut c_int,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        unsafe { write_out(out, extension.inner.parameters.len() as c_int) }
    })
}

/// Describes one parameter, by index.
///
/// # Safety
///
/// `extension` must be alive and `out` writable; the strings written into `out`
/// stay valid until the extension is destroyed.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_extension_parameter(
    extension: *const RustacityExtExtension,
    index: c_int,
    out: *mut RustacityExtParameter,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        if index < 0 {
            return Err(ExtError::InvalidArgument(format!(
                "parameter index {index} is negative"
            )));
        }
        let parameter = extension
            .inner
            .parameters
            .get(index as usize)
            .ok_or_else(|| {
                ExtError::InvalidArgument(format!(
                    "this extension has {} parameter(s); there is no parameter {index}",
                    extension.inner.parameters.len()
                ))
            })?;
        let described = RustacityExtParameter {
            id: parameter.id.as_ptr(),
            name: parameter.name.as_ptr(),
            unit: or_null(parameter.unit.as_ref()),
            description: or_null(parameter.description.as_ref()),
            kind: parameter.kind,
            minimum: parameter.minimum,
            maximum: parameter.maximum,
            default_value: parameter.default_value,
            choice_count: parameter.choices.len() as c_int,
            choices: if parameter.choice_pointers.is_empty() {
                std::ptr::null()
            } else {
                parameter.choice_pointers.as_ptr()
            },
        };
        unsafe { write_out(out, described) }
    })
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

/// Prepares one running copy of an extension.
///
/// # Safety
///
/// `extension` must be alive and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_create(
    extension: *const RustacityExtExtension,
    sample_rate: f32,
    max_block: c_int,
    channels: c_int,
    out: *mut *mut RustacityExtInstance,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let extension = unsafe { required(extension, "extension") }?;
        if max_block <= 0 || channels <= 0 {
            return Err(ExtError::InvalidArgument(
                "a block and a channel count are both at least 1".to_string(),
            ));
        }
        let instance = extension.inner.extension.instantiate(
            sample_rate,
            max_block as usize,
            channels as usize,
        )?;
        let handle = RustacityExtInstance {
            inner: Mutex::new(instance),
            logs: Mutex::new(Vec::new()),
        };
        unsafe { write_out(out, Box::into_raw(Box::new(handle))) }
    })
}

/// Destroys an instance.
///
/// # Safety
///
/// `instance` must be null or a pointer from [`rustacity_ext_instance_create`],
/// not already destroyed.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_destroy(instance: *mut RustacityExtInstance) {
    guard(|| {
        if !instance.is_null() {
            // SAFETY: the caller promises a live pointer from create.
            drop(unsafe { Box::from_raw(instance) });
        }
        Ok(())
    });
}

/// Sets a parameter by index, in manifest order.
///
/// # Safety
///
/// `instance` must be alive.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_set_param(
    instance: *const RustacityExtInstance,
    index: c_int,
    value: f32,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promise.
        let instance = unsafe { instance_of(instance) }?;
        if index < 0 {
            return Err(ExtError::InvalidArgument(format!(
                "parameter index {index} is negative"
            )));
        }
        locked(instance).set_param(index as usize, value)
    })
}

/// Sets a parameter by its manifest id.
///
/// # Safety
///
/// `instance` must be alive and `id` a NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_set_param_by_id(
    instance: *const RustacityExtInstance,
    id: *const c_char,
    value: f32,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let instance = unsafe { instance_of(instance) }?;
        let id = unsafe { c_text(id, "parameter id") }?;
        locked(instance).set_param_by_id(&id, value)
    })
}

/// Sets every parameter to the manifest's default.
///
/// # Safety
///
/// `instance` must be alive.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_set_default_parameters(
    instance: *const RustacityExtInstance,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promise.
        let instance = unsafe { instance_of(instance) }?;
        locked(instance).set_default_parameters()
    })
}

/// Sends one block of planar audio through the extension.
///
/// `input` is an array of `channels` pointers to `frames` samples, and so is
/// `output`; they may be the same buffers.
///
/// # Safety
///
/// Both arrays must hold `channels` valid pointers to at least `frames` floats.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_process(
    instance: *const RustacityExtInstance,
    input: *const *const f32,
    output: *const *mut f32,
    channels: c_int,
    frames: c_int,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promise.
        let instance = unsafe { instance_of(instance) }?;
        if input.is_null() || output.is_null() {
            return Err(ExtError::InvalidArgument(
                "the audio pointers are null".to_string(),
            ));
        }
        if channels <= 0 || frames <= 0 {
            return Err(ExtError::InvalidArgument(
                "a block has at least one channel and one frame".to_string(),
            ));
        }
        let channels = channels as usize;
        let frames = frames as usize;

        let mut input_slices: Vec<&[f32]> = Vec::with_capacity(channels);
        let mut output_slices: Vec<&mut [f32]> = Vec::with_capacity(channels);
        for channel in 0..channels {
            // SAFETY: the caller promises channels valid pointers.
            let (in_channel, out_channel) = unsafe { (*input.add(channel), *output.add(channel)) };
            if in_channel.is_null() || out_channel.is_null() {
                return Err(ExtError::InvalidArgument(format!(
                    "channel {channel} has a null buffer"
                )));
            }
            // SAFETY: the caller promises each buffer holds frames samples.
            input_slices.push(unsafe { std::slice::from_raw_parts(in_channel, frames) });
            // SAFETY: as above, and the slices are disjoint from the input's by
            // the caller's contract.
            output_slices.push(unsafe { std::slice::from_raw_parts_mut(out_channel, frames) });
        }

        locked(instance).process(&input_slices, &mut output_slices)
    })
}

/// Returns the extension to the state it was instantiated in.
///
/// # Safety
///
/// `instance` must be alive.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_reset(
    instance: *const RustacityExtInstance,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promise.
        let instance = unsafe { instance_of(instance) }?;
        locked(instance).reset()
    })
}

/// Whether a failure has spent this instance.
///
/// # Safety
///
/// `instance` must be alive and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_is_poisoned(
    instance: *const RustacityExtInstance,
    out: *mut c_int,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let instance = unsafe { instance_of(instance) }?;
        unsafe { write_out(out, c_int::from(locked(instance).is_poisoned())) }
    })
}

/// Collects what the extension has logged since the last collection, so that the
/// accessors below can hand it out. Called by them; call it directly only if you
/// want the records before then.
///
/// # Safety
///
/// `instance` must be alive and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_collect_logs(
    instance: *const RustacityExtInstance,
    out: *mut c_int,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let instance = unsafe { instance_of(instance) }?;
        let collected = {
            let mut instance = locked(instance);
            instance.take_logs()
        };
        let count = {
            let mut logs = instance
                .logs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for record in collected {
                logs.push(CachedLog {
                    level: record.level,
                    text: CString::new(record.message.replace('\0', " ")).unwrap_or_default(),
                });
            }
            logs.len() as c_int
        };
        // SAFETY: out was checked by write_out.
        unsafe { write_out(out, count) }
    })
}

/// How many collected log records there are.
///
/// # Safety
///
/// `instance` must be alive and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_log_count(
    instance: *const RustacityExtInstance,
    out: *mut c_int,
) -> c_int {
    // SAFETY: the caller's promises.
    unsafe { rustacity_ext_instance_collect_logs(instance, out) }
}

/// One collected log record: its level and its text, which stays valid until
/// the next collection.
///
/// # Safety
///
/// `instance` must be alive, `level` and `text` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_log(
    instance: *const RustacityExtInstance,
    index: c_int,
    level: *mut c_int,
    text: *mut *const c_char,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promise.
        let instance = unsafe { instance_of(instance) }?;
        if index < 0 {
            return Err(ExtError::InvalidArgument(format!(
                "log index {index} is negative"
            )));
        }
        let logs = instance
            .logs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = logs.get(index as usize).ok_or_else(|| {
            ExtError::InvalidArgument(format!(
                "there are {} collected log record(s); there is no record {index}",
                logs.len()
            ))
        })?;
        unsafe { write_out(level, record.level)? };
        unsafe { write_out(text, record.text.as_ptr()) }
    })
}

/// How many log records were dropped for want of budget.
///
/// # Safety
///
/// `instance` must be alive and `out` writable.
#[no_mangle]
pub unsafe extern "C" fn rustacity_ext_instance_dropped_logs(
    instance: *const RustacityExtInstance,
    out: *mut u64,
) -> c_int {
    guard(|| {
        // SAFETY: the caller's promises.
        let instance = unsafe { instance_of(instance) }?;
        unsafe { write_out(out, locked(instance).dropped_log_records()) }
    })
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// The message from the last failing call on this thread.
///
/// The pointer belongs to the library and stays valid until the next call that
/// sets or clears it: do not free it, and copy it if you need to keep it.
#[no_mangle]
pub extern "C" fn rustacity_ext_last_error() -> *const c_char {
    LAST_ERROR.with(|cell| cell.borrow().as_ptr())
}

/// The API version this host implements.
#[no_mangle]
pub extern "C" fn rustacity_ext_api_version() -> c_int {
    rustacity_ext_api::abi::API_VERSION
}

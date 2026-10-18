// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! The engine: loading extensions, running them, and keeping them inside the
//! limits the API promises.
//!
//! The shape is [`Engine`] -> [`Extension`] -> [`Instance`]. Loading is where
//! everything is *checked*: the manifest is validated, the module is compiled,
//! its imports are examined, and a probe instance is built so that a missing
//! export, a mistyped one, or an extension claiming the wrong API version is
//! refused before a user ever picks it. Instantiating is where audio starts
//! flowing, and every call into the module goes through the machinery here:
//! a budgeted log, a wall-clock deadline, a memory ceiling, and buffer copies
//! that are bounds-checked on both sides.
//!
//! A trap or a timeout poisons that one instance and nothing else: the host,
//! the loaded extension and every other instance carry on.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wasmtime::{
    Caller, Engine as WasmEngine, ExternType, Instance as WasmInstance, Linker, Memory, Module,
    Store, TypedFunc, UpdateDeadline, ValType,
};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

use rustacity_ext_api::abi;
use rustacity_ext_api::{ExtError, Manifest, Parameter, ParameterKind};

use crate::limits::{HostConfig, ProcessLimits, StoreData};
use crate::log::LogRecord;

/// The name Rust compiles a module's initializer to, when it has one.
const INITIALIZER_EXPORT: &str = "_initialize";

/// WASI's "bad address".
const ERRNO_FAULT: i32 = 21;

/// Where the deterministic generator starts. Fixed, so that two runs of the
/// same extension on the same audio are the same run.
pub(crate) const DETERMINISTIC_RANDOM_START: u64 = 0x5255_5354_4143_4954;

/// Advances the epoch so that running calls meet their deadline.
///
/// Wasmtime checks the epoch at loop back edges and at function entries; this
/// ticker is what makes those checks happen while a module is stuck. One thread
/// per engine, waking every [`abi::EPOCH_TICK_MS`], stopped when the engine is
/// dropped.
struct EpochTicker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EpochTicker {
    fn start(engine: WasmEngine, tick: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("rustacity-ext-epoch".to_string())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        thread::sleep(tick);
                        engine.increment_epoch();
                    }
                })
                .ok()
        };
        Self { stop, thread }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One WebAssembly engine, with the ticker that enforces call deadlines.
///
/// An application wants one of these for as long as it runs; loading an
/// extension from it is cheap after the first time.
pub struct Engine {
    inner: WasmEngine,
    limits: ProcessLimits,
    log_capacity: usize,
    /// Dropped last, and only then does the ticker stop.
    _ticker: EpochTicker,
}

impl fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The engine itself has no printable state; what matters when
        // something goes wrong is the policy it applies.
        formatter
            .debug_struct("Engine")
            .field("limits", &self.limits)
            .field("log_capacity", &self.log_capacity)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Builds an engine with the given policy.
    ///
    /// The WebAssembly feature set is chosen to be as small as extensions need:
    /// no threads and no shared memory, one memory, 32-bit addressing. A module
    /// that requires more than that is refused when it is loaded, which is the
    /// point of turning the features off.
    pub fn new(config: HostConfig) -> Result<Self, ExtError> {
        let mut wasmtime_config = wasmtime::Config::new();
        // An epoch check is the only thing that can interrupt a module that has
        // decided not to return; fuel would tax every call for the same safety.
        wasmtime_config.epoch_interruption(true);
        // Threads are not merely switched off here: the engine is compiled
        // without Wasmtime's `threads` feature (see the crate's manifest), so a
        // module that wants shared memory cannot even be compiled. These two
        // stay explicit because the features behind them *are* available.
        wasmtime_config.wasm_multi_memory(false);
        wasmtime_config.wasm_memory64(false);

        let inner = WasmEngine::new(&wasmtime_config).map_err(|error| {
            ExtError::Internal(format!("cannot start the WebAssembly engine: {error}"))
        })?;
        let ticker = EpochTicker::start(
            inner.clone(),
            Duration::from_millis(abi::EPOCH_TICK_MS.max(1)),
        );

        Ok(Self {
            inner,
            limits: config.limits,
            log_capacity: config.log_capacity,
            _ticker: ticker,
        })
    }

    /// The limits this engine hands to the extensions it loads.
    pub fn limits(&self) -> &ProcessLimits {
        &self.limits
    }

    /// Loads the extension in `directory`: its manifest, its module, and enough
    /// of a probe run to know the module can be used at all.
    pub fn load(&self, directory: &Path) -> Result<Extension, ExtError> {
        let manifest = Manifest::load(directory).map_err(|error| match error {
            ExtError::Io(message) => ExtError::Io(format!(
                "{message} (an extension is a directory holding {} and {})",
                rustacity_ext_api::manifest::MANIFEST_FILE,
                rustacity_ext_api::manifest::MODULE_FILE
            )),
            other => other,
        })?;

        let module_path = Manifest::module_path_in(directory);
        let bytes = std::fs::read(&module_path).map_err(|error| {
            ExtError::Io(format!("cannot read {}: {error}", module_path.display()))
        })?;
        let module = Module::new(&self.inner, &bytes).map_err(|error| {
            ExtError::ModuleInvalid(format!("{}: {error}", module_path.display()))
        })?;
        check_imports(&module, &module_path)?;

        let extension = Extension {
            engine: self.inner.clone(),
            module,
            manifest,
            path: directory.to_path_buf(),
            limits: self.limits,
            log_capacity: self.log_capacity,
        };

        // A probe instance: the same checks the real one will make, with one
        // channel and one frame, thrown away again. It is what turns "the
        // module does not export rx_process" into a load error.
        drop(extension.build(44_100.0, 1, 1)?);

        Ok(extension)
    }
}

/// A loaded extension: its manifest, its compiled module, and the limits it
/// runs under.
pub struct Extension {
    engine: WasmEngine,
    module: Module,
    manifest: Manifest,
    path: PathBuf,
    limits: ProcessLimits,
    log_capacity: usize,
}

impl fmt::Debug for Extension {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Extension")
            .field("id", &self.manifest.id)
            .field("version", &self.manifest.version)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Extension {
    /// What the extension says about itself.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The directory it was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its manifest id.
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Its manifest name, for menus.
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Its own version.
    pub fn version(&self) -> &str {
        &self.manifest.version
    }

    /// Its parameters, in manifest order.
    pub fn parameters(&self) -> &[Parameter] {
        &self.manifest.parameters
    }

    /// Prepares one running copy: the buffers are allocated inside the module,
    /// and `rx_init` is told the sample rate, the largest block that will ever
    /// arrive, and how many channels.
    pub fn instantiate(
        &self,
        sample_rate: f32,
        max_block: usize,
        channels: usize,
    ) -> Result<Instance, ExtError> {
        if !self.manifest.can_process_audio() {
            return Err(ExtError::CapabilityDenied(format!(
                "'{}' does not ask for both {} and {}, so there is no audio to give it",
                self.manifest.id,
                abi::CAP_AUDIO_READ,
                abi::CAP_AUDIO_WRITE
            )));
        }
        self.build(sample_rate, max_block, channels)
    }

    fn build(
        &self,
        sample_rate: f32,
        max_block: usize,
        channels: usize,
    ) -> Result<Instance, ExtError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(ExtError::InvalidArgument(
                "the sample rate must be a positive number".to_string(),
            ));
        }
        if channels == 0 {
            return Err(ExtError::InvalidArgument(
                "audio has at least one channel".to_string(),
            ));
        }
        if max_block == 0 || max_block > self.limits.max_block_frames {
            return Err(ExtError::InvalidArgument(format!(
                "a block of {max_block} frames is outside 1..{}",
                self.limits.max_block_frames
            )));
        }
        let buffer_bytes = match channels
            .checked_mul(max_block)
            .and_then(|frames| frames.checked_mul(std::mem::size_of::<f32>()))
        {
            Some(bytes) => bytes,
            None => {
                return Err(ExtError::InvalidArgument(
                    "that many channels and frames overflow this host".to_string(),
                ))
            }
        };
        if buffer_bytes.saturating_mul(2) > self.limits.max_memory_bytes {
            return Err(ExtError::InvalidArgument(format!(
                "two {channels}-channel blocks of {max_block} frames would not fit in the \
                 extension's {} MiB of memory",
                self.limits.max_memory_bytes / (1024 * 1024)
            )));
        }

        // WASI is linked and knows nothing: no preopened directories, no
        // environment, no arguments, and closed standard streams. What is left
        // of it - the clock, the random source - is replaced below.
        let wasi: WasiP1Ctx = WasiCtxBuilder::new().build_p1();
        let mut store = Store::new(
            &self.engine,
            StoreData::new(&self.limits, self.log_capacity, wasi),
        );
        store.limiter(|data| &mut data.limits);
        store.epoch_deadline_callback(|mut context| {
            context.data_mut().timed_out = true;
            Ok(UpdateDeadline::Interrupt)
        });
        let limit_flag = store.data().limits.flag();

        let mut linker: Linker<StoreData> = Linker::new(&self.engine);
        // The deterministic clock and randomness below are defined *after* WASI
        // and take its place; without this, Wasmtime refuses the second
        // definition of an import.
        linker.allow_shadowing(true);
        p1::add_to_linker_sync(&mut linker, |data| &mut data.wasi).map_err(|error| {
            ExtError::Internal(format!("cannot link the WASI runtime: {error}"))
        })?;
        linker
            .func_wrap(abi::IMPORT_MODULE, abi::IMPORT_LOG, host_log)
            .map_err(|error| ExtError::Internal(format!("cannot link rx_log: {error}")))?;
        linker
            .func_wrap(abi::WASI_MODULE, "random_get", random_get)
            .map_err(|error| ExtError::Internal(format!("cannot link random_get: {error}")))?;
        linker
            .func_wrap(abi::WASI_MODULE, "clock_time_get", clock_time_get)
            .map_err(|error| ExtError::Internal(format!("cannot link clock_time_get: {error}")))?;
        linker
            .func_wrap(abi::WASI_MODULE, "clock_res_get", clock_res_get)
            .map_err(|error| ExtError::Internal(format!("cannot link clock_res_get: {error}")))?;

        store.set_epoch_deadline(self.limits.deadline_ticks());
        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|error| {
                ExtError::ModuleInvalid(format!(
                    "cannot instantiate {}: {error}",
                    self.path.display()
                ))
            })?;

        // Exports, checked by type rather than by name alone.
        let memory = instance
            .get_memory(&mut store, abi::EXPORT_MEMORY)
            .ok_or_else(|| {
                ExtError::AbiMismatch(format!(
                    "'{}' does not export a linear memory named '{}'",
                    self.manifest.id,
                    abi::EXPORT_MEMORY
                ))
            })?;
        let api_version = typed::<(), i32>(&mut store, &instance, abi::EXPORT_API_VERSION)?;
        let alloc = typed::<i32, i32>(&mut store, &instance, abi::EXPORT_ALLOC)?;
        let free = typed::<(i32, i32), ()>(&mut store, &instance, abi::EXPORT_FREE)?;
        let init = typed::<(f32, i32, i32), i32>(&mut store, &instance, abi::EXPORT_INIT)?;
        let set_param = typed::<(i32, f32), ()>(&mut store, &instance, abi::EXPORT_SET_PARAM)?;
        let process = typed::<(i32, i32, i32), i32>(&mut store, &instance, abi::EXPORT_PROCESS)?;
        let reset = instance
            .get_typed_func::<(), ()>(&mut store, abi::EXPORT_RESET)
            .ok();

        let mut running = Instance {
            store,
            instance,
            memory,
            api_version,
            alloc,
            free,
            init,
            set_param,
            process,
            reset,
            parameters: self.manifest.parameters.clone(),
            limits: self.limits,
            channels,
            max_block,
            sample_rate,
            in_ptr: 0,
            out_ptr: 0,
            buffer_bytes,
            scratch: Vec::new(),
            limit_flag,
            poisoned: None,
        };

        // The module's own initializer, if it has one: a Rust `cdylib` that
        // needs its constructors run exports `_initialize`.
        running.call_initializer()?;

        running.arm();
        let version = running
            .api_version
            .call(&mut running.store, ())
            .map_err(|error| running.trap(error))?;
        if version != abi::API_VERSION {
            return Err(ExtError::AbiMismatch(format!(
                "'{}' says it speaks API version {version}; this host speaks version {}",
                self.manifest.id,
                abi::API_VERSION
            )));
        }

        running.allocate_buffers()?;

        running.arm();
        let code = running
            .init
            .call(
                &mut running.store,
                (sample_rate, max_block as i32, channels as i32),
            )
            .map_err(|error| running.trap(error))?;
        if code != 0 {
            return Err(ExtError::ModuleReportedError(code));
        }
        Ok(running)
    }
}

/// Looks up an export and insists on its exact signature.
fn typed<Params, Results>(
    store: &mut Store<StoreData>,
    instance: &WasmInstance,
    export: &str,
) -> Result<TypedFunc<Params, Results>, ExtError>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    instance
        .get_typed_func::<Params, Results>(store, export)
        .map_err(|error| {
            ExtError::AbiMismatch(format!(
                "the extension's '{export}' export is missing or has the wrong signature: {error}"
            ))
        })
}

/// Refuses a module that imports anything the host does not provide.
///
/// Only two namespaces are open: WASI preview 1, which is linked empty (see the
/// API document), and `rustacity.rx_log`.
fn check_imports(module: &Module, path: &Path) -> Result<(), ExtError> {
    for import in module.imports() {
        let module_name = import.module();
        let field = import.name();
        if module_name == abi::WASI_MODULE {
            continue;
        }
        if module_name == abi::IMPORT_MODULE {
            if field != abi::IMPORT_LOG {
                return Err(ExtError::ModuleInvalid(format!(
                    "{} imports '{module_name}.{field}'; API version {} provides only '{}.{}'",
                    path.display(),
                    abi::API_VERSION,
                    abi::IMPORT_MODULE,
                    abi::IMPORT_LOG
                )));
            }
            let ExternType::Func(function) = import.ty() else {
                return Err(ExtError::ModuleInvalid(format!(
                    "{} imports '{module_name}.{field}', which is not a function",
                    path.display()
                )));
            };
            let params: Vec<ValType> = function.params().collect();
            let results: Vec<ValType> = function.results().collect();
            let three_integers =
                params.len() == 3 && params.iter().all(|value| matches!(value, ValType::I32));
            if !three_integers || !results.is_empty() {
                return Err(ExtError::ModuleInvalid(format!(
                    "{} imports '{module_name}.{field}' with the wrong signature: \
                     API version {} defines it as (i32, i32, i32) -> ()",
                    path.display(),
                    abi::API_VERSION
                )));
            }
            continue;
        }
        return Err(ExtError::ModuleInvalid(format!(
            "{} imports '{module_name}.{field}'; this host provides WASI preview 1 and \
             '{}.{}', and nothing else",
            path.display(),
            abi::IMPORT_MODULE,
            abi::IMPORT_LOG
        )));
    }
    Ok(())
}

/// One running copy of an extension.
///
/// Instances are cheap but not free: each has its own linear memory and its own
/// store. One effect invocation gets one instance, which is what the API
/// document says.
pub struct Instance {
    store: Store<StoreData>,
    instance: WasmInstance,
    memory: Memory,
    api_version: TypedFunc<(), i32>,
    alloc: TypedFunc<i32, i32>,
    free: TypedFunc<(i32, i32), ()>,
    init: TypedFunc<(f32, i32, i32), i32>,
    set_param: TypedFunc<(i32, f32), ()>,
    process: TypedFunc<(i32, i32, i32), i32>,
    reset: Option<TypedFunc<(), ()>>,
    parameters: Vec<Parameter>,
    limits: ProcessLimits,
    channels: usize,
    max_block: usize,
    sample_rate: f32,
    /// Where the module keeps this instance's buffers, in its own memory.
    in_ptr: u32,
    out_ptr: u32,
    buffer_bytes: usize,
    /// A byte buffer for the copies in and out, reused for every channel.
    scratch: Vec<u8>,
    limit_flag: Arc<AtomicBool>,
    /// Set once something has gone wrong badly enough that this instance must
    /// not be used again.
    poisoned: Option<String>,
}

impl fmt::Debug for Instance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Instance")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("max_block", &self.max_block)
            .field("parameters", &self.parameters.len())
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

impl Instance {
    /// The sample rate this instance was prepared for.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// How many channels it was prepared for.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The largest block it accepts, in frames.
    pub fn max_block(&self) -> usize {
        self.max_block
    }

    /// The extension's parameters, in the order `set_param` indexes them.
    pub fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }

    /// The limits it runs under.
    pub fn limits(&self) -> &ProcessLimits {
        &self.limits
    }

    /// Whether a failure has made this instance unusable.
    pub fn is_poisoned(&self) -> bool {
        self.poisoned.is_some()
    }

    /// Why, if it is.
    pub fn poison_reason(&self) -> Option<&str> {
        self.poisoned.as_deref()
    }

    /// Sets a parameter by its index in the manifest.
    ///
    /// The value is checked against the manifest and *refused* rather than
    /// clamped, so that a host which miscalculated cannot quietly turn into a
    /// different request.
    pub fn set_param(&mut self, index: usize, value: f32) -> Result<(), ExtError> {
        self.check_usable()?;
        let parameter = self.parameters.get(index).cloned().ok_or_else(|| {
            ExtError::InvalidArgument(format!(
                "this extension has {} parameter(s); there is no parameter {index}",
                self.parameters.len()
            ))
        })?;
        validate_value(&parameter, value)?;
        let value = normalise(&parameter, value);
        self.arm();
        self.set_param
            .call(&mut self.store, (index as i32, value))
            .map_err(|error| self.trap(error))
    }

    /// Sets a parameter by its id, which is what a manifest and a settings file
    /// would name.
    pub fn set_param_by_id(&mut self, id: &str, value: f32) -> Result<(), ExtError> {
        let index = self
            .parameters
            .iter()
            .position(|parameter| parameter.id == id)
            .ok_or_else(|| {
                ExtError::InvalidArgument(format!("this extension has no parameter '{id}'"))
            })?;
        self.set_param(index, value)
    }

    /// Sets every parameter to the default the manifest declares.
    pub fn set_default_parameters(&mut self) -> Result<(), ExtError> {
        for index in 0..self.parameters.len() {
            let default = self.parameters[index].default;
            self.set_param(index, default)?;
        }
        Ok(())
    }

    /// Sends one block of audio through the extension.
    ///
    /// `input` and `output` are planar: one slice per channel, all the same
    /// length, and no longer than the block the instance was prepared for.
    pub fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) -> Result<(), ExtError> {
        self.check_usable()?;
        if input.len() != self.channels || output.len() != self.channels {
            return Err(ExtError::InvalidArgument(format!(
                "this instance was prepared for {} channel(s) but was handed {} in and {} out",
                self.channels,
                input.len(),
                output.len()
            )));
        }
        let frames = input.first().map_or(0, |channel| channel.len());
        if frames == 0 || frames > self.max_block {
            return Err(ExtError::InvalidArgument(format!(
                "a block of {frames} frames is outside 1..{}",
                self.max_block
            )));
        }
        for (index, channel) in input.iter().enumerate() {
            if channel.len() != frames {
                return Err(ExtError::InvalidArgument(format!(
                    "input channel {index} has {} frames, not {frames}",
                    channel.len()
                )));
            }
        }
        for (index, channel) in output.iter().enumerate() {
            if channel.len() != frames {
                return Err(ExtError::InvalidArgument(format!(
                    "output channel {index} has {} frames, not {frames}",
                    channel.len()
                )));
            }
        }

        let sample_bytes = std::mem::size_of::<f32>();
        for (channel, samples) in input.iter().enumerate() {
            let offset = self.in_ptr as usize + channel * frames * sample_bytes;
            pack_samples(samples, &mut self.scratch);
            self.memory
                .write(&mut self.store, offset, &self.scratch)
                .map_err(|error| {
                    ExtError::Internal(format!(
                        "cannot copy audio into the extension's memory: {error}"
                    ))
                })?;
        }

        self.arm();
        let result = self.process.call(
            &mut self.store,
            (self.in_ptr as i32, self.out_ptr as i32, frames as i32),
        );
        match result {
            Ok(0) => {}
            Ok(code) => return Err(ExtError::ModuleReportedError(code)),
            Err(error) => return Err(self.trap(error)),
        }

        for (channel, samples) in output.iter_mut().enumerate() {
            let offset = self.out_ptr as usize + channel * frames * sample_bytes;
            unpack_samples(
                &self.memory,
                &self.store,
                offset,
                samples,
                &mut self.scratch,
            )?;
        }
        Ok(())
    }

    /// Returns the extension to the state it was instantiated in. An extension
    /// that does not export `rx_reset` has nothing to reset.
    pub fn reset(&mut self) -> Result<(), ExtError> {
        self.check_usable()?;
        let Some(reset) = self.reset.clone() else {
            return Ok(());
        };
        self.arm();
        reset
            .call(&mut self.store, ())
            .map_err(|error| self.trap(error))
    }

    /// Collects the messages the extension has written through `rx_log`.
    pub fn take_logs(&mut self) -> Vec<LogRecord> {
        self.store.data_mut().log.drain()
    }

    /// How many log records were dropped for want of budget or room.
    pub fn dropped_log_records(&self) -> u64 {
        self.store.data().log.dropped()
    }

    /// How many were cut short at [`abi::MAX_LOG_MESSAGE_BYTES`].
    pub fn truncated_log_records(&self) -> u64 {
        self.store.data().log.truncated()
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    /// Gives the next call its own time budget and clears the flags that
    /// explain a failure.
    fn arm(&mut self) {
        self.store.data_mut().timed_out = false;
        self.limit_flag.store(false, Ordering::SeqCst);
        self.store.data_mut().log.begin_call();
        self.store.set_epoch_deadline(self.limits.deadline_ticks());
    }

    fn check_usable(&self) -> Result<(), ExtError> {
        match &self.poisoned {
            Some(reason) => Err(ExtError::Poisoned(reason.clone())),
            None => Ok(()),
        }
    }

    /// Turns a Wasmtime error into the API's vocabulary, and, when the error
    /// means the module's state cannot be trusted any more, spends the
    /// instance.
    fn trap(&mut self, error: wasmtime::Error) -> ExtError {
        let mapped = self.explain(&error);
        let fatal = matches!(
            mapped,
            ExtError::Trap(_)
                | ExtError::Timeout(_)
                | ExtError::OutOfMemory(_)
                | ExtError::Internal(_)
        );
        if fatal {
            self.poisoned = Some(mapped.to_string());
        }
        mapped
    }

    fn explain(&self, error: &wasmtime::Error) -> ExtError {
        if self.store.data().timed_out {
            return ExtError::Timeout(format!(
                "the extension was still working after {} ms",
                self.limits.timeout.as_millis()
            ));
        }
        if self.limit_flag.load(Ordering::SeqCst) {
            return ExtError::OutOfMemory(format!(
                "the extension asked for more memory than the {} MiB it is allowed",
                self.limits.max_memory_bytes / (1024 * 1024)
            ));
        }
        if let Some(exit) = error.downcast_ref::<wasmtime_wasi::I32Exit>() {
            return ExtError::Trap(format!(
                "the extension called proc_exit({}); it stopped itself",
                exit.0
            ));
        }
        ExtError::Trap(format!("{error}"))
    }

    /// Runs the module's own initializer, when it has one.
    fn call_initializer(&mut self) -> Result<(), ExtError> {
        let Ok(initializer) = self
            .instance
            .get_typed_func::<(), ()>(&mut self.store, INITIALIZER_EXPORT)
        else {
            // No initializer, or one that is not `() -> ()`: not ours to run.
            return Ok(());
        };
        self.arm();
        initializer
            .call(&mut self.store, ())
            .map_err(|error| self.trap(error))
    }

    fn allocate_buffers(&mut self) -> Result<(), ExtError> {
        let bytes = self.buffer_bytes as i32;
        self.arm();
        let input = self
            .alloc
            .call(&mut self.store, bytes)
            .map_err(|error| self.trap(error))?;
        if input == 0 {
            return Err(ExtError::OutOfMemory(
                "the extension refused to allocate its input buffer".to_string(),
            ));
        }
        let input = self.check_buffer(input, "the input buffer")?;

        let output = self
            .alloc
            .call(&mut self.store, bytes)
            .map_err(|error| self.trap(error))?;
        if output == 0 {
            return Err(ExtError::OutOfMemory(
                "the extension refused to allocate its output buffer".to_string(),
            ));
        }
        let output = self.check_buffer(output, "the output buffer")?;

        self.in_ptr = input;
        self.out_ptr = output;
        // Room for one channel's worth of bytes at a time, which is what the
        // copies below move.
        self.scratch = Vec::with_capacity(self.buffer_bytes.min(1 << 20));
        Ok(())
    }

    /// Refuses a pointer that is not wholly inside the module's memory.
    fn check_buffer(&self, pointer: i32, what: &str) -> Result<u32, ExtError> {
        if pointer < 0 {
            return Err(ExtError::AbiMismatch(format!(
                "rx_alloc returned {pointer} for {what}, which is not a pointer"
            )));
        }
        let pointer = pointer as u32 as usize;
        let size = self.memory.data_size(&self.store);
        match pointer.checked_add(self.buffer_bytes) {
            Some(end) if end <= size => Ok(pointer as u32),
            _ => Err(ExtError::AbiMismatch(format!(
                "rx_alloc returned {what} at {pointer:#x}, which runs past the end of the \
                 module's {size} bytes of memory"
            ))),
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // Give the module its buffers back, so that an extension which tracks
        // them sees a tidy end. A spent instance is left alone.
        if self.poisoned.is_some() || self.in_ptr == 0 {
            return;
        }
        let _ = self.free.call(
            &mut self.store,
            (self.in_ptr as i32, self.buffer_bytes as i32),
        );
        let _ = self.free.call(
            &mut self.store,
            (self.out_ptr as i32, self.buffer_bytes as i32),
        );
    }
}

// ---------------------------------------------------------------------------
// Host imports: the log, and a clock and randomness that belong to the sandbox
// ---------------------------------------------------------------------------

/// `rustacity.rx_log(level, ptr, len)`: everything within the budget is kept,
/// everything else is dropped. A message that cannot be read never fails the
/// call.
fn host_log(mut caller: Caller<'_, StoreData>, level: i32, pointer: i32, length: i32) {
    if pointer < 0 || length <= 0 {
        return;
    }
    let Some(memory) = caller
        .get_export(abi::EXPORT_MEMORY)
        .and_then(|export| export.into_memory())
    else {
        return;
    };
    let keep = (length as usize).min(abi::MAX_LOG_MESSAGE_BYTES);
    let mut bytes = vec![0u8; keep];
    if memory.read(&caller, pointer as usize, &mut bytes).is_err() {
        return;
    }
    caller.data_mut().log.push(level, &bytes);
}

/// `wasi_snapshot_preview1.random_get(buf, len)`: a fixed generator, so that an
/// extension cannot use the host's entropy and a run is reproducible.
fn random_get(mut caller: Caller<'_, StoreData>, buffer: i32, length: i32) -> i32 {
    if buffer < 0 || length < 0 {
        return ERRNO_FAULT;
    }
    let Some(memory) = caller
        .get_export(abi::EXPORT_MEMORY)
        .and_then(|export| export.into_memory())
    else {
        return ERRNO_FAULT;
    };
    let total = length as usize;
    let mut written = 0usize;
    // In chunks, so that a module asking for a gigabyte only ever makes the
    // host allocate a page of it.
    while written < total {
        let chunk = (total - written).min(4096);
        let mut bytes = Vec::with_capacity(chunk);
        for _ in 0..chunk {
            let state = &mut caller.data_mut().random_state;
            *state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            bytes.push((*state >> 33) as u8);
        }
        if memory
            .write(&mut caller, buffer as usize + written, &bytes)
            .is_err()
        {
            return ERRNO_FAULT;
        }
        written += chunk;
    }
    0
}

/// `wasi_snapshot_preview1.clock_time_get(id, precision, out)`: a counter that
/// starts at zero when the instance is created, never the host's clock.
fn clock_time_get(mut caller: Caller<'_, StoreData>, id: i32, _precision: i64, out: i32) -> i32 {
    let ticks = match id {
        // CLOCK_REALTIME: frozen, so nothing leaks the date.
        0 => 0,
        // CLOCK_MONOTONIC and everything else: one millisecond per reading.
        _ => {
            let data = caller.data_mut();
            data.clock_ticks = data.clock_ticks.saturating_add(1);
            data.clock_ticks
        }
    };
    write_u64(&mut caller, out, ticks.saturating_mul(1_000_000))
}

/// `wasi_snapshot_preview1.clock_res_get(id, out)`: the resolution of the
/// counter above.
fn clock_res_get(mut caller: Caller<'_, StoreData>, _id: i32, out: i32) -> i32 {
    write_u64(&mut caller, out, 1_000_000)
}

fn write_u64(caller: &mut Caller<'_, StoreData>, out: i32, value: u64) -> i32 {
    if out < 0 {
        return ERRNO_FAULT;
    }
    let Some(memory) = caller
        .get_export(abi::EXPORT_MEMORY)
        .and_then(|export| export.into_memory())
    else {
        return ERRNO_FAULT;
    };
    if memory
        .write(caller, out as usize, &value.to_le_bytes())
        .is_err()
    {
        return ERRNO_FAULT;
    }
    0
}

// ---------------------------------------------------------------------------
// Buffer plumbing
// ---------------------------------------------------------------------------

/// Copies host samples into the scratch buffer, little-endian, as WebAssembly
/// keeps them.
fn pack_samples(samples: &[f32], scratch: &mut Vec<u8>) {
    scratch.clear();
    scratch.reserve(samples.len() * 4);
    for sample in samples {
        scratch.extend_from_slice(&sample.to_le_bytes());
    }
}

/// Copies samples out of the module's memory.
fn unpack_samples(
    memory: &Memory,
    store: &Store<StoreData>,
    offset: usize,
    samples: &mut [f32],
    scratch: &mut Vec<u8>,
) -> Result<(), ExtError> {
    scratch.clear();
    scratch.resize(samples.len() * 4, 0);
    memory.read(store, offset, scratch).map_err(|error| {
        ExtError::Internal(format!(
            "cannot copy audio out of the extension's memory: {error}"
        ))
    })?;
    for (index, sample) in samples.iter_mut().enumerate() {
        let start = index * 4;
        *sample = f32::from_le_bytes([
            scratch[start],
            scratch[start + 1],
            scratch[start + 2],
            scratch[start + 3],
        ]);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Parameter values
// ---------------------------------------------------------------------------

/// Checks a value against its parameter, refusing what the manifest does not
/// allow.
fn validate_value(parameter: &Parameter, value: f32) -> Result<(), ExtError> {
    if !value.is_finite() {
        return Err(ExtError::InvalidArgument(format!(
            "'{}' is not a number",
            parameter.id
        )));
    }
    match &parameter.kind {
        ParameterKind::Float { min, max } => {
            let value = value as f64;
            if value < *min || value > *max {
                return Err(ExtError::InvalidArgument(format!(
                    "{value} is outside the range of '{}' ({min}..{max})",
                    parameter.id
                )));
            }
            Ok(())
        }
        ParameterKind::Bool => {
            if value == 0.0 || value == 1.0 {
                Ok(())
            } else {
                Err(ExtError::InvalidArgument(format!(
                    "'{}' is a switch: it takes 0.0 or 1.0, not {value}",
                    parameter.id
                )))
            }
        }
        ParameterKind::Choice { choices } => {
            if value.fract() == 0.0 && value >= 0.0 && (value as usize) < choices.len() {
                Ok(())
            } else {
                Err(ExtError::InvalidArgument(format!(
                    "'{}' has {} choice(s); {value} is not one of them",
                    parameter.id,
                    choices.len()
                )))
            }
        }
    }
}

/// The value as the ABI carries it: a switch becomes exactly 0.0 or 1.0, a
/// choice its index.
fn normalise(parameter: &Parameter, value: f32) -> f32 {
    match &parameter.kind {
        ParameterKind::Float { .. } => value,
        ParameterKind::Bool => {
            if value == 0.0 {
                0.0
            } else {
                1.0
            }
        }
        ParameterKind::Choice { .. } => value.round(),
    }
}

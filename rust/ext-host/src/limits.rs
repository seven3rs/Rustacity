// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! What the sandbox allows an extension to use, and the machinery that enforces
//! it.
//!
//! Two sets of numbers live here. [`ProcessLimits`] is the policy - the
//! defaults come from [`rustacity_ext_api::abi`], where the documentation quotes
//! them. [`StoreData`] is the per-instance state that carries the policy into
//! Wasmtime, including the [`DataLimits`] wrapper that notices *why* a memory
//! growth failed so the host can say `OutOfMemory` rather than `Trap`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use wasmtime::{ResourceLimiter, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::p1::WasiP1Ctx;

use rustacity_ext_api::abi;

use crate::log::LogCollector;

/// The limits one instance runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessLimits {
    /// Wall-clock budget for a single call into the module.
    pub timeout: Duration,
    /// Ceiling on the module's linear memory, in bytes.
    pub max_memory_bytes: usize,
    /// Ceiling on one table, in elements.
    pub max_table_elements: usize,
    /// How many tables the module may have.
    pub max_tables: usize,
    /// How many instances it may create.
    pub max_instances: usize,
    /// How many memories it may have.
    pub max_memories: usize,
    /// Largest block the host will hand to `rx_process`.
    pub max_block_frames: usize,
    /// Log budget for one call, as in `docs/extensions/API.md` section 4.
    pub max_log_messages_per_call: usize,
    pub max_log_bytes_per_call: usize,
    pub max_log_message_bytes: usize,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(abi::DEFAULT_TIMEOUT_MS),
            max_memory_bytes: abi::DEFAULT_MEMORY_LIMIT,
            max_table_elements: abi::DEFAULT_TABLE_LIMIT,
            max_tables: 1,
            max_instances: abi::DEFAULT_INSTANCE_LIMIT,
            max_memories: 1,
            max_block_frames: abi::MAX_BLOCK_FRAMES,
            max_log_messages_per_call: abi::MAX_LOG_MESSAGES_PER_CALL,
            max_log_bytes_per_call: abi::MAX_LOG_BYTES_PER_CALL,
            max_log_message_bytes: abi::MAX_LOG_MESSAGE_BYTES,
        }
    }
}

impl ProcessLimits {
    /// The deadline in epoch ticks, which is the resolution the timeout really
    /// has: the engine's ticker advances once per [`abi::EPOCH_TICK_MS`].
    pub(crate) fn deadline_ticks(&self) -> u64 {
        let tick = abi::EPOCH_TICK_MS.max(1);
        let millis = self.timeout.as_millis() as u64;
        (millis / tick).max(1)
    }
}

/// What an [`crate::Engine`] is built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostConfig {
    pub limits: ProcessLimits,
    /// How many log records an instance keeps for the user to read. Records
    /// beyond this are dropped and counted.
    pub log_capacity: usize,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            limits: ProcessLimits::default(),
            log_capacity: 256,
        }
    }
}

/// The store state of one instance: the WASI context the module may talk to
/// (which knows nothing), the limits, the log, and the flags the host needs to
/// explain a failure afterwards.
pub(crate) struct StoreData {
    pub(crate) limits: DataLimits,
    pub(crate) wasi: WasiP1Ctx,
    pub(crate) log: LogCollector,
    /// Set by the epoch callback, which only fires when a call overran.
    pub(crate) timed_out: bool,
    /// The counter behind the deterministic `clock_time_get`.
    pub(crate) clock_ticks: u64,
    /// The state behind the deterministic `random_get`.
    pub(crate) random_state: u64,
}

impl StoreData {
    pub(crate) fn new(limits: &ProcessLimits, log_capacity: usize, wasi: WasiP1Ctx) -> Self {
        Self {
            limits: DataLimits::new(limits),
            wasi,
            log: LogCollector::new(log_capacity, limits),
            timed_out: false,
            clock_ticks: 0,
            random_state: crate::engine::DETERMINISTIC_RANDOM_START,
        }
    }
}

/// [`StoreLimits`] with a flag: Wasmtime tells the limiter when a growth it
/// allowed failed, and when it refuses one, and both are the host's cue to
/// report `OutOfMemory` instead of a bare trap.
pub(crate) struct DataLimits {
    inner: StoreLimits,
    hit: Arc<AtomicBool>,
}

impl DataLimits {
    fn new(limits: &ProcessLimits) -> Self {
        let inner = StoreLimitsBuilder::new()
            .memory_size(limits.max_memory_bytes)
            .table_elements(limits.max_table_elements)
            .instances(limits.max_instances)
            .tables(limits.max_tables)
            .memories(limits.max_memories)
            .build();
        Self {
            inner,
            hit: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A handle on the flag, for the code that runs after the store is gone.
    pub(crate) fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.hit)
    }

    fn refused(&self) {
        self.hit.store(true, Ordering::SeqCst);
    }
}

impl ResourceLimiter for DataLimits {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = self.inner.memory_growing(current, desired, maximum)?;
        if !allowed {
            self.refused();
        }
        Ok(allowed)
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        // Returning the error makes `memory.grow` trap rather than quietly
        // return -1, which gives the user a sentence instead of a mystery.
        self.refused();
        Err(error)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        self.inner.table_growing(current, desired, maximum)
    }

    fn instances(&self) -> usize {
        self.inner.instances()
    }

    fn tables(&self) -> usize {
        self.inner.tables()
    }

    fn memories(&self) -> usize {
        self.inner.memories()
    }
}

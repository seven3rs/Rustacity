// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sato
// SPDX-License-Identifier: GPL-2.0-or-later

//! The one error type of the extension engine.
//!
//! Every public function of the host returns one of these, and the C ABI turns
//! each into the integer status in [`ExtError::status`]. The mapping is part of
//! the published contract, so it lives beside the enum rather than in the glue
//! that happens to need it.

/// The status a successful call returns.
pub const STATUS_OK: i32 = 0;

/// Everything that can go wrong loading or running an extension.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ExtError {
    /// A bug in this engine, or a panic caught at a boundary.
    #[error("internal error: {0}")]
    Internal(String),

    /// `extension.toml` is missing, unreadable, or breaks a rule of the API.
    #[error("manifest: {0}")]
    ManifestInvalid(String),

    /// The `.wasm` is not a module, or imports something it may not.
    #[error("module: {0}")]
    ModuleInvalid(String),

    /// An export is missing, mistyped, or reports the wrong API version.
    #[error("ABI: {0}")]
    AbiMismatch(String),

    /// A capability is unknown, reserved, or not held.
    #[error("capability: {0}")]
    CapabilityDenied(String),

    /// The module trapped: `unreachable`, an out-of-bounds access, a division
    /// by zero, or `proc_exit`.
    #[error("the extension trapped: {0}")]
    Trap(String),

    /// One call ran longer than its wall-clock budget.
    #[error("the extension timed out: {0}")]
    Timeout(String),

    /// The module asked for more memory than the limit allows.
    #[error("out of memory: {0}")]
    OutOfMemory(String),

    /// `rx_init` or `rx_process` returned a non-zero code.
    #[error("the extension reported error code {0}")]
    ModuleReportedError(i32),

    /// Reading the extension's files failed.
    #[error("i/o: {0}")]
    Io(String),

    /// An earlier call on this instance failed; it will not be used again.
    #[error("this instance is spent: {0}")]
    Poisoned(String),

    /// A parameter index or value the manifest does not allow.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

impl ExtError {
    /// The integer status this error becomes in the C ABI.
    pub fn status(&self) -> i32 {
        match self {
            ExtError::Internal(_) => 1,
            ExtError::ManifestInvalid(_) => 2,
            ExtError::ModuleInvalid(_) => 3,
            ExtError::AbiMismatch(_) => 4,
            ExtError::CapabilityDenied(_) => 5,
            ExtError::Trap(_) => 6,
            ExtError::Timeout(_) => 7,
            ExtError::OutOfMemory(_) => 8,
            ExtError::ModuleReportedError(_) => 9,
            ExtError::Io(_) => 10,
            ExtError::Poisoned(_) => 11,
            ExtError::InvalidArgument(_) => 12,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers are published in docs/extensions/API.md and in
    /// rust/ext-host/include/rustacity_ext.h; they may not drift.
    #[test]
    fn statuses_are_the_published_ones() {
        let cases = [
            (ExtError::Internal(String::new()), 1),
            (ExtError::ManifestInvalid(String::new()), 2),
            (ExtError::ModuleInvalid(String::new()), 3),
            (ExtError::AbiMismatch(String::new()), 4),
            (ExtError::CapabilityDenied(String::new()), 5),
            (ExtError::Trap(String::new()), 6),
            (ExtError::Timeout(String::new()), 7),
            (ExtError::OutOfMemory(String::new()), 8),
            (ExtError::ModuleReportedError(3), 9),
            (ExtError::Io(String::new()), 10),
            (ExtError::Poisoned(String::new()), 11),
            (ExtError::InvalidArgument(String::new()), 12),
        ];
        for (error, status) in cases {
            assert_eq!(error.status(), status, "{error:?}");
            assert_ne!(status, STATUS_OK);
        }
    }
}

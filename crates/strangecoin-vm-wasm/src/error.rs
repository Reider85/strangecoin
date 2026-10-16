use std::fmt;

use strangecoin_core::vm::VmError;
use thiserror::Error;

/// Errors the wasmi-backed executor can fail with.
///
/// Converts into [`VmError`] via [`Into<VmError>`], the bound required by
/// `strangecoin_core::vm::VmExecutor`.
#[derive(Debug, Error)]
pub enum WasmiError {
    #[error("no contract code registered for address `{address}`")]
    NoCode { address: String },

    #[error("out of gas")]
    OutOfFuel,

    #[error("wasmi engine error: {0}")]
    Engine(String),

    #[error("module instantiation failed: {0}")]
    Instantiate(String),

    #[error("missing export `{0}`")]
    MissingExport(String),

    #[error("type mismatch: {0}")]
    TypeMismatch(String),

    #[error(transparent)]
    Core(#[from] VmError),
}

/// A typed [`VmError`] transported through wasmi's host-error channel.
///
/// Host functions (`call`, `storage_write`, …) can only return
/// `wasmi::Error`, so security-critical failures (reentrancy, depth and
/// storage limits, S1.5-P13) are wrapped in this carrier and recovered in
/// [`From<wasmi::Error> for WasmiError`] via downcast — no string parsing,
/// the precise `VmError` variant survives the engine round-trip.
#[derive(Debug)]
pub struct VmHostError(pub VmError);

impl fmt::Display for VmHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for VmHostError {}

impl wasmi::errors::HostError for VmHostError {}

impl From<wasmi::Error> for WasmiError {
    fn from(err: wasmi::Error) -> Self {
        // Typed host failure (S1.5-P13): recover the exact variant.
        if let Some(host) = err.downcast_ref::<VmHostError>() {
            return WasmiError::Core(host.0.clone());
        }
        match err.as_trap_code() {
            Some(wasmi::TrapCode::OutOfFuel) => WasmiError::OutOfFuel,
            // Resource limits (S1.5-P13): runaway recursion overflows the
            // stack, memory growth past `MAX_MEMORY_BYTES` is denied by the
            // store limiter — both surface as an out-of-memory condition.
            Some(wasmi::TrapCode::StackOverflow | wasmi::TrapCode::GrowthOperationLimited) => {
                WasmiError::Core(VmError::OutOfMemory)
            }
            _ => WasmiError::Engine(err.to_string()),
        }
    }
}

impl From<WasmiError> for VmError {
    fn from(err: WasmiError) -> Self {
        match err {
            WasmiError::OutOfFuel => VmError::OutOfGas,
            WasmiError::Core(inner) => inner,
            WasmiError::NoCode { address } => VmError::HostFunctionError {
                name: "execute".to_string(),
                reason: format!("no contract code for address `{address}`"),
            },
            other => VmError::HostFunctionError {
                name: "wasmi".to_string(),
                reason: other.to_string(),
            },
        }
    }
}

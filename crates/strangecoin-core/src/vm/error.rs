use thiserror::Error;

/// Errors a VM execution can fail with.
///
/// Every concrete runtime error type (e.g. `WasmiError` in
/// `strangecoin-vm-wasm`) converts into this enum via `Into<VmError>`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VmError {
    #[error("out of gas")]
    OutOfGas,
    #[error("out of memory")]
    OutOfMemory,
    #[error("execution reverted")]
    Reverted,
    #[error("host function `{name}` failed: {reason}")]
    HostFunctionError { name: String, reason: String },
    #[error("reentrancy detected")]
    ReentrancyDetected,
    #[error("max call depth exceeded")]
    MaxCallDepth,
    #[error("contract code exceeds max code size")]
    MaxCodeSize,
    #[error("contract storage exceeds max size per contract")]
    MaxStoragePerContract,
    #[error("gas metering inconsistency detected")]
    GasMeteringBug,
}

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

impl From<wasmi::Error> for WasmiError {
    fn from(err: wasmi::Error) -> Self {
        if err.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
            return WasmiError::OutOfFuel;
        }
        WasmiError::Engine(err.to_string())
    }
}

impl From<WasmiError> for VmError {
    fn from(err: WasmiError) -> Self {
        match err {
            WasmiError::OutOfFuel => VmError::OutOfGas,
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

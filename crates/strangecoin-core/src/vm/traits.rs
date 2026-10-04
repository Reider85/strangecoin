use crate::types::{AccountState, Block};

/// Execution surface for smart contracts (Stage 1.5+).
///
/// The core crate depends on this trait, never on a concrete VM runtime.
/// Implementations (wasmi, etc.) live outside `strangecoin-core`.
pub trait VmExecutor {
    type Error;

    fn execute(
        &self,
        block: &Block,
        accounts: &std::collections::HashMap<String, AccountState>,
    ) -> Result<(), Self::Error>;
}

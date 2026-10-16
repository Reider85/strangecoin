use std::collections::HashMap;

use crate::types::{AccountState, BlockHeader};

/// Deterministic view of the block being executed.
///
/// Host function `block_context` (S1.5-P10) returns these fields; `coinbase`
/// is carried explicitly since the header does not commit to it directly.
#[derive(Clone, Debug)]
pub struct BlockContext {
    pub height: u64,
    pub timestamp: u64,
    pub coinbase: String,
    pub block_gas_limit: u64,
}

/// Everything one contract call frame sees.
///
/// Owned by value: the executor (S1.5-P06) consumes it and returns an
/// [`ExecutionResult`](crate::vm::ExecutionResult); nested calls
/// (S1.5-P10) derive child contexts from this one with `depth + 1`.
#[derive(Clone, Debug)]
pub struct ExecutionContext {
    /// Header view of the executing block — full `Block` bodies are excluded
    /// so execution cannot depend on sibling transactions.
    pub block: BlockHeader,
    pub accounts: HashMap<String, AccountState>,
    pub caller: String,
    pub callee: String,
    pub value: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub depth: u32,
    /// True when this frame re-enters a contract already on the active call
    /// stack (S1.5-P13, threat V-18). Set by the nested-call host functions;
    /// a `non_reentrant` ABI entry turns the situation into
    /// [`VmError::ReentrancyDetected`](crate::vm::VmError::ReentrancyDetected).
    pub reentrant: bool,
    /// Read-only frame: set by `staticcall` (S1.5-P10); `storage_write` and
    /// state-mutating host functions must refuse to run while this is true.
    pub read_only: bool,
    pub block_context: BlockContext,
    /// Contract storage writes staged by this frame, flushed to the SMT on
    /// success (S1.5-P15), discarded on revert.
    pub storage_overlay: HashMap<[u8; 32], Vec<u8>>,
    /// Calldata of this frame: `CallTx.data` for the top-level call, or the
    /// `data` argument of a nested `call`/`staticcall`/`delegatecall`.
    /// Precompiles (S1.5-P11) read their input here; the JSON ABI (S1.5-P12)
    /// decodes it for WASM contracts.
    pub input: Vec<u8>,
}

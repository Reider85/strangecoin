use serde::{Deserialize, Serialize};

/// Terminal status of one transaction execution.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxStatus {
    Success,
    Reverted,
}

/// Event emitted by a contract during execution (`emit_log`, S1.5-P10).
///
/// Lives in a [`Receipt`] (invariant #18: receipts are immutable once
/// the block is applied).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Log {
    /// bech32 address of the emitting contract.
    pub address: String,
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}

/// Outcome of a single VM execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionResult {
    pub gas_used: u64,
    pub logs: Vec<Log>,
    pub status: TxStatus,
    pub return_data: Vec<u8>,
}

/// Record of a transaction's execution, committed alongside the block
/// (S1.5-P14).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub gas_used: u64,
    pub logs: Vec<Log>,
    pub status: TxStatus,
}

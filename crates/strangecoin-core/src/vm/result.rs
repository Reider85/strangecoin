use serde::{Deserialize, Serialize};

use crate::vm::error::VmError;

/// Terminal status of one transaction execution.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxStatus {
    Success,
    Reverted,
    OutOfGas,
    Failed,
}

impl TxStatus {
    pub fn canonical_byte(self) -> u8 {
        match self {
            TxStatus::Success => 0,
            TxStatus::Reverted => 1,
            TxStatus::OutOfGas => 2,
            TxStatus::Failed => 3,
        }
    }
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
/// (S1.5-P14, invariant #18): once the block is applied a receipt is never
/// mutated — reorgs recompute it from the chain instead.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub txid: [u8; 32],
    pub gas_used: u64,
    pub logs: Vec<Log>,
    pub status: TxStatus,
    pub block_height: u64,
    pub tx_index: u32,
}

impl Receipt {
    /// Build the receipt of a successful (or reverted) VM execution.
    pub fn from_execution(
        txid: [u8; 32],
        block_height: u64,
        tx_index: u32,
        result: &ExecutionResult,
    ) -> Self {
        Receipt {
            txid,
            gas_used: result.gas_used,
            logs: result.logs.clone(),
            status: result.status,
            block_height,
            tx_index,
        }
    }

    /// Build the receipt of an exceptional VM failure: the frame burns its
    /// full gas budget and emits no logs (logs of a failed frame are
    /// discarded by the executor).
    pub fn from_vm_error(
        txid: [u8; 32],
        block_height: u64,
        tx_index: u32,
        err: &VmError,
        gas_limit: u64,
    ) -> Self {
        let status = match err {
            VmError::OutOfGas => TxStatus::OutOfGas,
            _ => TxStatus::Failed,
        };
        Receipt {
            txid,
            gas_used: gas_limit,
            logs: Vec::new(),
            status,
            block_height,
            tx_index,
        }
    }
}

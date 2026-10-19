use serde::{Deserialize, Serialize};

use crate::state::ContractStorageMap;
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

/// One balance move committed during execution (S1.5-FBP05): the paying
/// account, the receiving account, and the amount.
///
/// An execution's [`ExecutionResult::value_transfers`] lists every move in
/// commit order — the top-level frame's entry transfer first, then each
/// nested `call` transfer as it settles. `apply_call`/`apply_create` commit
/// the list to `State` on `Success`; `unapply` reverses it in reverse order
/// (invariant #4). The same list is mirrored into the [`Receipt`] so the
/// audit trail is visible to light clients once receipts fold into the
/// header (S1.5-FBP07).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ValueTransfer {
    pub from: String,
    pub to: String,
    pub amount: u64,
}

/// Outcome of a single VM execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionResult {
    pub gas_used: u64,
    pub logs: Vec<Log>,
    pub status: TxStatus,
    pub return_data: Vec<u8>,
    /// Contract storage writes staged during this execution, per contract
    /// address (S1.5-P15): `apply_call` flushes every contract scope into
    /// `State.contract_storage` on `Success` and discards the map on
    /// failure. Empty for precompiles and transfer-only frames.
    pub storage_overlay: ContractStorageMap,
    /// Value moves settled during this execution, in commit order
    /// (S1.5-FBP05): the top-level frame's entry transfer (when `value > 0`)
    /// followed by every nested `call` transfer that was not reverted. A
    /// failed frame's transfers are reversed inside the VM and never reach
    /// this list; `apply_call` commits it to `State` only on `Success`.
    pub value_transfers: Vec<ValueTransfer>,
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
    /// Audit trail of the value moves this transaction committed, in commit
    /// order (S1.5-FBP05): empty for failed frames, coinbase mints and
    /// transfer-free calls; a successful `Transfer` carries its single move;
    /// a successful `Call`/`Create` carries the entry transfer (value /
    /// endowment) followed by every settled nested transfer. Mirrors
    /// `ExecutionResult::value_transfers` (plus the top-level move for
    /// transfer-only paths). `serde(default)` keeps older JSON decodable.
    #[serde(default)]
    pub value_transfers: Vec<ValueTransfer>,
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
            value_transfers: result.value_transfers.clone(),
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
            value_transfers: Vec::new(),
        }
    }
}

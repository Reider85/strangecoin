//! Shared read-only (`eth_call`-style) VM execution path (UI-P03).
//!
//! Extracted from the MCP adapter so both the MCP `call_contract` tool and the
//! in-process wallet backend run exactly the same isolated frame. No state is
//! committed: the execution context gets a snapshot of accounts and contract
//! storage, and `read_only = true` forbids writes in the VM.

use strangecoin_core::vm::ExecutionResult;

use crate::BlockchainFacade;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOnlyError {
    NotFound(String),
    Rejected(String),
}

#[cfg(feature = "vm-wasm")]
pub fn read_only_call(
    facade: &BlockchainFacade,
    address: &str,
    calldata: Vec<u8>,
    gas_limit: u64,
) -> Result<ExecutionResult, ReadOnlyError> {
    use std::sync::Arc;
    use strangecoin_core::vm::gas::BLOCK_GAS_LIMIT;
    use strangecoin_core::vm::{BlockContext, ExecutionContext, VmExecutor};

    let account = facade
        .get_account(address)
        .ok_or_else(|| ReadOnlyError::NotFound(format!("no account `{address}`")))?;
    if !account.is_contract() {
        // EOA target mirrors `apply_call` transfer-only path: zero gas, empty
        // return data (`eth_call` semantics).
        return Ok(ExecutionResult {
            gas_used: 0,
            return_data: Vec::new(),
            logs: Vec::new(),
            status: strangecoin_core::vm::TxStatus::Success,
            storage_overlay: Default::default(),
            value_transfers: Vec::new(),
        });
    }

    let code = facade
        .with_inner(|bc| bc.codes.get(address).cloned())
        .ok_or_else(|| ReadOnlyError::NotFound(format!("no code for `{address}`")))?;
    let tip = facade
        .tip()
        .ok_or_else(|| ReadOnlyError::NotFound("chain has no tip".into()))?;

    let state = facade.state_snapshot().to_state();
    let mut executor = strangecoin_vm_wasm::WasmiExecutor::new();
    executor.insert_code(address, code);

    let ctx = ExecutionContext {
        block: tip.header(),
        accounts: state.balances.clone(),
        caller: address.to_string(),
        callee: address.to_string(),
        value: 0,
        gas_limit,
        gas_used: 0,
        depth: 0,
        reentrant: false,
        read_only: true,
        block_context: BlockContext {
            height: tip.index,
            timestamp: tip.timestamp,
            coinbase: String::new(),
            block_gas_limit: BLOCK_GAS_LIMIT,
        },
        storage: Arc::clone(&state.contract_storage),
        storage_overlay: Default::default(),
        storage_address: address.to_string(),
        input: calldata,
    };

    executor
        .execute(ctx)
        .map_err(|err| ReadOnlyError::Rejected(format!("vm execution failed: {err}")))
}

#[cfg(not(feature = "vm-wasm"))]
pub fn read_only_call(
    _facade: &BlockchainFacade,
    _address: &str,
    _calldata: Vec<u8>,
    _gas_limit: u64,
) -> Result<ExecutionResult, ReadOnlyError> {
    Err(ReadOnlyError::Rejected(
        "node built without the `vm-wasm` feature".into(),
    ))
}

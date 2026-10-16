//! Nested-call host functions (S1.5-P10): `call`, `delegatecall`,
//! `staticcall`.
//!
//! All three share [`nested_call`], which derives a child
//! [`ExecutionContext`] from the parent's host state (`depth + 1`), runs the
//! callee (precompile, WASM module, or plain EOA) with a slice of the
//! parent's remaining fuel, and merges the child's storage overlay, accounts,
//! and logs back into the parent on success. On failure only gas is consumed
//! — the parent's state is untouched (revert semantics).
//!
//! Nested-call semantics are VM-level; wiring the overlay into the Sparse
//! Merkle Tree and real contract accounts is S1.5-P15. The reentrancy guard
//! and `max_call_depth` enforcement land in S1.5-P13 on top of this module.

pub mod call;
pub mod delegatecall;
pub mod staticcall;

use std::collections::HashMap;
use std::rc::Rc;

use strangecoin_core::address::is_precompile_address;
use strangecoin_core::types::AccountState;
use strangecoin_core::vm::{TxStatus, VmError};
use wasmi::Caller;

use crate::gas::HostCallWeight;
use crate::host::{abi, charge, checked_u32, HostState};
use crate::precompiles::{dispatch_precompile, PrecompileError};
use crate::{run_frame, FrameOutcome};

pub fn register(linker: &mut wasmi::Linker<HostState>) -> Result<(), wasmi::Error> {
    call::register(linker)?;
    delegatecall::register(linker)?;
    staticcall::register(linker)?;
    Ok(())
}

/// Which nested-call variant is executing (differs in context inheritance).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallKind {
    Call,
    DelegateCall,
    StaticCall,
}

impl CallKind {
    fn base_weight(self) -> HostCallWeight {
        match self {
            CallKind::Call => HostCallWeight::Call,
            CallKind::DelegateCall => HostCallWeight::DelegateCall,
            CallKind::StaticCall => HostCallWeight::StaticCall,
        }
    }
}

/// Status codes returned to the calling contract (ABI convention).
pub const STATUS_OK: i32 = 0;
/// Child frame reverted (or a precompile returned `NotImplemented`/
/// `InvalidInput`).
pub const STATUS_REVERTED: i32 = 1;
/// Child frame failed exceptionally (trap, out of gas, no value).
pub const STATUS_FAILED: i32 = 2;

/// Cap on the UTF-8 length of a nested-call target address (bech32 strings
/// are ~45–63 chars; precompile addresses are 4). Defense in depth against
/// a malicious module feeding megabyte-long targets (threat V-21).
const MAX_TARGET_LEN: usize = 128;

/// Shared body of `call`/`delegatecall`/`staticcall` (S1.5-P10).
#[allow(clippy::too_many_arguments)]
pub(crate) fn nested_call(
    caller: &mut Caller<'_, HostState>,
    kind: CallKind,
    to_ptr: i32,
    to_len: i32,
    value: i64,
    data_ptr: i32,
    data_len: i32,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, wasmi::Error> {
    charge(caller, kind.base_weight())?;
    let to_ptr = checked_u32(to_ptr, "to_ptr")?;
    let to_len = checked_u32(to_len, "to_len")?;
    let data_ptr = checked_u32(data_ptr, "data_ptr")?;
    let data_len = checked_u32(data_len, "data_len")?;
    let out_ptr = checked_u32(out_ptr, "out_ptr")?;
    let out_len = checked_u32(out_len, "out_len")?;

    if to_len as usize > MAX_TARGET_LEN {
        return Err(wasmi::Error::new("call: target address too long"));
    }
    let value = u64::try_from(value).map_err(|_| wasmi::Error::new("call: negative value"))?;

    let to_bytes = abi::read_bytes(caller, to_ptr, to_len)?;
    let to = String::from_utf8(to_bytes)
        .map_err(|_| wasmi::Error::new("call: target address is not UTF-8"))?;
    let data = abi::read_bytes(caller, data_ptr, data_len)?;

    // Snapshot what the child frame needs; the parent borrows end here so
    // the (potentially long) child execution does not hold the store.
    let (mut child_ctx, gas_budget, code, child_call_stack, parent_callee) = {
        let state = caller.data();
        let fuel = caller.get_fuel()?;
        let mut stack = state.call_stack.clone();
        stack.push(to.clone());
        let mut child = state.ctx.clone();
        child.gas_limit = fuel;
        child.gas_used = 0;
        child.depth = state.ctx.depth.saturating_add(1);
        child.input = data.clone();
        child.read_only = state.ctx.read_only || kind == CallKind::StaticCall;
        match kind {
            CallKind::Call => {
                child.caller = state.ctx.callee.clone();
                child.callee = to.clone();
                child.value = value;
            }
            CallKind::DelegateCall => {
                // Keep the outer caller identity and the parent's value;
                // only the code source (`callee`) changes.
                child.value = state.ctx.value;
                child.callee = to.clone();
            }
            CallKind::StaticCall => {
                child.caller = state.ctx.callee.clone();
                child.callee = to.clone();
                child.value = 0;
            }
        }
        (
            child,
            fuel,
            Rc::clone(&state.code),
            stack,
            state.ctx.callee.clone(),
        )
    };

    // Value transfer (`call` only): from the executing contract to `to`.
    let mut value_ok = true;
    if kind == CallKind::Call && value > 0 {
        value_ok = transfer_value(&mut child_ctx.accounts, &parent_callee, &to, value);
    }
    // Snapshot the (possibly value-transferred) accounts so success paths
    // that do not run WASM still merge the transfer back into the parent.
    let child_accounts = child_ctx.accounts.clone();

    let (status, gas_used, return_data, overlay, accounts, logs) = if !value_ok {
        // Insufficient balance: exceptional failure, full child budget burn.
        (STATUS_FAILED, gas_budget, Vec::new(), None, None, Vec::new())
    } else if is_precompile_address(&to) {
        let (status, gas_used, return_data) = run_precompile_nested(&to, &data, gas_budget);
        let accounts = (status == STATUS_OK).then(|| child_accounts.clone());
        (status, gas_used, return_data, None, accounts, Vec::new())
    } else {
        match code.get(&to).cloned() {
            Some(wasm) => match run_frame(&wasm, child_ctx, code, child_call_stack) {
                Ok(FrameOutcome {
                    result,
                    storage_overlay,
                    accounts,
                    logs,
                    return_data,
                }) => {
                    let status = match result.status {
                        TxStatus::Success => STATUS_OK,
                        TxStatus::Reverted => STATUS_REVERTED,
                    };
                    (
                        status,
                        result.gas_used,
                        return_data,
                        Some(storage_overlay),
                        Some(accounts),
                        logs,
                    )
                }
                Err(crate::WasmiError::NoCode { .. }) => {
                    // Treated as a plain EOA transfer: success, no code.
                    (STATUS_OK, 0, Vec::new(), None, Some(child_accounts.clone()), Vec::new())
                }
                Err(err) => {
                    let _: VmError = err.into();
                    (STATUS_FAILED, gas_budget, Vec::new(), None, None, Vec::new())
                }
            },
            // No code registered for `to`: EOA — success, empty return.
            None => (STATUS_OK, 0, Vec::new(), None, Some(child_accounts.clone()), Vec::new()),
        }
    };

    // Charge the child's gas against the parent's fuel.
    let parent_fuel = caller.get_fuel()?;
    caller.set_fuel(parent_fuel.saturating_sub(gas_used))?;

    // Merge child state into the parent on success only.
    let state = caller.data_mut();
    if status == STATUS_OK {
        if let Some(overlay) = overlay {
            state.ctx.storage_overlay = overlay;
        }
        if let Some(accounts) = accounts {
            state.ctx.accounts = accounts;
        }
        state.logs.extend(logs);
    }
    state.last_return_data = return_data.clone();

    // Copy as much return data as fits into the caller's out-buffer.
    let n = return_data.len().min(out_len as usize);
    if n > 0 {
        abi::write_bytes(caller, out_ptr, &return_data[..n])?;
    }
    Ok(status)
}

/// Precompile fast-path of a nested call (mirrors ADR-0015 semantics).
fn run_precompile_nested(to: &str, data: &[u8], gas_budget: u64) -> (i32, u64, Vec<u8>) {
    match dispatch_precompile(to, data) {
        Ok(out) => {
            if out.gas_used > gas_budget {
                (STATUS_FAILED, gas_budget, Vec::new())
            } else {
                (STATUS_OK, out.gas_used, out.output)
            }
        }
        Err(PrecompileError::NotImplemented { gas_used, .. }) => {
            (STATUS_REVERTED, gas_used.min(gas_budget), Vec::new())
        }
        Err(PrecompileError::InvalidInput(_)) => (STATUS_REVERTED, gas_budget, Vec::new()),
    }
}

/// Move `value` from `from` to `to`. Returns `false` (and changes nothing)
/// when `from` cannot cover the amount.
fn transfer_value(
    accounts: &mut HashMap<String, AccountState>,
    from: &str,
    to: &str,
    value: u64,
) -> bool {
    let from_balance = accounts.get(from).map(AccountState::balance).unwrap_or(0);
    if from_balance < value {
        return false;
    }
    if let Some(acc) = accounts.get_mut(from) {
        acc.set_balance(from_balance - value);
    }
    let recipient = accounts
        .entry(to.to_string())
        .or_default();
    recipient.set_balance(recipient.balance().saturating_add(value));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn funded(n: u64) -> HashMap<String, AccountState> {
        let mut accounts = HashMap::new();
        accounts.insert("a".to_string(), AccountState::eoa(n, 0));
        accounts
    }

    #[test]
    fn transfer_moves_balance_between_accounts() {
        let mut accounts = funded(100);
        assert!(transfer_value(&mut accounts, "a", "b", 40));
        assert_eq!(accounts["a"].balance(), 60);
        assert_eq!(accounts["b"].balance(), 40);
    }

    #[test]
    fn transfer_rejects_insufficient_balance_without_side_effects() {
        let mut accounts = funded(10);
        assert!(!transfer_value(&mut accounts, "a", "b", 11));
        assert_eq!(accounts["a"].balance(), 10);
        assert!(!accounts.contains_key("b"));
    }

    #[test]
    fn transfer_zero_creates_recipient() {
        let mut accounts = funded(10);
        assert!(transfer_value(&mut accounts, "a", "b", 0));
        assert_eq!(accounts["a"].balance(), 10);
        assert_eq!(accounts["b"].balance(), 0);
    }
}

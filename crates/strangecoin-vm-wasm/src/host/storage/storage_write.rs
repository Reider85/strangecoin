//! `storage_write(key_ptr, val_ptr, val_len) -> i32`.
//!
//! Writes `value` under the 32-byte key into the frame's scoped contract
//! storage overlay (S1.5-P15: keyed by `ctx.storage_address`). Returns `1`
//! on success. Refuses to run in a read-only frame (the `staticcall`
//! context, S1.5-P10). Gas: 5000 (ADR-0014) — writes must hurt so storage
//! spam stays expensive. The per-contract size cap
//! (`MAX_STORAGE_PER_CONTRACT`, threat V-20) counts the persisted snapshot
//! plus the staged overlay: a write that would push the contract's total
//! value bytes past 1 MB is rejected (S1.5-P13).

use strangecoin_core::vm::limits::MAX_STORAGE_PER_CONTRACT;
use strangecoin_core::vm::VmError;
use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{abi, charge, checked_u32, vm_err, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "storage_write", storage_write)
        .map(|_| ())
        .map_err(Into::into)
}

fn storage_write(
    mut caller: Caller<'_, HostState>,
    key_ptr: i32,
    val_ptr: i32,
    val_len: i32,
) -> Result<i32, wasmi::Error> {
    charge(&mut caller, HostCallWeight::StorageWrite)?;
    let key_ptr = checked_u32(key_ptr, "key_ptr")?;
    let val_ptr = checked_u32(val_ptr, "val_ptr")?;
    let val_len = checked_u32(val_len, "val_len")?;

    let key = abi::read_array32(&caller, key_ptr)?;
    let value = abi::read_bytes(&caller, val_ptr, val_len)?;

    let state = caller.data_mut();
    if state.ctx.read_only {
        return Err(wasmi::Error::new(
            "storage_write: called from a read-only (staticcall) frame",
        ));
    }
    // Per-contract storage cap (threat V-20): persisted snapshot + staged
    // writes for this contract must stay within MAX_STORAGE_PER_CONTRACT.
    // S1.5-FBP06: the persisted snapshot is read through the shared
    // handle under a short-lived `read()` guard.
    let address = state.ctx.storage_address.clone();
    let persisted: usize = state
        .ctx
        .storage
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&address)
        .map(|storage| storage.values().map(Vec::len).sum())
        .unwrap_or(0);
    let overlay = state.ctx.storage_overlay.entry(address).or_default();
    let replaced = overlay.get(&key).map_or(0, Vec::len);
    let staged: usize = overlay.values().map(Vec::len).sum::<usize>().saturating_sub(replaced);
    if persisted.saturating_add(staged).saturating_add(value.len()) > MAX_STORAGE_PER_CONTRACT {
        return Err(vm_err(VmError::MaxStoragePerContract));
    }
    overlay.insert(key, value);
    Ok(1)
}

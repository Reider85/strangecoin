//! `storage_write(key_ptr, val_ptr, val_len) -> i32`.
//!
//! Writes `value` under the 32-byte key into the frame's storage overlay.
//! Returns `1` on success. Refuses to run in a read-only frame (the
//! `staticcall` context, S1.5-P10). Gas: 5000 (ADR-0014) — writes must hurt
//! so storage spam stays expensive. The per-contract size cap
//! (`MAX_STORAGE_PER_CONTRACT`, threat V-20) rejects a write that would push
//! the overlay's total value bytes past 1 MB (S1.5-P13).

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
    // Per-contract storage cap (threat V-20): total value bytes staged for
    // this contract must stay within MAX_STORAGE_PER_CONTRACT.
    let replaced = state.ctx.storage_overlay.get(&key).map_or(0, Vec::len);
    let other: usize = state
        .ctx
        .storage_overlay
        .values()
        .map(Vec::len)
        .sum::<usize>()
        .saturating_sub(replaced);
    if other.saturating_add(value.len()) > MAX_STORAGE_PER_CONTRACT {
        return Err(vm_err(VmError::MaxStoragePerContract));
    }
    state.ctx.storage_overlay.insert(key, value);
    Ok(1)
}

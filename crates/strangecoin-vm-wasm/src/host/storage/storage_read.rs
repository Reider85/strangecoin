//! `storage_read(key_ptr, out_ptr, out_len) -> i32`.
//!
//! Reads the 32-byte-keyed value from the frame's contract storage
//! (S1.5-P15, S1.5-FBP06: the base is a shared handle — read falls
//! through the staged write in the frame's scoped overlay to the shared
//! persisted snapshot under a short-lived `read()` guard, zero-copy of
//! the map). Returns the number of bytes written into the out-buffer;
//! `-1` when the buffer is too small (the contract can retry with a
//! larger one); `0` for an absent key. Gas: 200 (ADR-0014), charged
//! before the lookup.

use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{abi, charge, checked_u32, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "storage_read", storage_read)
        .map(|_| ())
        .map_err(Into::into)
}

fn storage_read(
    mut caller: Caller<'_, HostState>,
    key_ptr: i32,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, wasmi::Error> {
    charge(&mut caller, HostCallWeight::StorageRead)?;
    let key_ptr = checked_u32(key_ptr, "key_ptr")?;
    let out_ptr = checked_u32(out_ptr, "out_ptr")?;
    let out_len = checked_u32(out_len, "out_len")?;

    let key = abi::read_array32(&caller, key_ptr)?;
    let value = {
        let state = caller.data();
        let address = &state.ctx.storage_address;
        state
            .ctx
            .storage_overlay
            .get(address)
            .and_then(|overlay| overlay.get(&key))
            .cloned()
            .or_else(|| {
                state
                    .ctx
                    .storage
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .get(address)
                    .and_then(|storage| storage.get(&key))
                    .cloned()
            })
    };
    let Some(value) = value else {
        return Ok(0);
    };
    if value.len() > out_len as usize {
        return Ok(-1);
    }
    abi::write_bytes(&mut caller, out_ptr, &value)?;
    Ok(value.len() as i32)
}

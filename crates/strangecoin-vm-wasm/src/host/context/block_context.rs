//! `block_context(out_ptr, out_len) -> i32`.
//!
//! Writes a deterministic snapshot of the executing block into the caller's
//! out-buffer and returns the number of bytes written (`-1` when the buffer
//! is too small). Layout (all integers little-endian):
//!
//! ```text
//! [0..8)   height      (u64)
//! [8..16)  timestamp   (u64)
//! [16..24) gas_limit   (u64)
//! [24..)   coinbase    (UTF-8 bech32 bytes)
//! ```
//!
//! Timestamp is the block header's consensus timestamp, not wall-clock —
//! contracts must not observe real time (threat V-21 determinism). Gas: 3
//! (ADR-0014).

use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{abi, charge, checked_u32, HostState};

/// Fixed prefix of the block-context out-buffer: 3 × u64.
pub const BLOCK_CONTEXT_FIXED_LEN: usize = 24;

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "block_context", block_context)
        .map(|_| ())
        .map_err(Into::into)
}

fn block_context(
    mut caller: Caller<'_, HostState>,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, wasmi::Error> {
    charge(&mut caller, HostCallWeight::BlockContext)?;
    let out_ptr = checked_u32(out_ptr, "out_ptr")?;
    let out_len = checked_u32(out_len, "out_len")?;

    let (height, timestamp, gas_limit, coinbase) = {
        let state = caller.data();
        let bc = &state.ctx.block_context;
        (
            bc.height,
            bc.timestamp,
            bc.block_gas_limit,
            bc.coinbase.clone(),
        )
    };
    let coinbase_bytes = coinbase.as_bytes();

    let mut buf = Vec::with_capacity(BLOCK_CONTEXT_FIXED_LEN + coinbase_bytes.len());
    buf.extend_from_slice(&height.to_le_bytes());
    buf.extend_from_slice(&timestamp.to_le_bytes());
    buf.extend_from_slice(&gas_limit.to_le_bytes());
    buf.extend_from_slice(coinbase_bytes);

    if buf.len() > out_len as usize {
        return Ok(-1);
    }
    abi::write_bytes(&mut caller, out_ptr, &buf)?;
    Ok(buf.len() as i32)
}

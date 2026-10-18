//! `input_read(out_ptr, out_len) -> i32` — calldata of the current frame.
//!
//! Copies `ctx.input` (`CallTx.data` for the top-level frame, or the `data`
//! argument of a nested `call`/`staticcall`/`delegatecall`) into the
//! caller-provided out-buffer (S1.5-FBP03). Returns the number of bytes
//! written; `-1` when the buffer is too small (the contract can retry with a
//! larger one); `0` when the frame carries no calldata. Deterministic (pure
//! view of execution state). Gas: 3 (ADR-0014 `HOST_CALL_INPUT_READ`),
//! charged before the copy.

use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{charge, checked_u32, abi, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "input_read", input_read)
        .map(|_| ())
        .map_err(Into::into)
}

fn input_read(
    mut caller: Caller<'_, HostState>,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, wasmi::Error> {
    charge(&mut caller, HostCallWeight::InputRead)?;
    let out_ptr = checked_u32(out_ptr, "out_ptr")?;
    let out_len = checked_u32(out_len, "out_len")?;

    let input = {
        let state = caller.data();
        state.ctx.input.clone()
    };
    if input.len() > out_len as usize {
        return Ok(-1);
    }
    if input.is_empty() {
        return Ok(0);
    }
    abi::write_bytes(&mut caller, out_ptr, &input)?;
    Ok(input.len() as i32)
}

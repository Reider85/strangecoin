//! `delegatecall(to_ptr, to_len, data_ptr, data_len, out_ptr, out_len) -> i32`.
//!
//! Executes the callee's code in the caller's context: the outer `caller`
//! identity and the parent's `value` are preserved, and the storage overlay
//! is shared (same logical contract storage). No value transfer happens.
//! Gas: 700 base + the child frame's own consumption (ADR-0014).

use wasmi::{Caller, Linker};

use super::{nested_call, CallKind, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "delegatecall", delegatecall)
        .map(|_| ())
        .map_err(Into::into)
}

fn delegatecall(
    mut caller: Caller<'_, HostState>,
    to_ptr: i32,
    to_len: i32,
    data_ptr: i32,
    data_len: i32,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, wasmi::Error> {
    nested_call(
        &mut caller,
        CallKind::DelegateCall,
        to_ptr,
        to_len,
        0,
        data_ptr,
        data_len,
        out_ptr,
        out_len,
    )
}

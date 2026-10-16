//! `staticcall(to_ptr, to_len, data_ptr, data_len, out_ptr, out_len) -> i32`.
//!
//! Read-only nested call: the child frame runs with `read_only = true`, so
//! `storage_write`/`emit_log` refuse to execute. No value transfer. Gas:
//! 700 base + the child frame's own consumption (ADR-0014).

use wasmi::{Caller, Linker};

use super::{nested_call, CallKind, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "staticcall", staticcall)
        .map(|_| ())
        .map_err(Into::into)
}

fn staticcall(
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
        CallKind::StaticCall,
        to_ptr,
        to_len,
        0,
        data_ptr,
        data_len,
        out_ptr,
        out_len,
    )
}

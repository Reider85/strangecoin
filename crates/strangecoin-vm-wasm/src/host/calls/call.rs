//! `call(to_ptr, to_len, value, data_ptr, data_len, out_ptr, out_len) -> i32`.
//!
//! Executes the callee with a fresh child frame (`caller` = the currently
//! executing contract), transferring `value` first. Returns one of the
//! [`STATUS_*`](super) codes; on success the callee's return data is copied
//! into the out-buffer (truncated to `out_len`; the full data stays in
//! `last_return_data`). Gas: 700 base + the child frame's own consumption
//! (ADR-0014).

use wasmi::{Caller, Linker};

use super::{nested_call, CallKind, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "call", call)
        .map(|_| ())
        .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn call(
    caller: Caller<'_, HostState>,
    to_ptr: i32,
    to_len: i32,
    value: i64,
    data_ptr: i32,
    data_len: i32,
    out_ptr: i32,
    out_len: i32,
) -> Result<i32, wasmi::Error> {
    let mut caller = caller;
    nested_call(
        &mut caller,
        CallKind::Call,
        to_ptr,
        to_len,
        value,
        data_ptr,
        data_len,
        out_ptr,
        out_len,
    )
}

//! `blake3(in_ptr, in_len, out_ptr) -> i32` — BLAKE3-256, the chain's
//! native hash (same primitive as canonical serialization). Gas: 50 + 6 per
//! 64-byte chunk (ADR-0014).

use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{checked_u32, crypto::hash_host, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "blake3", blake3_hash)
        .map(|_| ())
        .map_err(Into::into)
}

fn blake3_hash(
    mut caller: Caller<'_, HostState>,
    in_ptr: i32,
    in_len: i32,
    out_ptr: i32,
) -> Result<i32, wasmi::Error> {
    let len = checked_u32(in_len, "in_len")?;
    let weight = HostCallWeight::Blake3 { bytes: len as u64 };
    hash_host(
        &mut caller,
        weight,
        |input| *blake3::hash(input).as_bytes(),
        in_ptr,
        in_len,
        out_ptr,
    )
}

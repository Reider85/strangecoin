//! `sha256(in_ptr, in_len, out_ptr) -> i32` — SHA-256 (FIPS 180-4). Gas:
//! 50 + 12 per 32-byte word (ADR-0014).

use sha2::{Digest, Sha256};
use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{crypto::hash_host, checked_u32, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "sha256", sha256_hash)
        .map(|_| ())
        .map_err(Into::into)
}

fn sha256_hash(
    mut caller: Caller<'_, HostState>,
    in_ptr: i32,
    in_len: i32,
    out_ptr: i32,
) -> Result<i32, wasmi::Error> {
    let len = checked_u32(in_len, "in_len")?;
    let weight = HostCallWeight::Sha256 {
        bytes: len as u64,
    };
    hash_host(
        &mut caller,
        weight,
        |input| Sha256::digest(input).into(),
        in_ptr,
        in_len,
        out_ptr,
    )
}

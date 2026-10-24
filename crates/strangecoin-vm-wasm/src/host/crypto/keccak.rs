//! `keccak(in_ptr, in_len, out_ptr) -> i32` — Keccak-256 (Ethereum padding
//! 0x01, not NIST SHA3; ADR-0015). Gas: 30 + 6 per 32-byte word (ADR-0014).

use tiny_keccak::{Hasher, Keccak};
use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{checked_u32, crypto::hash_host, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "keccak", keccak_hash)
        .map(|_| ())
        .map_err(Into::into)
}

fn keccak_hash(
    mut caller: Caller<'_, HostState>,
    in_ptr: i32,
    in_len: i32,
    out_ptr: i32,
) -> Result<i32, wasmi::Error> {
    let len = checked_u32(in_len, "in_len")?;
    let weight = HostCallWeight::Keccak { bytes: len as u64 };
    hash_host(
        &mut caller,
        weight,
        |input| {
            let mut hasher = Keccak::v256();
            let mut out = [0u8; 32];
            hasher.update(input);
            hasher.finalize(&mut out);
            out
        },
        in_ptr,
        in_len,
        out_ptr,
    )
}

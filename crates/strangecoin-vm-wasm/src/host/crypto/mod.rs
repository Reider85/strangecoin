//! Crypto host functions (S1.5-P10): `secp256k1_verify`, `bls_verify`,
//! `blake3`, `keccak`, `sha256`. Deterministic primitives only — the same
//! libraries the precompiles (ADR-0015) use, exposed for in-contract calls.

pub mod blake3;
pub mod bls_verify;
pub mod keccak;
pub mod secp256k1_verify;
pub mod sha256;

use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{abi, charge, checked_u32, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    secp256k1_verify::register(linker)?;
    bls_verify::register(linker)?;
    blake3::register(linker)?;
    keccak::register(linker)?;
    sha256::register(linker)?;
    Ok(())
}

/// Shared body of the three hash host functions: charge by input length,
/// read `(in_ptr, in_len)`, hash, write 32 bytes into `out_ptr`.
///
/// `weight` must already carry the input length (e.g.
/// `HostCallWeight::Blake3 { bytes }`). Returns `1` on success and `0` when
/// the out-buffer is too small for the digest (the contract can retry with a
/// larger buffer).
pub(crate) fn hash_host(
    caller: &mut Caller<'_, HostState>,
    weight: HostCallWeight,
    hash_fn: impl FnOnce(&[u8]) -> [u8; 32],
    in_ptr: i32,
    in_len: i32,
    out_ptr: i32,
) -> Result<i32, wasmi::Error> {
    charge(caller, weight)?;
    let in_len_u32 = checked_u32(in_len, "in_len")?;
    let in_ptr = checked_u32(in_ptr, "in_ptr")?;
    let out_ptr = checked_u32(out_ptr, "out_ptr")?;
    let input = abi::read_bytes(caller, in_ptr, in_len_u32)?;
    let digest = hash_fn(&input);
    if abi::write_bytes(caller, out_ptr, &digest).is_err() {
        return Ok(0);
    }
    Ok(1)
}

//! `0x03 keccak` — Keccak-256 digest, the Ethereum variant with padding
//! `0x01` (ADR-0015; *not* NIST SHA3-256).
//!
//! Input: arbitrary bytes. Output: 32-byte digest.
//! Gas: 30 + 6 per 32-byte word (`HostCallWeight::Keccak`).

use tiny_keccak::{Hasher as _, Keccak};

use super::{hash_gas, PrecompileError, PrecompileOutput};
use crate::gas::HostCallWeight;

pub fn run(data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    let gas_used = hash_gas(HostCallWeight::Keccak {
        bytes: data.len() as u64,
    });
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut output = [0u8; 32];
    hasher.finalize(&mut output);
    Ok(PrecompileOutput {
        output: output.to_vec(),
        gas_used,
    })
}

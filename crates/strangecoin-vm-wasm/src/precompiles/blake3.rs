//! `0x04 blake3` — blake3 hash, the chain-native function (ADR-0015;
//! same primitive as canonical serialization in core `serialize.rs`).
//!
//! Input: arbitrary bytes. Output: 32-byte digest.
//! Gas: 50 + 6 per 64-byte chunk (`HostCallWeight::Blake3`).

use super::{hash_gas, PrecompileError, PrecompileOutput};
use crate::gas::HostCallWeight;

pub fn run(data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    let gas_used = hash_gas(HostCallWeight::Blake3 {
        bytes: data.len() as u64,
    });
    let digest = blake3::hash(data);
    Ok(PrecompileOutput {
        output: digest.as_bytes().to_vec(),
        gas_used,
    })
}

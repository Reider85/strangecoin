//! `0x02 sha256` — SHA-256 digest (ADR-0015).
//!
//! Input: arbitrary bytes. Output: 32-byte digest.
//! Gas: 50 + 12 per 32-byte word (`HostCallWeight::Sha256`).

use sha2::{Digest, Sha256};

use super::{hash_gas, PrecompileError, PrecompileOutput};
use crate::gas::HostCallWeight;

pub fn run(data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    let gas_used = hash_gas(HostCallWeight::Sha256 {
        bytes: data.len() as u64,
    });
    let digest = Sha256::digest(data);
    Ok(PrecompileOutput {
        output: digest.to_vec(),
        gas_used,
    })
}

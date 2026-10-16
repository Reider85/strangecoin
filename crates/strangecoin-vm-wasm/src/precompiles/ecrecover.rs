//! `0x01 ecrecover` — secp256k1 public-key recovery (ADR-0015).
//!
//! Input (97 bytes): `hash (32) ‖ v (1) ‖ r (32) ‖ s (32)`.
//! `v ∈ {27, 28}` (Ethereum convention: 27 + recovery id).
//! Output: 33-byte compressed public key, or empty on recovery failure.
//!
//! Structural errors (length ≠ 97, `v` out of range) are
//! [`PrecompileError::InvalidInput`] (full gas burn). A well-formed input
//! whose signature does not recover — bad point, high-s, wrong parity —
//! returns empty output with success status and the declared gas, matching
//! Ethereum's "empty on failure" convention for this precompile.

use secp256k1::ecdsa::{RecoverableSignature, RecoveryId};
use secp256k1::{Message, Secp256k1};

use super::{PrecompileError, PrecompileOutput};
use crate::gas::HostCallWeight;

/// Exact input length: hash ‖ v ‖ r ‖ s.
pub const INPUT_LEN: usize = 32 + 1 + 32 + 32;

pub fn run(data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    let gas_used = HostCallWeight::Secp256k1Verify.gas();
    if data.len() != INPUT_LEN {
        return Err(PrecompileError::invalid_input(format!(
            "ecrecover expects {INPUT_LEN} bytes, got {}",
            data.len()
        )));
    }

    let v = data[32];
    let rec_id = match v {
        27 | 28 => v - 27,
        _ => {
            return Err(PrecompileError::invalid_input(format!(
                "ecrecover recovery id byte must be 27 or 28, got {v}"
            )))
        }
    };

    let Ok(msg) = Message::from_digest_slice(&data[0..32]) else {
        return Ok(empty(gas_used));
    };
    let Ok(rec_id) = RecoveryId::from_i32(i32::from(rec_id)) else {
        return Ok(empty(gas_used));
    };
    let mut compact = [0u8; 64];
    compact.copy_from_slice(&data[33..97]);
    let Ok(sig) = RecoverableSignature::from_compact(&compact, rec_id) else {
        return Ok(empty(gas_used));
    };

    let secp = Secp256k1::verification_only();
    match secp.recover_ecdsa(&msg, &sig) {
        Ok(pubkey) => Ok(PrecompileOutput {
            output: pubkey.serialize().to_vec(),
            gas_used,
        }),
        Err(_) => Ok(empty(gas_used)),
    }
}

fn empty(gas_used: u64) -> PrecompileOutput {
    PrecompileOutput {
        output: Vec::new(),
        gas_used,
    }
}

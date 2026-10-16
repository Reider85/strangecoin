//! `0x05 ed25519_verify` — legacy Ed25519 signature verification
//! (ADR-0015; EOAs use secp256k1 per ADR-0001 — this precompile exists for
//! cross-chain verification use-cases only).
//!
//! Input: `pubkey (32) ‖ signature (64) ‖ msg (…)` (at least 96 bytes).
//! Output: 32-byte `1` on success, 32-byte `0` on failure.
//! Gas: 2000.
//!
//! Short input is [`PrecompileError::InvalidInput`] (full gas burn). An
//! unparseable public key or a verification mismatch is *not* an error —
//! it yields the 32-byte zero output with the declared gas, so callers can
//! branch on the result without the frame reverting.

use ed25519_dalek::{Signature, VerifyingKey};

use super::{PrecompileError, PrecompileOutput, GAS_ED25519_VERIFY};

/// Minimum length: pubkey ‖ signature (message may be empty).
pub const MIN_INPUT_LEN: usize = 32 + 64;

pub fn run(data: &[u8]) -> Result<PrecompileOutput, PrecompileError> {
    let gas_used = GAS_ED25519_VERIFY;
    if data.len() < MIN_INPUT_LEN {
        return Err(PrecompileError::invalid_input(format!(
            "ed25519_verify expects at least {MIN_INPUT_LEN} bytes, got {}",
            data.len()
        )));
    }

    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(&data[0..32]);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(&data[32..96]);
    let msg = &data[96..];

    let verified = match VerifyingKey::from_bytes(&pk_bytes) {
        Ok(pk) => {
            let sig = Signature::from_bytes(&sig_bytes);
            pk.verify_strict(msg, &sig).is_ok()
        }
        Err(_) => false,
    };

    let mut output = vec![0u8; 32];
    if verified {
        output[31] = 1;
    }
    Ok(PrecompileOutput {
        output,
        gas_used,
    })
}

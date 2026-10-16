//! `secp256k1_verify(pk_ptr, hash_ptr, sig_ptr) -> i32`.
//!
//! Verifies a compact ECDSA signature over a 32-byte message hash.
//! Inputs: 33-byte compressed public key, 32-byte hash, 64-byte `r‖s`
//! compact signature. Returns `1` when the signature is valid, `0` otherwise.
//!
//! A malformed key or signature is a normal `0` (verification failed), not a
//! trap — callers distinguish "invalid crypto material" from "bad memory
//! access" (the latter aborts the frame). Gas: 3000 (ADR-0014), charged
//! before any parsing.

use secp256k1::ecdsa::Signature;
use secp256k1::{Message, PublicKey, Secp256k1};
use wasmi::{Caller, Linker};

use crate::gas::HostCallWeight;
use crate::host::{abi, charge, checked_u32, HostState};

pub fn register(linker: &mut Linker<HostState>) -> Result<(), wasmi::Error> {
    linker
        .func_wrap("sc", "secp256k1_verify", secp256k1_verify)
        .map(|_| ())
        .map_err(Into::into)
}

fn secp256k1_verify(
    mut caller: Caller<'_, HostState>,
    pk_ptr: i32,
    hash_ptr: i32,
    sig_ptr: i32,
) -> Result<i32, wasmi::Error> {
    charge(&mut caller, HostCallWeight::Secp256k1Verify)?;
    let pk_ptr = checked_u32(pk_ptr, "pk_ptr")?;
    let hash_ptr = checked_u32(hash_ptr, "hash_ptr")?;
    let sig_ptr = checked_u32(sig_ptr, "sig_ptr")?;

    let pk_bytes = abi::read_bytes(&caller, pk_ptr, 33)?;
    let hash = abi::read_array32(&caller, hash_ptr)?;
    let sig_bytes = abi::read_bytes(&caller, sig_ptr, 64)?;

    let secp = Secp256k1::verification_only();
    let Ok(pk) = PublicKey::from_slice(&pk_bytes) else {
        return Ok(0);
    };
    let Ok(sig) = Signature::from_compact(&sig_bytes) else {
        return Ok(0);
    };
    let msg = Message::from_digest(hash);
    Ok(i32::from(secp.verify_ecdsa(&msg, &sig, &pk).is_ok()))
}

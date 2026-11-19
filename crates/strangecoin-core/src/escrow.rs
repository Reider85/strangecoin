//! EscrowVault calldata codec — AI-P03, ADR-0030 (`docs/spec/acp_messages.md`
//! §8).
//!
//! One fixed-width wire layout shared by three consumers that all depend on
//! `strangecoin-core` and nothing else: the `AcpHandler` (escrow
//! verification, invariant #24), the `EscrowVault` WASM contract
//! (`strangecoin-vm-wasm/examples/escrow_vault.rs` — WAT parses these exact
//! offsets) and the tests that build deposit/release transactions.
//!
//! **Why a fixed layout instead of the JSON-ABI head/tail codec**: the
//! monolith never persists `AbiDocument`s (`WasmiExecutor::insert_abi` has
//! no production call site — the ADR-0029 amendment record), so selector
//! dispatch always falls back to the `main` export. The contract therefore
//! self-dispatches inside `main` on [`deposit_selector`] /
//! [`release_selector`] / [`refund_selector`] — computed with the regular
//! `method_selector` blake3 prefix, so a future ABI-doc registry stays
//! compatible — and reads a flat layout a WAT function can parse with plain
//! `i32.load`s.
//!
//! ```text
//! deposit(calldata):
//!   [0..4)     selector deposit
//!   [4..8)     u32 LE len_a
//!   [8..8+a)   agent_a bech32 address (utf8)
//!   next 4B    u32 LE len_b
//!   next  b    agent_b bech32 address (utf8)
//!   next 33B   agent_a compressed pubkey
//!   next 33B   agent_b compressed pubkey
//!   next 8B    u64 LE amount (SC)
//!   next 8B    u64 LE deadline_block
//!
//! release(calldata):
//!   [0..4)     selector release | [4..12) escrow_id_bytes | [12..76) sig_a
//!   [76..140)  sig_b
//!
//! refund(calldata): selector | id | sig_a  (76 B, timeout mode)
//!                    + sig_b             (140 B, mutual mode)
//!
//! EscrowVault storage record (245 B under key = id_bytes || 0*24):
//!   [0..8) amount | [8..16) deadline | [16] status | [17] len_a | [18] len_b
//!   [19..99) addr_a | [99..179) addr_b | [179..212) pk_a | [212..245) pk_b
//! ```
//!
//! **Escrow id**: `blake3(deposit_calldata)[0..8]` — both the handler (from
//! `tx.data`) and the contract (from its own input, identical bytes)
//! recompute it without any extra round-trip; the signature message is
//! `blake3(id_bytes)` (32 B). A repeated *identical* deposit calldata maps
//! to an occupied storage key and the contract reverts (documented ADR
//! amendment — the ADR's monotonic-counter `escrow_id` cannot be derived by
//! the handler from `escrow_txid` alone).
//!
//! Determinism: pure byte functions — no time, no RNG, identical output for
//! identical input (Stage 3 storage determinism checklist pattern).

use crate::abi::{method_selector, AbiParam, AbiType};
use std::fmt;

/// Max stored/parsed bech32 address length (mainnet `sc1…` ≈ 63 chars for a
/// 33-byte pubkey payload; 80 leaves headroom for every network HRP).
pub const MAX_ADDRESS_LEN: usize = 80;

/// Fixed deposit fields after the two length-prefixed addresses
/// (`33 + 33 + 8 + 8`).
const DEPOSIT_FIXED_TAIL: usize = 33 + 33 + 8 + 8;

/// EscrowVault storage record size (see the module table).
pub const ESCROW_RECORD_LEN: usize = 245;

/// `deposit(agent_a: address, agent_b: address, amount: uint64,
/// deadline_block: uint64)` — spec §8.1.
pub fn deposit_selector() -> [u8; 4] {
    method_selector(
        "deposit",
        &[
            AbiParam::new("agent_a", AbiType::Address),
            AbiParam::new("agent_b", AbiType::Address),
            AbiParam::new("amount", AbiType::Uint(64)),
            AbiParam::new("deadline_block", AbiType::Uint(64)),
        ],
    )
}

/// `release(escrow_id: uint64, signature_a: bytes, signature_b: bytes)` —
/// spec §8.2.
pub fn release_selector() -> [u8; 4] {
    method_selector(
        "release",
        &[
            AbiParam::new("escrow_id", AbiType::Uint(64)),
            AbiParam::new("signature_a", AbiType::Bytes),
            AbiParam::new("signature_b", AbiType::Bytes),
        ],
    )
}

/// `refund(escrow_id: uint64, signature_a: bytes, signature_b: bytes)` —
/// spec §8.3 (single selector; the contract picks timeout vs mutual mode by
/// calldata length, 76 vs 140 bytes).
pub fn refund_selector() -> [u8; 4] {
    method_selector(
        "refund",
        &[
            AbiParam::new("escrow_id", AbiType::Uint(64)),
            AbiParam::new("signature_a", AbiType::Bytes),
            AbiParam::new("signature_b", AbiType::Bytes),
        ],
    )
}

/// Codec failure (truncated frames, bad selector, oversize address…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscrowCodecError {
    /// Calldata shorter than the fixed header / a length prefix runs past
    /// the end.
    Truncated,
    /// First 4 bytes are not a known EscrowVault selector.
    BadSelector,
    /// A length-prefixed address exceeds [`MAX_ADDRESS_LEN`] (or a zero
    /// length where ≥ 1 is required).
    BadAddressLen,
    /// Valid prefix parsed but bytes remain after the last field.
    TrailingBytes,
    /// Release/refund frame has a length that is neither 76 nor 140
    /// (refund) / 140 (release).
    BadLength,
}

impl fmt::Display for EscrowCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EscrowCodecError::Truncated => write!(f, "escrow calldata truncated"),
            EscrowCodecError::BadSelector => write!(f, "unknown escrow selector"),
            EscrowCodecError::BadAddressLen => write!(f, "bad escrow address length"),
            EscrowCodecError::TrailingBytes => write!(f, "trailing bytes after escrow fields"),
            EscrowCodecError::BadLength => write!(f, "bad escrow release/refund length"),
        }
    }
}

impl std::error::Error for EscrowCodecError {}

/// Decoded `deposit` arguments (spec §8.1 ABI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscrowDepositData {
    /// Assigner's bech32 address (escrow refunds here).
    pub agent_a: String,
    /// Performer's bech32 address (escrow releases here).
    pub agent_b: String,
    /// Assigner's compressed pubkey — binds `agent_a`'s settle/refund sigs.
    pub pk_a: [u8; 33],
    /// Performer's compressed pubkey — binds `agent_b`'s settle sig.
    pub pk_b: [u8; 33],
    /// Escrowed amount in SC (must equal the tx value — enforced by the
    /// handler and by the contract through `sc.value`).
    pub amount: u64,
    /// Deadline block height (unilateral revoke after this height).
    pub deadline_block: u64,
}

/// Input to [`encode_deposit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscrowDepositParams<'a> {
    /// Assigner address (bech32).
    pub agent_a: &'a str,
    /// Performer address (bech32).
    pub agent_b: &'a str,
    /// Assigner compressed pubkey (33 B).
    pub pk_a: [u8; 33],
    /// Performer compressed pubkey (33 B).
    pub pk_b: [u8; 33],
    /// Escrowed amount (SC).
    pub amount: u64,
    /// Deadline block height.
    pub deadline_block: u64,
}

fn push_str_field(out: &mut Vec<u8>, value: &str) -> Result<(), EscrowCodecError> {
    if value.is_empty() || value.len() > MAX_ADDRESS_LEN {
        return Err(EscrowCodecError::BadAddressLen);
    }
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

/// Build `EscrowVault.deposit` calldata (spec §8.1).
pub fn encode_deposit(params: &EscrowDepositParams<'_>) -> Result<Vec<u8>, EscrowCodecError> {
    let mut out = Vec::with_capacity(96 + params.agent_a.len() + params.agent_b.len());
    out.extend_from_slice(&deposit_selector());
    push_str_field(&mut out, params.agent_a)?;
    push_str_field(&mut out, params.agent_b)?;
    out.extend_from_slice(&params.pk_a);
    out.extend_from_slice(&params.pk_b);
    out.extend_from_slice(&params.amount.to_le_bytes());
    out.extend_from_slice(&params.deadline_block.to_le_bytes());
    Ok(out)
}

/// Advance `off` by `len` and hand back the slice — bounds-checked cursor
/// for the flat layout parsers.
fn take<'a>(calldata: &'a [u8], off: &mut usize, len: usize) -> Result<&'a [u8], EscrowCodecError> {
    let end = off.checked_add(len).ok_or(EscrowCodecError::Truncated)?;
    let slice = calldata.get(*off..end).ok_or(EscrowCodecError::Truncated)?;
    *off = end;
    Ok(slice)
}

/// Parse `EscrowVault.deposit` calldata — the handler's #24 `Type`/`Parties`
/// checks read every field.
pub fn decode_deposit(calldata: &[u8]) -> Result<EscrowDepositData, EscrowCodecError> {
    if calldata.len() < 4 || calldata[..4] != deposit_selector() {
        return Err(EscrowCodecError::BadSelector);
    }
    let mut off = 4usize;

    let len_a = u32::from_le_bytes(
        take(calldata, &mut off, 4)?
            .try_into()
            .map_err(|_| EscrowCodecError::Truncated)?,
    ) as usize;
    if len_a == 0 || len_a > MAX_ADDRESS_LEN {
        return Err(EscrowCodecError::BadAddressLen);
    }
    let agent_a = std::str::from_utf8(take(calldata, &mut off, len_a)?)
        .map_err(|_| EscrowCodecError::BadAddressLen)?
        .to_string();

    let len_b = u32::from_le_bytes(
        take(calldata, &mut off, 4)?
            .try_into()
            .map_err(|_| EscrowCodecError::Truncated)?,
    ) as usize;
    if len_b == 0 || len_b > MAX_ADDRESS_LEN {
        return Err(EscrowCodecError::BadAddressLen);
    }
    let agent_b = std::str::from_utf8(take(calldata, &mut off, len_b)?)
        .map_err(|_| EscrowCodecError::BadAddressLen)?
        .to_string();

    let pk_a: [u8; 33] = take(calldata, &mut off, 33)?
        .try_into()
        .map_err(|_| EscrowCodecError::Truncated)?;
    let pk_b: [u8; 33] = take(calldata, &mut off, 33)?
        .try_into()
        .map_err(|_| EscrowCodecError::Truncated)?;
    let amount = u64::from_le_bytes(
        take(calldata, &mut off, 8)?
            .try_into()
            .map_err(|_| EscrowCodecError::Truncated)?,
    );
    let deadline_block = u64::from_le_bytes(
        take(calldata, &mut off, 8)?
            .try_into()
            .map_err(|_| EscrowCodecError::Truncated)?,
    );
    if off != calldata.len() {
        return Err(EscrowCodecError::TrailingBytes);
    }
    debug_assert_eq!(
        calldata.len(),
        4 + (4 + len_a + 4 + len_b) + DEPOSIT_FIXED_TAIL,
        "layout table must stay in sync"
    );

    Ok(EscrowDepositData {
        agent_a,
        agent_b,
        pk_a,
        pk_b,
        amount,
        deadline_block,
    })
}

/// Escrow id bytes: `blake3(deposit_calldata)[0..8]` — the raw 8-byte slice
/// both sides derive without extra state (module doc).
pub fn escrow_id_bytes(deposit_calldata: &[u8]) -> [u8; 8] {
    let hash = blake3::hash(deposit_calldata);
    hash.as_bytes()[..8].try_into().expect("8 bytes")
}

/// Numeric `u64` view of [`escrow_id_bytes`] (little-endian) — logging/API
/// sugar; the canonical id on the wire and in signatures is the raw 8 bytes.
pub fn escrow_id(deposit_calldata: &[u8]) -> u64 {
    u64::from_le_bytes(escrow_id_bytes(deposit_calldata))
}

/// EscrowVault storage key for an id: `id_bytes || 0 * 24` (the storage
/// layer keys by 32 bytes — spec §6.2 uses a different key scheme for
/// nonces, this one is contract-local).
pub fn escrow_storage_key(id_bytes: &[u8; 8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..8].copy_from_slice(id_bytes);
    key
}

/// 32-byte ECDSA message both agents sign/verify for settle/refund:
/// `blake3(id_bytes)` (the host `sc.secp256k1_verify` takes the digest
/// directly).
pub fn escrow_signature_message(id_bytes: &[u8; 8]) -> [u8; 32] {
    *blake3::hash(id_bytes).as_bytes()
}

/// Build `EscrowVault.release` calldata (spec §8.2 — always mutual, both
/// signatures).
pub fn encode_release(
    id_bytes: &[u8; 8],
    signature_a: &[u8; 64],
    signature_b: &[u8; 64],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(140);
    out.extend_from_slice(&release_selector());
    out.extend_from_slice(id_bytes);
    out.extend_from_slice(signature_a);
    out.extend_from_slice(signature_b);
    out
}

/// Parsed `release` / `refund` frame (both share the `id ‖ sig_a ‖ [sig_b]`
/// tail; [`refund`] uses `signature_b: None` for the timeout mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscrowActionData {
    /// Raw escrow id bytes (storage key prefix + signature preimage source).
    pub id_bytes: [u8; 8],
    /// Agent A's signature over [`escrow_signature_message`].
    pub signature_a: [u8; 64],
    /// Agent B's signature — `None` on the refund-timeout form.
    pub signature_b: Option<[u8; 64]>,
}

fn decode_action(calldata: &[u8], selector: [u8; 4]) -> Result<EscrowActionData, EscrowCodecError> {
    if calldata.len() < 4 || calldata[..4] != selector {
        return Err(EscrowCodecError::BadSelector);
    }
    match calldata.len() {
        76 => {
            let id_bytes = calldata[4..12]
                .try_into()
                .map_err(|_| EscrowCodecError::Truncated)?;
            let signature_a = calldata[12..76]
                .try_into()
                .map_err(|_| EscrowCodecError::Truncated)?;
            Ok(EscrowActionData {
                id_bytes,
                signature_a,
                signature_b: None,
            })
        }
        140 => {
            let id_bytes = calldata[4..12]
                .try_into()
                .map_err(|_| EscrowCodecError::Truncated)?;
            let signature_a = calldata[12..76]
                .try_into()
                .map_err(|_| EscrowCodecError::Truncated)?;
            let signature_b = calldata[76..140]
                .try_into()
                .map_err(|_| EscrowCodecError::Truncated)?;
            Ok(EscrowActionData {
                id_bytes,
                signature_a,
                signature_b: Some(signature_b),
            })
        }
        _ => Err(EscrowCodecError::BadLength),
    }
}

/// Parse `EscrowVault.release` calldata (always the 140-byte mutual form).
pub fn decode_release(calldata: &[u8]) -> Result<EscrowActionData, EscrowCodecError> {
    let data = decode_action(calldata, release_selector())?;
    if data.signature_b.is_none() {
        return Err(EscrowCodecError::BadLength);
    }
    Ok(data)
}

/// Build `EscrowVault.refund` calldata: `signature_b: None` → timeout mode
/// (76 B, contract additionally requires height > deadline), `Some` →
/// mutual mode (140 B).
pub fn encode_refund(
    id_bytes: &[u8; 8],
    signature_a: &[u8; 64],
    signature_b: Option<&[u8; 64]>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(140);
    out.extend_from_slice(&refund_selector());
    out.extend_from_slice(id_bytes);
    out.extend_from_slice(signature_a);
    if let Some(sig_b) = signature_b {
        out.extend_from_slice(sig_b);
    }
    out
}

/// Parse `EscrowVault.refund` calldata (76 B timeout / 140 B mutual).
pub fn decode_refund(calldata: &[u8]) -> Result<EscrowActionData, EscrowCodecError> {
    decode_action(calldata, refund_selector())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params<'a>(a: &'a str, b: &'a str) -> EscrowDepositParams<'a> {
        EscrowDepositParams {
            agent_a: a,
            agent_b: b,
            pk_a: [0x02; 33],
            pk_b: [0x03; 33],
            amount: 50,
            deadline_block: 1000,
        }
    }

    #[test]
    fn deposit_roundtrip() {
        let p = params(
            "tsc1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
            "tsc1qz",
        );
        let data = decode_deposit(&encode_deposit(&p).expect("encode")).expect("decode");
        assert_eq!(data.agent_a, p.agent_a);
        assert_eq!(data.agent_b, p.agent_b);
        assert_eq!(data.pk_a, p.pk_a);
        assert_eq!(data.pk_b, p.pk_b);
        assert_eq!(data.amount, 50);
        assert_eq!(data.deadline_block, 1000);
    }

    #[test]
    fn deposit_rejects_malformed_frames() {
        let good = encode_deposit(&params("tsc1qa", "tsc1qb")).expect("encode");
        // Wrong selector.
        let mut bad = good.clone();
        bad[0] ^= 0xFF;
        assert_eq!(decode_deposit(&bad), Err(EscrowCodecError::BadSelector));
        // Truncated tail.
        assert_eq!(
            decode_deposit(&good[..good.len() - 1]),
            Err(EscrowCodecError::Truncated)
        );
        // Trailing junk.
        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(
            decode_deposit(&trailing),
            Err(EscrowCodecError::TrailingBytes)
        );
        // Oversize address length prefix.
        let mut huge = vec![];
        huge.extend_from_slice(&deposit_selector());
        huge.extend_from_slice(&((MAX_ADDRESS_LEN as u32) + 1).to_le_bytes());
        assert_eq!(decode_deposit(&huge), Err(EscrowCodecError::BadAddressLen));
        // Empty frame.
        assert_eq!(decode_deposit(&[]), Err(EscrowCodecError::BadSelector));
    }

    #[test]
    fn deposit_encode_rejects_oversize_address() {
        let long = "a".repeat(MAX_ADDRESS_LEN + 1);
        assert_eq!(
            encode_deposit(&params(&long, "tsc1qb")),
            Err(EscrowCodecError::BadAddressLen)
        );
        assert_eq!(
            encode_deposit(&params("", "tsc1qb")),
            Err(EscrowCodecError::BadAddressLen)
        );
    }

    #[test]
    fn escrow_id_is_deterministic_and_address_sensitive() {
        let a = encode_deposit(&params("tsc1qa", "tsc1qb")).unwrap();
        let mut b_params = params("tsc1qa", "tsc1qb");
        b_params.amount = 51;
        let b = encode_deposit(&b_params).unwrap();
        assert_eq!(escrow_id_bytes(&a), escrow_id_bytes(&a), "stable");
        assert_ne!(escrow_id_bytes(&a), escrow_id_bytes(&b), "content-bound");
        let id = escrow_id(&a);
        assert_eq!(id.to_le_bytes(), escrow_id_bytes(&a), "u64 view = LE");
        let key = escrow_storage_key(&escrow_id_bytes(&a));
        assert_eq!(&key[..8], &escrow_id_bytes(&a));
        assert_eq!(&key[8..], &[0u8; 24]);
        // Signature message is the blake3 of the id bytes, not the raw id.
        assert_eq!(
            escrow_signature_message(&escrow_id_bytes(&a)),
            *blake3::hash(&escrow_id_bytes(&a)).as_bytes()
        );
        assert_ne!(
            escrow_signature_message(&escrow_id_bytes(&a)),
            [0u8; 32],
            "digest of real input is never zero"
        );
    }

    #[test]
    fn release_and_refund_roundtrip() {
        let deposit = encode_deposit(&params("tsc1qa", "tsc1qb")).unwrap();
        let id = escrow_id_bytes(&deposit);
        let sig_a = [0xAA; 64];
        let sig_b = [0xBB; 64];

        let release = encode_release(&id, &sig_a, &sig_b);
        let parsed = decode_release(&release).expect("release parses");
        assert_eq!(parsed.id_bytes, id);
        assert_eq!(parsed.signature_a, sig_a);
        assert_eq!(parsed.signature_b, Some(sig_b));

        // Refund timeout form (76 B).
        let timeout = encode_refund(&id, &sig_a, None);
        assert_eq!(timeout.len(), 76);
        let parsed = decode_refund(&timeout).expect("refund parses");
        assert_eq!(parsed.signature_b, None);
        // Refund mutual form.
        let mutual = encode_refund(&id, &sig_a, Some(&sig_b));
        assert_eq!(mutual.len(), 140);
        assert_eq!(
            decode_refund(&mutual).expect("mutual parses").signature_b,
            Some(sig_b)
        );

        // Selector mismatch across actions: a refund frame is not a release.
        assert_eq!(decode_refund(&release), Err(EscrowCodecError::BadSelector));
        assert_eq!(
            decode_release(&timeout),
            Err(EscrowCodecError::BadSelector),
            "refund frame is not a release frame"
        );
        // Release selector with the timeout (76-byte) length — the mutual
        // form is required, the signature_b slot is missing.
        let mut short_release = timeout.clone();
        short_release[..4].copy_from_slice(&release_selector());
        assert_eq!(
            decode_release(&short_release),
            Err(EscrowCodecError::BadLength),
            "release must be the mutual form"
        );
        // Odd length.
        let mut odd = timeout.clone();
        odd.push(0);
        assert_eq!(decode_refund(&odd), Err(EscrowCodecError::BadLength));
    }

    #[test]
    fn selectors_are_stable_golden_bytes() {
        // Pin the dispatch constants: a drift here silently breaks every
        // deployed EscrowVault against a new node build (wire-format break).
        assert_ne!(deposit_selector(), release_selector());
        assert_ne!(release_selector(), refund_selector());
        assert_eq!(
            deposit_selector(),
            method_selector(
                "deposit",
                &[
                    AbiParam::new("agent_a", AbiType::Address),
                    AbiParam::new("agent_b", AbiType::Address),
                    AbiParam::new("amount", AbiType::Uint(64)),
                    AbiParam::new("deadline_block", AbiType::Uint(64)),
                ],
            ),
            "deposit selector must stay derived from the canonical signature"
        );
    }
}

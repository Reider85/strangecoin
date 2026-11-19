//! ACP (Agent Communication Protocol) application layer — AI-P03,
//! ADR-0030.
//!
//! Everything above the wire codec ([`crate::protocol::acp`]): the
//! [`AcpHandler`] that validates incoming `ACP_*` frames (envelope
//! signature → replay nonce → escrow txid #24 → signatures → settlement),
//! the [`AcpNonceStore`] persistence seam (#25) and the
//! [`EscrowSettlement`] adapter that turns an accepted `ACP_SETTLE` /
//! `ACP_REVOKE` into the on-chain `EscrowVault.release` / `.refund` call.
//!
//! **Transport**: ACP rides the existing Noise-authenticated peer connection
//! — the connection loop dispatches tags `0x0E..=0x11` to
//! [`AcpHandler::handle`] after the Stage 2 token-bucket charge (spec §7,
//! V-44). No new handshake, no new crypto: envelopes are secp256k1-signed
//! application messages *inside* the encrypted channel.
//!
//! **Settlement wiring (deviation recorded in ADR-0030 amendments)**: the
//! handler validates fully but hands the release/refund to an injected
//! [`EscrowSettlement`]. Production wires [`NoopEscrowVault`] until AI-P04
//! provides session-scoped agent keys (`agent_signAndSubmitBatch`) — using
//! the operator wallet key would violate invariant #26 (agent/Operator key
//! isolation). Tests inject a chain-backed adapter that submits the real
//! `EscrowVault` call transaction.

pub mod handler;
pub mod nonce;

pub use handler::{AcpAccepted, AcpHandler, AcpHandlerConfig, AcpReject};
pub use nonce::{AcpNonceStore, MemoryNonceStore};

/// Outcome of an accepted SETTLE/REVOKE: what the settlement adapter must
/// execute on-chain (the handler has already verified everything).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscrowAction {
    /// Release the escrowed amount to agent B (spec §8.2, mutual sigs).
    Release {
        /// Numeric escrow id (logging; the canonical id is the raw 8 bytes).
        escrow_id: u64,
        /// Raw `blake3(deposit_calldata)[0..8]` id bytes.
        id_bytes: [u8; 8],
        /// txid of the deposit this settles.
        escrow_txid: [u8; 32],
        /// Agent A's signature over `blake3(id_bytes)`.
        signature_a: [u8; 64],
        /// Agent B's signature over `blake3(id_bytes)`.
        signature_b: [u8; 64],
    },
    /// Refund the escrowed amount to agent A (spec §8.3): `None` signature
    /// pair = timeout mode (contract additionally checks height >
    /// deadline), `Some` = mutual cancel.
    Refund {
        /// Numeric escrow id.
        escrow_id: u64,
        /// Raw id bytes.
        id_bytes: [u8; 8],
        /// txid of the deposit this refunds.
        escrow_txid: [u8; 32],
        /// Agent A's signature over `blake3(id_bytes)`.
        signature_a: [u8; 64],
        /// Agent B's signature for the mutual form.
        signature_b: Option<[u8; 64]>,
    },
}

/// Executes an [`EscrowAction`] on-chain.
///
/// `Err` is a *local submission* failure (no signing key wired, tx rejected
/// by the mempool, …) — the handler surfaces it as [`AcpReject::Settlement`]
/// **without** peer-score damage: the peer's message was valid.
pub trait EscrowSettlement: Send + Sync {
    /// Submit `EscrowVault.release`.
    fn release(&self, action: &EscrowAction) -> Result<(), String>;
    /// Submit `EscrowVault.refund`.
    fn refund(&self, action: &EscrowAction) -> Result<(), String>;
}

/// Production adapter until AI-P04 (ADR-0030 amendment): accepts nothing,
/// every call errs with the pointer to the landing prompt. Valid SETTLE /
/// REVOKE messages still pass handler validation — they surface as
/// [`AcpReject::Settlement`] locally and the escrow waits for AI-P04
/// session signing (or the deadline + ACP_REVOKE, per the ADR «ACP message
/// loss → escrow lock» consequence).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopEscrowVault;

impl EscrowSettlement for NoopEscrowVault {
    fn release(&self, _action: &EscrowAction) -> Result<(), String> {
        Err(
            "EscrowVault.release not wired until AI-P04 (agent session signing \
             via agent_signAndSubmitBatch)"
                .to_string(),
        )
    }

    fn refund(&self, _action: &EscrowAction) -> Result<(), String> {
        Err(
            "EscrowVault.refund not wired until AI-P04 (agent session signing \
             via agent_signAndSubmitBatch)"
                .to_string(),
        )
    }
}

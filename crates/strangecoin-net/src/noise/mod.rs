//! Noise Protocol Framework transport (ADR-0020) — S2-P07..S2-P09.
//!
//! * S2-P07 (this module's [`handshake`]): `Noise_XX_25519_ChaChaPoly_BLAKE2s`
//!   handshake state machine — 3 round trips, mutual auth without PSK,
//!   ephemeral keys per session (forward secrecy).
//! * S2-P08 (this module's [`transport`]): ChaCha20-Poly1305 record-layer
//!   framing on top of the `snow::TransportState` the handshake produces —
//!   explicit nonces, anti-replay, rekey.
//! * S2-P09: Ed25519 `node_id` binding for the peer identity.

pub mod handshake;
pub mod transport;

pub use handshake::{NoiseHandshake, NoiseRole, NOISE_PARAMS};
pub use transport::NoiseTransport;

use thiserror::Error;

/// Errors surfaced by the Noise handshake / transport layer.
///
/// Mirrors the relevant `snow::Error` cases (the `snow` enum is
/// `#[non_exhaustive]`, so unknown variants collapse into [`NoiseError::Init`])
/// plus the application-level checks this crate adds on top of Noise:
/// [`NoiseError::Auth`] (expected remote static key mismatch) and the
/// [`NoiseError::Input`] buffer guard (a too-short caller buffer must be
/// rejected before `snow`, which indexes into it unconditionally).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NoiseError {
    /// Builder failure: bad pattern string, missing prerequisite, bad key
    /// material, or an unmapped `snow` variant.
    #[error("noise init failed: {0}")]
    Init(String),

    /// Illegal state transition (e.g. `into_transport()` before the
    /// handshake finished).
    #[error("noise state problem: {0}")]
    State(String),

    /// Invalid input: undersized output buffer or oversized handshake
    /// message.
    #[error("noise invalid input: {0}")]
    Input(String),

    /// AEAD tag verification failed — corrupted or tampered message.
    #[error("noise decryption failed")]
    Decrypt,

    /// Diffie-Hellman agreement failed.
    #[error("noise diffie-hellman failed")]
    Dh,

    /// Random source failed.
    #[error("noise rng failed")]
    Rng,

    /// The peer's Noise static public key does not match the expected one
    /// (auth check at the `es` / `se` step, ADR-0020 MITM detection).
    #[error("remote static key mismatch")]
    Auth,

    /// Frame nonce did not equal the expected monotonic counter — replayed,
    /// reordered or corrupted nonce header (S2-P08 transport, threat V-14).
    #[error("nonce out of sequence")]
    Replay,
}

impl From<snow::Error> for NoiseError {
    fn from(err: snow::Error) -> Self {
        use snow::Error as Snow;
        match err {
            Snow::Decrypt => NoiseError::Decrypt,
            Snow::Dh => NoiseError::Dh,
            Snow::Rng => NoiseError::Rng,
            Snow::State(s) => NoiseError::State(format!("{s:?}")),
            Snow::Input => NoiseError::Input("invalid input".to_string()),
            // Init / Prereq / Pattern and any variant snow adds later.
            other => NoiseError::Init(other.to_string()),
        }
    }
}

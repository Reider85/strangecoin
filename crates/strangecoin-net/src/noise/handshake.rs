//! Noise **XX** handshake state machine (ADR-0020, S2-P07).
//!
//! The XX pattern gives mutual authentication without pre-shared keys in
//! exactly three round trips:
//!
//! ```text
//! Initiator                                        Responder
//! e            ─────────────────────────────────▶
//!              ◀─────────────────────────────────  e, ee, s, es
//! s, se        ─────────────────────────────────▶
//!              ── transport ready (both sides) ──
//! ```
//!
//! Static keys travel inside the handshake (encrypted), ephemeral keys are
//! generated per session — compromising a long-term key decrypts no recorded
//! session (forward secrecy).
//!
//! **Determinism (Stage 2 checklist, threat V-21 extension):** ephemeral keys
//! currently come from `snow`'s default resolver, i.e. the OS CSPRNG — the
//! constructor signature is fixed by the S2-P07 spec (`initiator(static_key)`).
//! Deriving them from `keystore.long_term_seed` via HKDF
//! (`info = b"noise_ephemeral_v1"`, ADR-0020) is an open obligation, tracked
//! in AGENTS.md §Stage 2 Determinism Checklist and planned for S2-P09 where
//! the keystore integration lands.
//!
//! **Threading:** `snow::HandshakeState` -> `snow::TransportState` must happen
//! on a single thread (Stage 2 Pitfalls) — `into_transport` consumes `self`,
//! so ownership transfer is explicit by construction.

use super::NoiseError;

/// Noise handshake parameters used by Strangecoin (ADR-0020).
pub const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Largest number of bytes the XX pattern can add around a handshake
/// payload, from the longest message (round 2):
/// `e`(32) + `s`(32) + `s` AEAD tag(16) + payload AEAD tag(16) = 96.
const MAX_XX_OVERHEAD: usize = 96;

/// Which side of the handshake this node is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseRole {
    /// Sends the first message (`e`).
    Initiator,
    /// Waits for the first message, replies with `e, ee, s, es`.
    Responder,
}

/// An in-progress Noise XX handshake.
///
/// Drive it by alternating `write_message` / `read_message` across the
/// transport until [`is_finished`](Self::is_finished) is `true` on both
/// sides, then consume it with [`into_transport`](Self::into_transport).
pub struct NoiseHandshake {
    state: snow::HandshakeState,
    role: NoiseRole,
    expected_remote_static: Option<[u8; 32]>,
}

impl NoiseHandshake {
    /// Build the side that sends the first message.
    ///
    /// `static_key` is this node's long-term X25519 private key (noise DH
    /// identity — distinct from the Ed25519 `node_id` added in S2-P09 and
    /// from the secp256k1 wallet keys).
    pub fn initiator(static_key: &[u8; 32]) -> Result<Self, NoiseError> {
        let state = Self::builder(static_key)?
            .build_initiator()
            .map_err(NoiseError::from)?;
        Ok(Self {
            state,
            role: NoiseRole::Initiator,
            expected_remote_static: None,
        })
    }

    /// Build the side that waits for the first message.
    pub fn responder(static_key: &[u8; 32]) -> Result<Self, NoiseError> {
        let state = Self::builder(static_key)?
            .build_responder()
            .map_err(NoiseError::from)?;
        Ok(Self {
            state,
            role: NoiseRole::Responder,
            expected_remote_static: None,
        })
    }

    fn builder(static_key: &[u8; 32]) -> Result<snow::Builder<'_>, NoiseError> {
        // `NoiseParams: FromStr<Err = snow::Error>`.
        let params = NOISE_PARAMS.parse::<snow::params::NoiseParams>()?;
        snow::Builder::new(params)
            .local_private_key(static_key)
            .map_err(NoiseError::from)
    }

    /// Require the peer's Noise static public key to equal `key`.
    ///
    /// XX only *transports* the remote static key — it never proves the key
    /// belongs to a particular peer, so binding it to an expected value is an
    /// application-level check (ADR-0020 «MITM detection»; the Ed25519
    /// `node_id` binding arrives in S2-P09). On the responder side the remote
    /// static key becomes known when reading round 3, i.e. at the `se` step;
    /// on the initiator side at `es` (round 2). Unset by default: peers
    /// learned through ADDR gossip are unknown until first contact.
    pub fn expect_remote_static(mut self, key: [u8; 32]) -> Self {
        self.expected_remote_static = Some(key);
        self
    }

    /// Produce the next handshake message into `out`, returning its length.
    ///
    /// Fails with [`NoiseError::Input`] when `out` cannot hold the message —
    /// `snow` writes into the buffer without bounds checks, so the guard runs
    /// first (invariant #16 discipline: reject before trusting the buffer).
    pub fn write_message(&mut self, payload: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        let need = payload.len() + MAX_XX_OVERHEAD;
        if out.len() < need {
            return Err(NoiseError::Input(format!(
                "handshake buffer too small: need {need}, got {}",
                out.len()
            )));
        }
        self.state
            .write_message(payload, out)
            .map_err(NoiseError::from)
    }

    /// Consume the peer's handshake message, writing its payload to `out`.
    ///
    /// Once the remote static key has been learned, it is checked against
    /// [`expect_remote_static`](Self::expect_remote_static) if one was set.
    pub fn read_message(&mut self, message: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        if out.len() < message.len() {
            return Err(NoiseError::Input(format!(
                "handshake buffer too small: need {}, got {}",
                message.len(),
                out.len()
            )));
        }
        let written = self
            .state
            .read_message(message, out)
            .map_err(NoiseError::from)?;
        self.verify_remote_static()?;
        Ok(written)
    }

    /// `true` once all three round trips completed and
    /// [`into_transport`](Self::into_transport) may be called.
    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Consume the finished handshake, returning the symmetric transport
    /// state used for every subsequent frame (S2-P08 wraps it into
    /// `NoiseTransport`).
    pub fn into_transport(self) -> Result<snow::TransportState, NoiseError> {
        self.verify_remote_static()?;
        self.state.into_transport_mode().map_err(NoiseError::from)
    }

    /// This node's role in the handshake.
    pub fn role(&self) -> NoiseRole {
        self.role
    }

    /// The running handshake hash — identical on both peers once the
    /// handshake succeeds, and the transcript the transport keys are derived
    /// from. Empty payloads only at S2-P07; S2-P09 signs over this value.
    pub fn handshake_hash(&self) -> &[u8] {
        self.state.get_handshake_hash()
    }

    /// The peer's Noise static public key, once the pattern has revealed it
    /// (`None` on earlier rounds).
    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.state.get_remote_static()?.get(..32)?.try_into().ok()
    }

    fn verify_remote_static(&self) -> Result<(), NoiseError> {
        let Some(expected) = self.expected_remote_static else {
            return Ok(());
        };
        let Some(remote) = self.remote_static() else {
            // Not learned yet — nothing to compare (earlier XX round).
            return Ok(());
        };
        if remote == expected {
            Ok(())
        } else {
            Err(NoiseError::Auth)
        }
    }
}

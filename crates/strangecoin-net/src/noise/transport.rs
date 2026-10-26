//! S2-P08 — Noise **transport** record layer (ADR-0020).
//!
//! After the XX handshake both peers hold a `snow::TransportState`; this
//! module wraps it with the per-frame discipline Strangecoin needs:
//!
//! * an explicit `[nonce: u64 BE]` header in front of every AEAD message, so
//!   a replay / reorder surfaces as [`NoiseError::Replay`] *before* the AEAD
//!   tag is consulted (kept distinct from tampering → [`NoiseError::Decrypt`]);
//! * monotonic send/receive counters that keep advancing across
//!   [`rekey`](NoiseTransport::rekey) — snow rotates only the key, the Noise
//!   spec nonce is not reset (noise rev1 §11.3);
//! * buffer guards **before** touching `snow` (invariant #16 discipline:
//!   never index into a caller buffer we have not measured first).
//!
//! **Determinism** (Stage 2 checklist, threat V-21 extension): no clock, no
//! RNG, no I/O — counters start at 0 on every session and advance exactly one
//! step per frame; keys come from the handshake transcript.
//!
//! **Threading:** one `NoiseTransport` owns the `TransportState`
//! (`HandshakeState -> TransportState` stays on a single thread — Stage 2
//! Pitfalls); sharing across tasks needs an `Arc<Mutex<>>` once the
//! connection loop lands (S2-P12 / S2-P19).

use super::NoiseError;

/// Maximum size of one Noise transport message, per the Noise spec
/// (`MAXMSGLEN`): snow rejects anything larger with `Error::Input`.
pub const MAX_NOISE_MSG_LEN: usize = 65_535;

/// ChaCha20-Poly1305 authentication tag appended to every ciphertext.
pub const NOISE_TAG_LEN: usize = 16;

/// Explicit per-frame nonce header in front of the ciphertext.
pub const NOISE_NONCE_LEN: usize = 8;

/// Largest plaintext that fits into a single transport message.
pub const MAX_TRANSPORT_PAYLOAD: usize = MAX_NOISE_MSG_LEN - NOISE_TAG_LEN; // 65_519

/// Largest `[nonce][ciphertext + tag]` part — the unit
/// [`crate::protocol::decrypt_frame`] splits a chunked blob on.
pub const MAX_ENCRYPTED_PART: usize = NOISE_NONCE_LEN + MAX_NOISE_MSG_LEN; // 65_543

/// Post-handshake transport session: ChaCha20-Poly1305 framing on top of the
/// `snow::TransportState` produced by a finished [`super::NoiseHandshake`].
///
/// One struct owns both directions of one TCP session, matching snow's
/// `TransportState` (which keeps an egress and an ingress `CipherState`).
pub struct NoiseTransport {
    state: snow::TransportState,
    send_nonce: u64,
    recv_nonce: u64,
    expected_node_id: Option<[u8; 32]>,
}

impl NoiseTransport {
    /// Wrap the transport state a finished [`super::NoiseHandshake`] produced.
    ///
    /// `expected_node_id` prepares the MITM binding check of ADR-0020: the
    /// `node_id` of the peer this connection was supposed to reach. The value
    /// is stored and exposed now; verification lands in S2-P09 together with
    /// the Ed25519 auth message — at S2-P08 nothing in the transport can be
    /// compared against yet (Noise static keys are X25519, `node_id` is
    /// `blake3(Ed25519 pubkey)`, different key material).
    pub fn new(state: snow::TransportState, expected_node_id: Option<[u8; 32]>) -> Self {
        Self {
            state,
            send_nonce: 0,
            recv_nonce: 0,
            expected_node_id,
        }
    }

    /// Encrypt one frame into `[nonce: u64 BE][ciphertext + AEAD tag]`,
    /// returning the number of bytes written.
    ///
    /// The nonce is written from [`send_nonce`](Self::send_nonce), which then
    /// advances one step — the header the peer validates against its own
    /// receive counter. Fails with [`NoiseError::Input`] when `out` cannot
    /// hold the frame or the plaintext exceeds
    /// [`MAX_TRANSPORT_PAYLOAD`]; both guards run **before** `snow` writes
    /// anything (invariant #16 discipline).
    pub fn encrypt(&mut self, plaintext: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        if plaintext.len() > MAX_TRANSPORT_PAYLOAD {
            return Err(NoiseError::Input(format!(
                "transport payload too large: {} > {MAX_TRANSPORT_PAYLOAD}",
                plaintext.len()
            )));
        }
        let need = NOISE_NONCE_LEN + plaintext.len() + NOISE_TAG_LEN;
        if out.len() < need {
            return Err(NoiseError::Input(format!(
                "transport buffer too small: need {need}, got {}",
                out.len()
            )));
        }
        out[..NOISE_NONCE_LEN].copy_from_slice(&self.send_nonce.to_be_bytes());
        let written = self
            .state
            .write_message(plaintext, &mut out[NOISE_NONCE_LEN..])?;
        self.send_nonce = self
            .send_nonce
            .checked_add(1)
            .ok_or_else(|| NoiseError::State("send nonce exhausted".to_string()))?;
        Ok(NOISE_NONCE_LEN + written)
    }

    /// Decrypt one frame written by [`encrypt`](Self::encrypt) on the peer,
    /// returning the plaintext length.
    ///
    /// The frame's nonce must equal [`recv_nonce`](Self::recv_nonce)
    /// **exactly**: TCP delivers in order, so the next frame is only ever the
    /// next counter value. Anything else — a replayed old frame, a reordered
    /// frame, a corrupted header — is rejected as [`NoiseError::Replay`]
    /// before the AEAD tag is consulted, keeping replay (threat V-14) and
    /// tampering distinguishable in logs. The counter advances only after a
    /// successful decrypt, so a rejected frame never desynchronizes the
    /// session.
    pub fn decrypt(&mut self, message: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        if message.len() < NOISE_NONCE_LEN + NOISE_TAG_LEN {
            return Err(NoiseError::Input(format!(
                "transport message too short: {}",
                message.len()
            )));
        }
        let mut nonce_bytes = [0u8; 8];
        nonce_bytes.copy_from_slice(&message[..NOISE_NONCE_LEN]);
        if u64::from_be_bytes(nonce_bytes) != self.recv_nonce {
            return Err(NoiseError::Replay);
        }
        let ciphertext = &message[NOISE_NONCE_LEN..];
        let need = ciphertext.len() - NOISE_TAG_LEN;
        if out.len() < need {
            return Err(NoiseError::Input(format!(
                "transport buffer too small: need {need}, got {}",
                out.len()
            )));
        }
        let written = self.state.read_message(ciphertext, out)?;
        self.recv_nonce = self
            .recv_nonce
            .checked_add(1)
            .ok_or_else(|| NoiseError::State("receive nonce exhausted".to_string()))?;
        Ok(written)
    }

    /// Rotate both traffic keys (noise rev1 §4.2 REKEY).
    ///
    /// Both directions rekey together because this struct owns the session's
    /// egress *and* ingress `CipherState`; nonces deliberately keep their
    /// monotonic run across the rotation (snow does not reset them either),
    /// so every frame still carries exactly the expected counter value.
    ///
    /// Synchronizing *when* to rekey is the application's job (noise rev1
    /// §11.3): both peers must rotate at the same message index. No connection
    /// loop exists yet at S2-P08 — the trigger (every N messages / M bytes)
    /// arrives with peer connections in S2-P12/S2-P19; this method is the
    /// primitive they will call.
    pub fn rekey(&mut self) {
        self.state.rekey_outgoing();
        self.state.rekey_incoming();
    }

    /// The nonce the next [`encrypt`](Self::encrypt) will stamp (0-based).
    pub fn send_nonce(&self) -> u64 {
        self.send_nonce
    }

    /// The nonce the next [`decrypt`](Self::decrypt) expects.
    pub fn recv_nonce(&self) -> u64 {
        self.recv_nonce
    }

    /// The `node_id` this session was supposed to reach, if one was pinned
    /// (S2-P09 performs the binding check).
    pub fn expected_node_id(&self) -> Option<[u8; 32]> {
        self.expected_node_id
    }
}

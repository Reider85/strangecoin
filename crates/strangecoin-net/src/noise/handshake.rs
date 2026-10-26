//! Noise **XX** handshake state machine (ADR-0020, S2-P07) with Ed25519
//! peer identity binding (S2-P09).
//!
//! The XX pattern gives mutual authentication without pre-shared keys in
//! exactly three round trips:
//!
//! ```text
//! Initiator                                        Responder
//! e            ─────────────────────────────────▶
//!              ◀─────────────────────────────────  e, ee, s, es + auth
//! s, se + auth  ────────────────────────────────▶
//!              ── transport ready (both sides) ──
//! ```
//!
//! Static keys travel inside the handshake (encrypted), ephemeral keys are
//! generated per session — compromising a long-term key decrypts no recorded
//! session (forward secrecy).
//!
//! **Peer identity (S2-P09):** on round 2 the responder and on round 3 the
//! initiator append an auth blob — `[ed25519 pubkey (32) || signature (64)]`,
//! signed over the handshake hash as it stood *before* the signing message.
//! The reader verifies the signature, derives `node_id = blake3(pubkey)` and
//! compares it against [`expected_node_id`](NoiseHandshake::initiator) when
//! one was pinned — an attacker completing the handshake under their own
//! Noise + Ed25519 keys is dropped right there, before any transport byte
//! flows (MITM detection, threat V-24). Without a pinned expectation (peers
//! learned via ADDR gossip) the signature still must verify; the learned
//! [`NodeId`] surfaces from
//! [`into_transport`](NoiseHandshake::into_transport) as *unverified* (the
//! transport keeps `expected_node_id == None`) until peer_store pins it.
//!
//! **Determinism (Stage 2 checklist, threat V-21 extension):** ephemeral keys
//! no longer come from the OS CSPRNG — they are HKDF-derived from the
//! identity's long-term seed via [`super::ephemeral`], one derivation per
//! handshake (checklist #2/#3, closed in S2-P09).
//!
//! **Threading:** `snow::HandshakeState` -> `snow::TransportState` must happen
//! on a single thread (Stage 2 Pitfalls) — `into_transport` consumes `self`,
//! so ownership transfer is explicit by construction.

use super::auth::{NodeId, NodeKeypair, AUTH_BLOB_LEN};
use super::ephemeral::SeedResolver;
use super::transport::NoiseTransport;
use super::NoiseError;
use crate::error::NetError;
use ed25519_dalek::{Signature, VerifyingKey};
use std::borrow::Cow;

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
    /// This node's identity — signs the auth blob on its signing round and
    /// derives the ephemeral key source (shared `Arc`: one counter sequence
    /// per identity).
    identity: NodeKeypair,
    /// The `node_id` this connection was supposed to reach, if known
    /// (from peer_store / hard-coded seed). `None` = unverified peer.
    expected_node_id: Option<NodeId>,
    /// The authenticated `node_id` of the peer, learned on its auth round.
    peer_node_id: Option<NodeId>,
    /// Handshake messages this side has written (auth round bookkeeping).
    writes: u8,
    /// Handshake messages this side has read (auth round bookkeeping).
    reads: u8,
}

impl NoiseHandshake {
    /// Build the side that sends the first message.
    ///
    /// `static_key` is this node's long-term X25519 private key (noise DH
    /// identity — distinct from the Ed25519 `node_id` identity and from the
    /// secp256k1 wallet keys). `identity_key` signs the round-3 auth blob;
    /// `expected_node_id` pins the peer we mean to reach (MITM check, ADR-0020)
    /// — `None` for peers first seen through ADDR gossip (accepted, but
    /// marked unverified).
    pub fn initiator(
        static_key: &[u8; 32],
        identity_key: &NodeKeypair,
        expected_node_id: Option<NodeId>,
    ) -> Result<Self, NoiseError> {
        Self::build(
            static_key,
            identity_key,
            expected_node_id,
            NoiseRole::Initiator,
        )
    }

    /// Build the side that waits for the first message; signs the round-2
    /// auth blob with `identity_key`.
    pub fn responder(
        static_key: &[u8; 32],
        identity_key: &NodeKeypair,
        expected_node_id: Option<NodeId>,
    ) -> Result<Self, NoiseError> {
        Self::build(
            static_key,
            identity_key,
            expected_node_id,
            NoiseRole::Responder,
        )
    }

    fn build(
        static_key: &[u8; 32],
        identity_key: &NodeKeypair,
        expected_node_id: Option<NodeId>,
        role: NoiseRole,
    ) -> Result<Self, NoiseError> {
        // `NoiseParams: FromStr<Err = snow::Error>`.
        let params = NOISE_PARAMS.parse::<snow::params::NoiseParams>()?;
        // Deterministic ephemeral key source (Stage 2 checklist #2/#3): the
        // resolver replaces snow's OS-CSPRNG RNG, everything else delegates
        // to the default resolver.
        let resolver = Box::new(SeedResolver::new(identity_key.next_ephemeral_seed()));
        let builder = snow::Builder::with_resolver(params, resolver)
            .local_private_key(static_key)
            .map_err(NoiseError::from)?;
        let state = match role {
            NoiseRole::Initiator => builder.build_initiator().map_err(NoiseError::from)?,
            NoiseRole::Responder => builder.build_responder().map_err(NoiseError::from)?,
        };
        Ok(Self {
            state,
            role,
            expected_remote_static: None,
            identity: identity_key.clone(),
            expected_node_id,
            peer_node_id: None,
            writes: 0,
            reads: 0,
        })
    }

    /// Require the peer's Noise static public key to equal `key`.
    ///
    /// XX only *transports* the remote static key — it never proves the key
    /// belongs to a particular peer, so binding it to an expected value is an
    /// application-level check (ADR-0020 «MITM detection»; orthogonal to the
    /// Ed25519 `node_id` binding, which proves *who* the peer is). On the
    /// responder side the remote static key becomes known when reading round
    /// 3, i.e. at the `se` step; on the initiator side at `es` (round 2).
    /// Unset by default: peers learned through ADDR gossip are unknown until
    /// first contact.
    pub fn expect_remote_static(mut self, key: [u8; 32]) -> Self {
        self.expected_remote_static = Some(key);
        self
    }

    /// Produce the next handshake message into `out`, returning its length.
    ///
    /// On this side's signing round the payload is extended with the auth
    /// blob `[ed25519 pubkey || signature]` over the handshake hash as it
    /// stands *before* this message (the payload feeds the hash, so a
    /// post-write signature would be circular).
    ///
    /// Fails with [`NoiseError::Input`] when `out` cannot hold the message —
    /// `snow` writes into the buffer without bounds checks, so the guard runs
    /// first (invariant #16 discipline: reject before trusting the buffer).
    pub fn write_message(&mut self, payload: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        let extra = if self.should_sign() { AUTH_BLOB_LEN } else { 0 };
        let need = payload.len() + extra + MAX_XX_OVERHEAD;
        if out.len() < need {
            return Err(NoiseError::Input(format!(
                "handshake buffer too small: need {need}, got {}",
                out.len()
            )));
        }

        let full: Cow<'_, [u8]> = if extra > 0 {
            let signature = self.identity.sign(self.state.get_handshake_hash());
            let mut blob = [0u8; AUTH_BLOB_LEN];
            blob[..32].copy_from_slice(&self.identity.public_key());
            blob[32..].copy_from_slice(&signature.to_bytes());
            let mut extended = Vec::with_capacity(payload.len() + AUTH_BLOB_LEN);
            extended.extend_from_slice(payload);
            extended.extend_from_slice(&blob);
            Cow::Owned(extended)
        } else {
            Cow::Borrowed(payload)
        };

        let written = self
            .state
            .write_message(&full, out)
            .map_err(NoiseError::from)?;
        self.writes += 1;
        Ok(written)
    }

    /// Consume the peer's handshake message, writing its payload to `out`.
    ///
    /// Once the remote static key has been learned, it is checked against
    /// [`expect_remote_static`](Self::expect_remote_static) if one was set.
    /// On this side's verification round the trailing
    /// [`AUTH_BLOB_LEN`] bytes of the payload are the peer's auth blob:
    /// signature verified against the handshake hash from *before* this
    /// message, `node_id` derived and compared with
    /// `expected_node_id` when pinned. What remains of the payload (always
    /// empty at S2-P09) is returned as the caller-visible payload length.
    pub fn read_message(&mut self, message: &[u8], out: &mut [u8]) -> Result<usize, NoiseError> {
        if out.len() < message.len() {
            return Err(NoiseError::Input(format!(
                "handshake buffer too small: need {}, got {}",
                message.len(),
                out.len()
            )));
        }
        let verify = self.should_verify();
        let hash_before = verify.then(|| self.state.get_handshake_hash().to_vec());
        let written = self
            .state
            .read_message(message, out)
            .map_err(NoiseError::from)?;
        self.verify_remote_static()?;

        let payload_len = match hash_before {
            Some(hash) => {
                let len = self.verify_peer_identity(&hash, &out[..written])?;
                self.reads += 1;
                len
            }
            None => {
                self.reads += 1;
                written
            }
        };
        Ok(payload_len)
    }

    /// `true` once all three round trips completed and
    /// [`into_transport`](Self::into_transport) may be called.
    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Consume the finished handshake, returning the encrypted transport for
    /// every subsequent frame (S2-P08) plus the peer's authenticated
    /// [`NodeId`].
    ///
    /// The transport carries this connection's `expected_node_id` — `Some`
    /// means the pin was checked against the returned `NodeId` during the
    /// handshake (verified), `None` means nobody pinned the peer (accepted
    /// as *unverified*, to be settled when peer_store records it). A pinned
    /// mismatch never gets this far: it already failed in `read_message`.
    ///
    /// Errors as [`NetError`] (S2-P09 spec): handshake not finished, static
    /// key mismatch, or a peer that never presented an auth blob.
    pub fn into_transport(self) -> Result<(NoiseTransport, NodeId), NetError> {
        if !self.state.is_handshake_finished() {
            return Err(NetError::from(NoiseError::State(
                "handshake is not finished".to_string(),
            )));
        }
        self.verify_remote_static().map_err(NetError::from)?;
        let node_id = self.peer_node_id.ok_or_else(|| {
            NetError::Handshake("peer node_id was never authenticated".to_string())
        })?;
        let state = self.state.into_transport_mode().map_err(NetError::from)?;
        let transport = NoiseTransport::new(state, self.expected_node_id.map(|id| id.0));
        Ok((transport, node_id))
    }

    /// This node's role in the handshake.
    pub fn role(&self) -> NoiseRole {
        self.role
    }

    /// The running handshake hash — identical on both peers once the
    /// handshake succeeds, and the transcript the transport keys are derived
    /// from. Also the value each side signs for its auth blob.
    pub fn handshake_hash(&self) -> &[u8] {
        self.state.get_handshake_hash()
    }

    /// The peer's Noise static public key, once the pattern has revealed it
    /// (`None` on earlier rounds).
    pub fn remote_static(&self) -> Option<[u8; 32]> {
        self.state.get_remote_static()?.get(..32)?.try_into().ok()
    }

    /// The authenticated `node_id` of the peer, once its auth round passed.
    pub fn peer_node_id(&self) -> Option<NodeId> {
        self.peer_node_id
    }

    /// Does this side sign on its next write? Initiator: round 3 (second
    /// write). Responder: round 2 (first write). Round 1 stays payload-free.
    fn should_sign(&self) -> bool {
        match self.role {
            NoiseRole::Initiator => self.writes == 1,
            NoiseRole::Responder => self.writes == 0,
        }
    }

    /// Does this side verify on its next read? Initiator: round 2 (first
    /// read, responder signed). Responder: round 3 (second read, initiator
    /// signed).
    fn should_verify(&self) -> bool {
        match self.role {
            NoiseRole::Initiator => self.reads == 0,
            NoiseRole::Responder => self.reads == 1,
        }
    }

    /// Validate the peer's auth blob against `handshake_hash` (the transcript
    /// *before* the message carrying it) and this connection's
    /// `expected_node_id` pin. Returns how many leading payload bytes are the
    /// peer's own (everything before the blob).
    fn verify_peer_identity(
        &mut self,
        handshake_hash: &[u8],
        payload: &[u8],
    ) -> Result<usize, NoiseError> {
        if payload.len() < AUTH_BLOB_LEN {
            // The peer skipped its signing round — protocol violation.
            return Err(NoiseError::Auth);
        }
        let (peer_payload, blob) = payload.split_at(payload.len() - AUTH_BLOB_LEN);

        let mut public = [0u8; 32];
        public.copy_from_slice(&blob[..32]);
        let signature = Signature::from_slice(&blob[32..]).map_err(|_| NoiseError::Auth)?;
        let verifying = VerifyingKey::from_bytes(&public).map_err(|_| NoiseError::Auth)?;
        verifying
            .verify_strict(handshake_hash, &signature)
            .map_err(|_| NoiseError::Auth)?;

        let node_id = NodeId::from_public_bytes(public);
        if let Some(expected) = self.expected_node_id {
            if expected != node_id {
                // Cryptographically valid peer — but not the peer we meant
                // to reach. ADR-0020 MITM detection (threat V-24): drop
                // before the transport layer ever exists.
                return Err(NoiseError::Auth);
            }
        }
        self.peer_node_id = Some(node_id);
        Ok(peer_payload.len())
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

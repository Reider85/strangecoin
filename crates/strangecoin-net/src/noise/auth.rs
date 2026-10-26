//! S2-P09 — Ed25519 node identity for internal node-to-node auth (ADR-0020).
//!
//! ARCHITECT3 §3.8 puts Ed25519 on the *internal* P2P layer only — transaction
//! signing stays secp256k1 (ADR-0001), and the two key kinds live in separate
//! keystore directories (`keystore/node/` vs `keystore/wallet_*.json`,
//! AGENTS.md Stage 2 Pitfalls).
//!
//! * [`NodeId`] — `blake3(ed25519 pubkey)`, the peer handle the rest of the
//!   network layer compares against (peer_store, hard-coded seeds, ADDR gossip).
//! * [`NodeKeypair`] — the node's long-term signing identity, loaded through
//!   the [`NodeKeystore`] view so the concrete wallet `Keystore` type stays on
//!   the orchestrator side of the `strangecoin → strangecoin-net` dependency.
//! * [`verify_node_id`] — signature check + `node_id` derivation + comparison
//!   in one call, the binding the handshake runs on every auth round.
//!
//! The handshake carries identity as a `[pubkey: 32][signature: 64]` blob
//! (Ed25519 cannot recover the public key from a signature alone), signed over
//! the Noise handshake hash as it stood **before** the signing message — the
//! payload itself feeds the hash, so signing the post-write value would be
//! circular.
//!
//! **Determinism (Stage 2 checklist #2/#3, threat V-21 extension):** no clock,
//! no OS RNG on any path here. The only entropy is the 32-byte long-term seed;
//! Noise ephemeral keys are derived from it through HKDF in
//! [`super::ephemeral`], advanced by a per-keypair counter (rebuilt from zero
//! on every node start, which is what makes `deterministic_replay` runs
//! reproducible).

use crate::error::NetError;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Length of the handshake auth blob: `ed25519 pubkey (32) || signature (64)`.
pub const AUTH_BLOB_LEN: usize = 96;

/// `node_id = blake3(ed25519 public key)` — the internal P2P identity of a
/// node (ARCHITECT3 §3.8). Deliberately *not* the key itself: the handle is
/// stable, short and safe to log/persist, while the signature still proves
/// possession of the underlying key on every handshake.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) [u8; 32]);

impl NodeId {
    /// Derive a `node_id` from raw Ed25519 public key bytes.
    pub fn from_public_bytes(public: [u8; 32]) -> Self {
        Self(*blake3::hash(&public).as_bytes())
    }

    /// The 32-byte digest backing this handle.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", hex::encode(self.0))
    }
}

/// Minimal view of the node keystore (`keystore/node/`) that the network
/// crate needs to materialize a [`NodeKeypair`].
///
/// The concrete wallet `Keystore` type lives in the orchestrator crate
/// (`strangecoin` depends on `strangecoin-net`, never the reverse), so the
/// keystore is reached through this trait; the orchestrator's
/// `wallet::load_or_create_node_keypair` implements the flow and hands the raw
/// seed over via the `[u8; 32]` impl below.
pub trait NodeKeystore {
    /// The long-term 32-byte seed the node identity derives from.
    fn long_term_seed(&self) -> Result<[u8; 32], NetError>;
}

impl NodeKeystore for [u8; 32] {
    fn long_term_seed(&self) -> Result<[u8; 32], NetError> {
        Ok(*self)
    }
}

struct NodeKeypairInner {
    seed: [u8; 32],
    signing: SigningKey,
    public: [u8; 32],
    /// Which ephemeral seed the next handshake draws (Stage 2 checklist #3):
    /// `HKDF(seed, info = "noise_ephemeral_v1" || counter)`. Shared through
    /// the `Arc` so cloned keypairs keep advancing one sequence — same seed,
    /// same order of handshakes → same ephemeral keys.
    handshake_counter: AtomicU64,
}

/// The node's long-term Ed25519 identity (internal P2P auth, never an EOA).
///
/// Cheap to clone: clones share one inner allocation, so the ephemeral
/// counter stays a single sequence per identity.
#[derive(Clone)]
pub struct NodeKeypair {
    inner: Arc<NodeKeypairInner>,
}

impl NodeKeypair {
    /// Build the identity directly from a 32-byte seed.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&seed);
        let public = signing.verifying_key().to_bytes();
        Self {
            inner: Arc::new(NodeKeypairInner {
                seed,
                signing,
                public,
                handshake_counter: AtomicU64::new(0),
            }),
        }
    }

    /// Load the identity from a node keystore (S2-P09 `from_keystore`).
    ///
    /// See [`NodeKeystore`] for why the parameter is the trait rather than
    /// the wallet `Keystore` struct.
    pub fn from_keystore(keystore: &impl NodeKeystore) -> Result<Self, NetError> {
        Ok(Self::from_seed(keystore.long_term_seed()?))
    }

    /// `node_id = blake3(pubkey)` for this identity.
    pub fn node_id(&self) -> NodeId {
        NodeId::from_public_bytes(self.inner.public)
    }

    /// The raw Ed25519 public key bytes (32).
    pub fn public_key(&self) -> [u8; 32] {
        self.inner.public
    }

    /// Deterministic RFC 8032 signature over `msg`.
    pub fn sign(&self, msg: &[u8]) -> ed25519_dalek::Signature {
        self.inner.signing.sign(msg)
    }

    /// The next per-handshake ephemeral seed (HKDF, Stage 2 checklist #3).
    ///
    /// Advances the shared counter — one seed per handshake, reproducible for
    /// a given seed + handshake order (AGENTS.md §Stage 2 Determinism
    /// Checklist, closed in S2-P09).
    pub(crate) fn next_ephemeral_seed(&self) -> [u8; 32] {
        let counter = self.inner.handshake_counter.fetch_add(1, Ordering::Relaxed);
        super::ephemeral::ephemeral_seed(&self.inner.seed, counter)
    }
}

/// Verify a handshake auth blob: signature over `msg` under `public`, then
/// `node_id = blake3(public)` compared against `expected_node_id`.
///
/// The comparison is what turns a cryptographically valid peer into *the*
/// peer we meant to reach — an attacker completing the handshake under their
/// own Noise + Ed25519 keys produces a valid signature for a different
/// `node_id`, and that mismatch is the MITM detection of ADR-0020 (threat
/// V-24).
pub fn verify_node_id(
    signature: &ed25519_dalek::Signature,
    msg: &[u8],
    public: &VerifyingKey,
    expected_node_id: &NodeId,
) -> Result<(), NetError> {
    public
        .verify_strict(msg, signature)
        .map_err(|_| NetError::Handshake("ed25519 signature verification failed".to_string()))?;
    let derived = NodeId::from_public_bytes(public.to_bytes());
    if &derived != expected_node_id {
        return Err(NetError::Handshake(format!(
            "node_id mismatch: expected {expected_node_id}, peer is {derived}"
        )));
    }
    Ok(())
}

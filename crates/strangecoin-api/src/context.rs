//! Shared execution context for the `agent_*` method layer (AI-P04).
//!
//! [`ApiContext`](crate::ApiContext) bundles everything a method needs —
//! session manager, challenge store, session subkeys, the node
//! [`AgentBackend`](crate::backend::AgentBackend) and the server config —
//! so method structs hold one `Arc` instead of five. The challenge store
//! implements the spec §5.1 session-open flow (issue → sign → consume,
//! single-use, TTL-bound).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use secp256k1::{PublicKey, SecretKey};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::auth::AuthMiddleware;
use crate::backend::AgentBackend;
use crate::error::{AgentError, AgentResult};
use crate::server::ServerConfig;
use crate::session::AgentSessionManager;

/// Outstanding challenges cap — issuance refuses past it (DoS guard for
/// the unauthenticated `agent_getChallenge` endpoint).
pub const MAX_PENDING_CHALLENGES: usize = 10_000;

/// Everything the `agent_*` method layer executes against.
pub struct ApiContext {
    /// Session lifecycle (create / lookup / TTL cleanup).
    pub session_manager: Arc<AgentSessionManager>,
    /// Bearer-token extraction + session-token verification.
    pub auth: Arc<AuthMiddleware>,
    /// Spec §5.1 challenge issue/consume store (single-use, TTL).
    pub challenges: Arc<ChallengeStore>,
    /// `session_id -> session-scoped batch subkey` (spec §5.2 — the server
    /// signs `agent_signAndSubmitBatch` outer txs with it; derived
    /// deterministically from the server seed, never from agent material).
    pub subkeys: Arc<RwLock<HashMap<String, SecretKey>>>,
    /// Node data plane (balances, mempool, simulation, chain scan).
    pub backend: Arc<dyn AgentBackend>,
    /// Server configuration (batch executor / escrow addresses, limits).
    pub config: Arc<ServerConfig>,
}

impl ApiContext {
    /// Assemble a context. Used by
    /// [`crate::AgentApiServer`](crate::AgentApiServer) in production and
    /// by [`ApiContext::for_tests`] in test code.
    pub fn new(
        session_manager: Arc<AgentSessionManager>,
        auth: Arc<AuthMiddleware>,
        backend: Arc<dyn AgentBackend>,
        config: Arc<ServerConfig>,
    ) -> Self {
        ApiContext {
            session_manager,
            auth,
            challenges: Arc::new(ChallengeStore::new(config.challenge_ttl_sec)),
            subkeys: Arc::new(RwLock::new(HashMap::new())),
            backend,
            config,
        }
    }

    /// Deterministic test context: fixed subkey seed, mock-friendly config.
    pub fn for_tests(backend: Arc<dyn AgentBackend>) -> Arc<Self> {
        let config = Arc::new(ServerConfig::test_default());
        let session_manager = Arc::new(AgentSessionManager::new(
            Arc::new(crate::session::InMemorySessionStore::new()),
            AuthMiddleware::new().auth().clone(),
            Duration::from_secs(config.session_timeout),
            config.max_sessions_per_agent,
        ));
        Arc::new(ApiContext::new(
            session_manager,
            Arc::new(AuthMiddleware::new()),
            backend,
            config,
        ))
    }
}

/// One outstanding authentication challenge (spec §5.1).
#[derive(Debug, Clone)]
struct ChallengeRecord {
    /// The agent pubkey the challenge was issued for.
    agent_pubkey: [u8; 33],
    /// The 32-byte value the client must sign.
    challenge: [u8; 32],
    /// Expiry (wall clock — auth edge, precedent: session `expires_at`).
    expires_at: DateTime<Utc>,
}

/// Single-use, TTL-bound challenge store for the session-open flow.
///
/// Challenges and ids use `Uuid::v4` (OS randomness) — the same
/// precedent as session ids in [`AgentSessionManager`]: auth-layer
/// unpredictability, not a consensus/network/storage path (Stage 2
/// determinism checklist scope).
pub struct ChallengeStore {
    ttl: Duration,
    inner: RwLock<HashMap<String, ChallengeRecord>>,
}

impl ChallengeStore {
    /// Store with the given challenge lifetime (seconds).
    pub fn new(ttl_secs: u64) -> Self {
        ChallengeStore {
            ttl: Duration::from_secs(ttl_secs.max(1)),
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Issue a challenge for `agent_pubkey`: returns `(challenge_id,
    /// challenge)` — the client signs `blake3(challenge)` (the recover
    /// helper hashes the message) with the agent keypair.
    pub async fn issue(&self, agent_pubkey: [u8; 33]) -> AgentResult<(String, [u8; 32])> {
        let mut inner = self.inner.write().await;
        if inner.len() >= MAX_PENDING_CHALLENGES {
            return Err(AgentError::RateLimitExceeded);
        }
        let id = Uuid::new_v4().to_string();
        let mut challenge = [0u8; 32];
        challenge[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        challenge[16..].copy_from_slice(Uuid::new_v4().as_bytes());
        inner.insert(
            id.clone(),
            ChallengeRecord {
                agent_pubkey,
                challenge,
                expires_at: Utc::now() + chrono::Duration::from_std(self.ttl).unwrap_or_default(),
            },
        );
        Ok((id, challenge))
    }

    /// Consume a challenge: single-use (removed), TTL-checked, bound to
    /// `agent_pubkey`. Returns the challenge bytes for signature
    /// verification on success.
    pub async fn consume(&self, id: &str, agent_pubkey: [u8; 33]) -> AgentResult<[u8; 32]> {
        let mut inner = self.inner.write().await;
        let record = inner
            .remove(id)
            .ok_or_else(|| AgentError::AuthenticationFailed("unknown or used challenge".into()))?;
        if Utc::now() > record.expires_at {
            return Err(AgentError::AuthenticationFailed("challenge expired".into()));
        }
        if record.agent_pubkey != agent_pubkey {
            return Err(AgentError::AuthenticationFailed(
                "challenge issued for a different key".into(),
            ));
        }
        Ok(record.challenge)
    }

    /// Drop expired challenges (called by the server's sweep task and
    /// directly by deterministic tests — no sleeps).
    pub async fn sweep(&self) {
        let mut inner = self.inner.write().await;
        let now = Utc::now();
        inner.retain(|_, rec| rec.expires_at > now);
    }
}

/// Derive the session-scoped batch subkey (spec §5.2): deterministic
/// `blake3(server_seed ‖ domain ‖ session_id ‖ counter)` clamped into a
/// valid secp256k1 scalar. No RNG — same seed + session id → same key
/// (Stage 2 determinism checklist pattern; the *session id* itself is
/// uuid-v4 like every existing session).
pub fn derive_session_subkey(server_seed: &[u8; 32], session_id: &str) -> SecretKey {
    let secp = secp256k1::Secp256k1::new();
    let mut counter = 0u32;
    loop {
        let mut hasher = blake3::Hasher::new();
        hasher.update(server_seed);
        hasher.update(b"agent_session_subkey_v1");
        hasher.update(session_id.as_bytes());
        hasher.update(&counter.to_le_bytes());
        let digest = hasher.finalize();
        if let Ok(sk) = SecretKey::from_slice(digest.as_bytes()) {
            // Defensive: also require the derived pubkey to serialize back
            // (always true for a valid SecretKey — keeps secp happy).
            let _ = PublicKey::from_secret_key(&secp, &sk);
            return sk;
        }
        counter = counter.wrapping_add(1); // astronomically rare retry
    }
}

/// Bech32 address of a session subkey on `chain_id`.
pub fn subkey_address(sk: &SecretKey, chain_id: u32) -> AgentResult<String> {
    let secp = secp256k1::Secp256k1::new();
    let pk = PublicKey::from_secret_key(&secp, sk);
    strangecoin_core::address::encode_address(&pk, chain_id)
        .map_err(|e| AgentError::InternalError(format!("subkey address: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn challenge_lifecycle_single_use_and_bound() {
        let store = ChallengeStore::new(300);
        let pk = [7u8; 33];
        let (id, challenge) = store.issue(pk).await.expect("issue");
        assert_eq!(challenge.len(), 32);

        // First consume wins; the record is gone afterwards.
        let got = store.consume(&id, pk).await.expect("consume");
        assert_eq!(got, challenge);
        assert!(
            store.consume(&id, pk).await.is_err(),
            "challenge must be single-use"
        );
    }

    #[tokio::test]
    async fn challenge_rejects_wrong_key_and_unknown_id() {
        let store = ChallengeStore::new(300);
        let (id, _) = store.issue([1u8; 33]).await.expect("issue");
        assert!(store.consume(&id, [2u8; 33]).await.is_err(), "wrong key");
        assert!(
            store.consume("no-such-id", [1u8; 33]).await.is_err(),
            "unknown id"
        );
    }

    #[tokio::test]
    async fn challenge_sweep_drops_expired() {
        // TTL of 1s → expired after the sleep below.
        let store = ChallengeStore::new(1);
        let (id, _) = store.issue([1u8; 33]).await.expect("issue");
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        store.sweep().await;
        assert!(
            store.consume(&id, [1u8; 33]).await.is_err(),
            "expired challenge must not survive the sweep"
        );
    }

    #[tokio::test]
    async fn challenge_issuance_is_capped() {
        let store = ChallengeStore::new(300);
        for _ in 0..MAX_PENDING_CHALLENGES {
            store.issue([1u8; 33]).await.expect("issue under cap");
        }
        assert!(store.issue([1u8; 33]).await.is_err(), "cap must refuse");
    }

    #[test]
    fn subkey_derivation_is_deterministic_and_seed_bound() {
        let seed = [0x42u8; 32];
        let a = derive_session_subkey(&seed, "agent_1");
        let b = derive_session_subkey(&seed, "agent_1");
        let c = derive_session_subkey(&seed, "agent_2");
        let d = derive_session_subkey(&[0x43u8; 32], "agent_1");
        assert_eq!(a.secret_bytes(), b.secret_bytes(), "same input → same key");
        assert_ne!(a.secret_bytes(), c.secret_bytes(), "session id bound");
        assert_ne!(a.secret_bytes(), d.secret_bytes(), "seed bound");
    }

    #[test]
    fn subkey_address_is_valid_bech32_on_chain() {
        let sk = derive_session_subkey(&[9u8; 32], "agent_x");
        let addr = subkey_address(&sk, 3).expect("regtest address");
        assert!(addr.starts_with("rsc1"), "regtest HRP, got {addr}");
    }
}

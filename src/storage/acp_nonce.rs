//! `MetaNonceStore` — ACP replay-nonce persistence (AI-P03, invariant #25,
//! ADR-0030 §Replay protection: "backed by RocksDB `meta` CF … restart
//! survival").
//!
//! Key: [`meta_key`]`("acp_nonce:{sender_hex}:{counterparty_hex}")` — the
//! ADR's literal `acp_nonce:{agent_id}:{counterparty}` shape through the
//! schema's identity key constructor (ADR-0024 `meta` CF). Value: big-endian
//! `u64` last-accepted nonce (spec §6.2).
//!
//! Lazy dual-read: the in-memory map answers hot lookups; a miss falls
//! through to the `meta` CF and re-primes the map (so restart-survival needs
//! no bulk load at startup). Writes go to the CF **first** and only then to
//! memory — a failed CF write surfaces as `Err` and the handler rejects the
//! message fail-closed (spec §6.2: "synchronous write … before processing;
//! fail-closed on write error").

use crate::storage::{meta_key, Cf, StorageEngine};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use strangecoin_net::acp::AcpNonceStore;
use strangecoin_net::AgentId;

/// Storage-backed nonce store (production wiring — `AcpHandler` takes it in
/// `Node::start_server`).
pub struct MetaNonceStore {
    engine: Arc<dyn StorageEngine>,
    /// Hot cache of `(sender, counterparty) → last_nonce`.
    cache: Mutex<HashMap<(AgentId, AgentId), u64>>,
}

impl MetaNonceStore {
    /// Wrap `engine` with an empty cache (pairs prime on first touch).
    pub fn new(engine: Arc<dyn StorageEngine>) -> Self {
        MetaNonceStore {
            engine,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The `meta` CF key for one direction (ADR-0030 layout).
    fn key(sender: AgentId, counterparty: AgentId) -> Vec<u8> {
        meta_key(&format!(
            "acp_nonce:{}:{}",
            hex::encode(sender.0),
            hex::encode(counterparty.0)
        ))
    }
}

impl AcpNonceStore for MetaNonceStore {
    fn last_nonce(&self, sender: AgentId, counterparty: AgentId) -> Result<Option<u64>, String> {
        let cached = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(sender, counterparty))
            .copied();
        if cached.is_some() {
            return Ok(cached);
        }
        let raw = self
            .engine
            .get(Cf::Meta, &Self::key(sender, counterparty))
            .map_err(|err| format!("meta cf read: {err}"))?;
        let Some(bytes) = raw else {
            return Ok(None);
        };
        let nonce_bytes: [u8; 8] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| format!("acp nonce value must be 8 BE bytes, got {}", bytes.len()))?;
        let nonce = u64::from_be_bytes(nonce_bytes);
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((sender, counterparty), nonce);
        Ok(Some(nonce))
    }

    fn store_nonce(
        &self,
        sender: AgentId,
        counterparty: AgentId,
        nonce: u64,
    ) -> Result<(), String> {
        // Write-ahead (spec §6.2): CF first — memory only mirrors a
        // durable write, so a failed write can never roll the window back.
        self.engine
            .put(
                Cf::Meta,
                &Self::key(sender, counterparty),
                &nonce.to_be_bytes(),
            )
            .map_err(|err| format!("meta cf write: {err}"))?;
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((sender, counterparty), nonce);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal in-memory engine: only `Cf::Meta` get/put are exercised here
    /// (the full engine matrix lives in `strangecoin-storage`). `put` is
    /// overridden directly — the trait's default routes through
    /// `write_batch`, which this mock deliberately leaves inert.
    #[derive(Default)]
    struct MetaOnlyEngine(std::sync::Mutex<std::collections::BTreeMap<Vec<u8>, Vec<u8>>>);

    impl StorageEngine for MetaOnlyEngine {
        fn get(
            &self,
            cf: Cf,
            key: &[u8],
        ) -> Result<Option<Vec<u8>>, strangecoin_storage::StorageError> {
            if cf != Cf::Meta {
                return Ok(None);
            }
            Ok(self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(key)
                .cloned())
        }

        fn put(
            &self,
            cf: Cf,
            key: &[u8],
            value: &[u8],
        ) -> Result<(), strangecoin_storage::StorageError> {
            if cf == Cf::Meta {
                self.0
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(key.to_vec(), value.to_vec());
            }
            Ok(())
        }

        fn write_batch(
            &self,
            _ops: Vec<strangecoin_storage::WriteBatchOp>,
        ) -> Result<(), strangecoin_storage::StorageError> {
            Ok(())
        }

        fn iterate_prefix(
            &self,
            _cf: Cf,
            _prefix: &[u8],
        ) -> Result<Vec<strangecoin_storage::KvPair>, strangecoin_storage::StorageError> {
            Ok(Vec::new())
        }

        fn flush(&self) -> Result<(), strangecoin_storage::StorageError> {
            Ok(())
        }
    }

    fn agent(byte: u8) -> AgentId {
        AgentId([byte; 33])
    }

    #[test]
    fn write_ahead_then_read_back_and_survive_cache_drop() {
        let engine: Arc<dyn StorageEngine> = Arc::new(MetaOnlyEngine::default());
        let store = MetaNonceStore::new(engine.clone());

        let a = agent(1);
        let b = agent(2);
        assert_eq!(store.last_nonce(a, b).unwrap(), None);
        store.store_nonce(a, b, 41).unwrap();
        assert_eq!(store.last_nonce(a, b).unwrap(), Some(41));
        // Opposite direction is an independent counter (spec §6.3).
        assert_eq!(store.last_nonce(b, a).unwrap(), None);

        // "Restart": a fresh store over the same engine re-reads the CF.
        let restarted = MetaNonceStore::new(engine);
        assert_eq!(restarted.last_nonce(a, b).unwrap(), Some(41));
    }
}

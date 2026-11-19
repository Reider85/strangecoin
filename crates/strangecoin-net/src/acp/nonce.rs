//! ACP replay protection (invariant #25, ADR-0030 + spec §6): monotonic
//! `agent_nonce` per `(sender, counterparty)` pair.
//!
//! The handler checks and persists **write-ahead** (spec §6.2): the fresh
//! nonce lands in the store *before* the message body is processed — if the
//! store write fails, the message is rejected (fail-closed: a nonce map that
//! silently rolled back would reopen the replay window). A rejected *stale*
//! nonce consumes nothing.
//!
//! Two directions are independent counters (`(A, B)` and `(B, A)` —
//! spec §6.3), so both agents allocate their own sequences without
//! coordination.
//!
//! [`AcpNonceStore`] is the persistence seam: [`MemoryNonceStore`] backs
//! tests and feature-off builds; the monolith's `MetaNonceStore` writes the
//! RocksDB `meta` CF (key `acp_nonce:{a_hex}:{b_hex}`, big-endian u64 —
//! ADR-0024/ADR-0030 restart-survival) and reads it back lazily on first
//! touch per pair.

use crate::protocol::acp::AgentId;
use std::collections::HashMap;
use std::sync::Mutex;

/// Persistence seam for the last-seen nonce of each direction.
pub trait AcpNonceStore: Send + Sync {
    /// Last accepted nonce for `(sender, counterparty)`, `None` when the
    /// pair has never spoken. `Err` = local storage failure (fail-closed —
    /// the handler rejects the message without touching score state).
    fn last_nonce(&self, sender: AgentId, counterparty: AgentId) -> Result<Option<u64>, String>;

    /// Persist `nonce` as the new last-seen value **before** message
    /// processing (spec §6.2 write-ahead). `Err` = local storage failure.
    fn store_nonce(&self, sender: AgentId, counterparty: AgentId, nonce: u64)
        -> Result<(), String>;
}

/// In-memory store: tests, feature-off builds, and any node that opted out
/// of on-disk nonce persistence.
#[derive(Debug, Default)]
pub struct MemoryNonceStore {
    map: Mutex<HashMap<(AgentId, AgentId), u64>>,
}

impl MemoryNonceStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current value for a pair — test observability hook.
    pub fn get(&self, sender: AgentId, counterparty: AgentId) -> Option<u64> {
        self.map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(sender, counterparty))
            .copied()
    }

    /// Number of tracked directions.
    pub fn len(&self) -> usize {
        self.map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// `true` when no direction has spoken yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AcpNonceStore for MemoryNonceStore {
    fn last_nonce(&self, sender: AgentId, counterparty: AgentId) -> Result<Option<u64>, String> {
        Ok(self
            .map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&(sender, counterparty))
            .copied())
    }

    fn store_nonce(
        &self,
        sender: AgentId,
        counterparty: AgentId,
        nonce: u64,
    ) -> Result<(), String> {
        self.map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert((sender, counterparty), nonce);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(byte: u8) -> AgentId {
        AgentId([byte; 33])
    }

    #[test]
    fn memory_store_roundtrip_per_direction() {
        let store = MemoryNonceStore::new();
        let a = agent(1);
        let b = agent(2);
        assert_eq!(store.last_nonce(a, b).unwrap(), None);
        store.store_nonce(a, b, 7).unwrap();
        assert_eq!(store.last_nonce(a, b).unwrap(), Some(7));
        // Bidirectional independence (spec §6.3).
        assert_eq!(store.last_nonce(b, a).unwrap(), None);
        store.store_nonce(b, a, 3).unwrap();
        assert_eq!(store.last_nonce(a, b).unwrap(), Some(7));
        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());
    }
}

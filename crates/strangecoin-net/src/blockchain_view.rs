//! Read/write surface the network runtime needs from the node's blockchain
//! (S2-P06 cycle break, ARCHITECT3 §17 PR-checklist: "Sync engine: разрыв
//! цикла network↔blockchain").
//!
//! The strangler pattern moves `network/*` into this crate, but the concrete
//! `BlockchainFacade` lives in the monolith — so the engine talks to
//! [`BlockchainView`] instead. The monolith implements the trait for its
//! facade; errors are `String` because only their `Display` ever surfaces
//! here (tracing fields), and keeping the trait free of the monolith's error
//! type keeps the dependency one-way.

use std::collections::HashMap;

use strangecoin_core::types::{AccountState, Block, ChainSnapshot, Transaction};

/// Object-safe view of the local chain + mempool used by the sync engine.
///
/// `Send + Sync` supertraits: `Arc<dyn BlockchainView>` is moved between
/// tokio tasks and worker threads. Every method is `&self` — the facade's
/// internal `RwLock` provides the mutation.
pub trait BlockchainView: Send + Sync {
    /// Hash of the current tip (empty chain → genesis handling by facade).
    fn tip_hash(&self) -> String;
    /// Number of blocks in the canonical chain.
    fn chain_len(&self) -> usize;
    /// Try to adopt a full candidate chain (fork choice + validation inside).
    /// `Ok(true)` = adopted, `Ok(false)` = rejected by fork choice/validation.
    fn adopt_candidate(
        &self,
        candidate_chain: Vec<Block>,
        candidate_balances: Option<HashMap<String, AccountState>>,
        candidate_mempool_txs: Vec<Transaction>,
        candidate_difficulty: u32,
    ) -> Result<bool, String>;
    /// Validate + apply a single transaction to the mempool/state.
    fn apply_tx(&self, transaction: Transaction) -> Result<(), String>;
    /// Validate + append a block to the canonical chain.
    fn add_block(&self, block: Block) -> Result<(), String>;
    /// Is this transaction already in the chain (any block)?
    fn chain_contains_tx(&self, tx: &Transaction) -> bool;
    /// Is this txid pending in the mempool?
    fn mempool_contains(&self, txid: &[u8; 32]) -> bool;
    /// Full pending transaction by txid, if present (S2-P13: gossip
    /// `GET_DATA` → `TX` response). `None` = not in the mempool.
    fn mempool_tx(&self, txid: &[u8; 32]) -> Option<Transaction>;
    /// All pending mempool transactions (S2-P14: compact-block fill-up
    /// indexes them by siphash short ID). Order is not consensus-relevant.
    fn mempool_txs(&self) -> Vec<Transaction>;
    /// Full canonical block by its hash, if present (S2-P13: gossip
    /// `GET_DATA` → `BLOCK` response). `None` = unknown block.
    fn block_by_hash(&self, block_hash: &str) -> Option<Block>;
    /// Persist current state (no-op semantics owned by the facade).
    fn save_state(&self);
    /// Does a block with this hash still exist in the canonical chain?
    /// Replaces the monolith's `with_inner(|bc| bc.chain.iter()…)` closure —
    /// the trait must not leak `Blockchain` internals.
    fn chain_contains_block(&self, block_hash: &str) -> bool;
    /// Wire snapshot (balances + chain + mempool) for the GUI/sync channel.
    fn snapshot_wire(&self) -> ChainSnapshot;
}

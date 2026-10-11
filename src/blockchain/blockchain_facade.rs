//! # Blockchain facade (ARCHITECT3 §3.4, component 4+1)
//!
//! Two layers:
//!
//! * [`Blockchain`] — chain data (blocks, state cache, mempool, storage,
//!   consensus rules). Fields are `pub(crate)`: outside this module reach
//!   state only through [`BlockchainFacade`]. Heavy logic lives in sibling
//!   components (S1.5-P04): mining/genesis/grant in `block_executor`,
//!   migrations/storage in `state_cache`, fork-choice adoption in
//!   `chain_selector`, wire snapshot + serde in `chain_selector`
//!   ([`BlockchainDeserialize`]).
//! * [`BlockchainFacade`] — sole public entry point. Wraps
//!   `Arc<RwLock<Blockchain>>`, takes the lock per call.
//!
//! Fork choice: [`BlockchainFacade::adopt_candidate`] →
//! `chain_selector::try_adopt_candidate` (work → timestamp → hash).
//! Consensus rules: never resolved here; executor gets them via `BlockView`
//! from [`ConsensusManager`](super::consensus_manager::ConsensusManager).

use std::collections::HashMap;
use std::sync::{mpsc, Arc, RwLock};

use super::block_executor::{self, BlockView};
use super::chain_selector::ChainSelector;
use super::consensus_manager::ConsensusManager;
use super::state_cache::StateCache;
use crate::error::StrangecoinError;
use crate::AccountState;
use strangecoin_core::types::{Block, BlockHeader, ChainSnapshot, Transaction};

/// Chain data. Not part of the public API — use [`BlockchainFacade`].
#[derive(Clone)]
pub struct Blockchain {
    pub(crate) chain: Vec<Block>,
    pub(crate) balances: StateCache,
    pub(crate) difficulty: u32,
    pub(crate) mempool: crate::mempool::Mempool,
    pub(crate) storage: crate::storage::Storage,
    pub(crate) allow_grant_blocks: bool,
    pub(crate) allow_zero_state_root: bool,
    pub(crate) total_work: strangecoin_core::consensus::U256,
    pub(crate) rules: ConsensusManager,
    /// Network this node validates (BUG-S1-004): propagated from
    /// `Config.network_id`; drives genesis, rewards and tx chain_id checks.
    pub(crate) chain_id: u32,
}

impl Blockchain {
    /// Open storage for `port` via `state_cache::open_blockchain`.
    pub(crate) fn new(port: u16, chain_id: u32) -> Self {
        super::state_cache::open_blockchain(port, chain_id)
    }

    pub(crate) fn view_for(&self, height: u64) -> BlockView<'_> {
        BlockView::new(
            &self.chain,
            block_executor::now_secs(),
            self.allow_grant_blocks,
            self.rules.expected_version(height),
            self.chain_id,
        )
        .with_phase(self.rules.phase_at(height))
        .with_allow_zero_state_root(self.allow_zero_state_root)
    }

    pub(crate) fn calculate_hash(&self, block: &Block) -> String {
        hex::encode(strangecoin_core::serialize::block_hash(block))
    }
}

/// Public entry point over `Arc<RwLock<Blockchain>>`.
///
/// Every method takes the lock itself; callers must never hold the lock across
/// a second facade call (RwLock is not re-entrant).
#[derive(Clone)]
pub struct BlockchainFacade {
    inner: Arc<RwLock<Blockchain>>,
    event_bus: Arc<crate::events::EventBus>,
}

impl BlockchainFacade {
    pub fn new(port: u16, chain_id: u32) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Blockchain::new(port, chain_id))),
            event_bus: Arc::new(crate::events::EventBus::new()),
        }
    }

    pub fn with_event_bus(
        port: u16,
        event_bus: Arc<crate::events::EventBus>,
        chain_id: u32,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Blockchain::new(port, chain_id))),
            event_bus,
        }
    }

    pub fn from_blockchain(bc: Blockchain) -> Self {
        Self {
            inner: Arc::new(RwLock::new(bc)),
            event_bus: Arc::new(crate::events::EventBus::new()),
        }
    }

    pub fn event_bus(&self) -> Arc<crate::events::EventBus> {
        Arc::clone(&self.event_bus)
    }

    // ------------------------------------------------------------------ chain

    pub fn chain_len(&self) -> usize {
        self.inner.read().expect(BLOCKCHAIN_LOCK).chain.len()
    }

    pub fn tip(&self) -> Option<Block> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).chain.last().cloned()
    }

    pub fn tip_hash(&self) -> String {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .chain
            .last()
            .map(|b| b.hash.clone())
            .unwrap_or_default()
    }

    pub fn chain_snapshot(&self) -> Vec<Block> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).chain.clone()
    }

    pub fn chain_info(&self) -> Option<super::chain_selector::ChainInfo> {
        ChainSelector::chain_info(&self.inner.read().expect(BLOCKCHAIN_LOCK).chain)
    }

    pub fn total_work(&self) -> strangecoin_core::consensus::U256 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).total_work
    }

    pub fn difficulty(&self) -> u32 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).difficulty
    }

    /// Network this node validates (`Config.network_id`, BUG-S1-004).
    pub fn chain_id(&self) -> u32 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).chain_id
    }

    pub fn push_block_unchecked(&self, block: Block) {
        self.inner.write().expect(BLOCKCHAIN_LOCK).chain.push(block);
    }

    pub fn add_block(&self, block: Block) -> Result<(), StrangecoinError> {
        let mut guard = self.inner.write().expect(BLOCKCHAIN_LOCK);
        guard.commit_block(&block)?;
        drop(guard);
        self.save_state();
        Ok(())
    }

    pub fn chain_contains_tx(&self, tx: &Transaction) -> bool {
        super::chain_selector::chain_has_tx(&self.inner.read().expect(BLOCKCHAIN_LOCK).chain, tx)
    }

    // ------------------------------------------------------------------ state

    pub fn get_balance(&self, address: &str) -> u64 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.balance(address)
    }

    pub fn get_nonce(&self, address: &str) -> u64 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.nonce(address)
    }

    pub fn get_account(&self, address: &str) -> Option<AccountState> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.get(address)
    }

    pub fn has_account(&self, address: &str) -> bool {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.contains_key(address)
    }

    pub fn first_account(&self) -> Option<String> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.keys().next().cloned()
    }

    pub fn account_keys(&self) -> Vec<String> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.keys().cloned().collect()
    }

    pub fn total_supply(&self) -> u64 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.total_supply()
    }

    pub fn nonzero_balances(&self) -> HashMap<String, u64> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.nonzero_balances()
    }

    pub fn state_snapshot(&self) -> StateCache {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.clone()
    }

    pub fn ensure_account(&self, address: &str) {
        self.inner.write().expect(BLOCKCHAIN_LOCK).balances.ensure_account(address);
    }

    pub fn with_state_cache_mut<R>(&self, f: impl FnOnce(&mut StateCache) -> R) -> R {
        f(&mut self.inner.write().expect(BLOCKCHAIN_LOCK).balances)
    }

    pub fn rebuild_state_cache(&self) -> Result<(), StrangecoinError> {
        self.inner.write().expect(BLOCKCHAIN_LOCK).rebuild_state_cache()
    }

    // ---------------------------------------------------------------- mempool

    pub fn mempool_len(&self) -> usize {
        self.inner.read().expect(BLOCKCHAIN_LOCK).mempool.len()
    }

    pub fn mempool_is_empty(&self) -> bool {
        self.inner.read().expect(BLOCKCHAIN_LOCK).mempool.is_empty()
    }

    pub fn mempool_contains(&self, txid: &[u8; 32]) -> bool {
        self.inner.read().expect(BLOCKCHAIN_LOCK).mempool.contains(txid)
    }

    pub fn mempool_transactions(&self) -> Vec<Transaction> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).mempool.transactions()
    }

    pub fn apply_tx(&self, transaction: Transaction) -> Result<(), StrangecoinError> {
        let outcome = self
            .inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .add_transaction(transaction)?;
        if let crate::mempool::InsertOutcome::Replaced(evicted) = outcome {
            for evicted_id in evicted {
                self.event_bus.publish(crate::events::NodeEvent::TxRejected {
                    txid: hex::encode(evicted_id),
                    reason: crate::events::REASON_REPLACED.to_string(),
                });
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------- flags

    pub fn allow_grant_blocks(&self) -> bool {
        self.inner.read().expect(BLOCKCHAIN_LOCK).allow_grant_blocks
    }

    pub fn set_allow_grant_blocks(&self, allow: bool) {
        self.inner.write().expect(BLOCKCHAIN_LOCK).allow_grant_blocks = allow;
    }

    /// SCIP-0002 / BUG-S1-002: legacy regtest opt-in for zero `state_root`
    /// blocks. Never enabled on mainnet/testnet (`Config::validate`).
    pub fn set_allow_zero_state_root(&self, allow: bool) {
        self.inner.write().expect(BLOCKCHAIN_LOCK).allow_zero_state_root = allow;
    }

    // -------------------------------------------------------------- operations

    pub fn mine_block(
        &self,
        progress_tx: mpsc::Sender<String>,
        shutdown: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Option<Block> {
        self.inner.write().expect(BLOCKCHAIN_LOCK).mine_block(progress_tx, shutdown)
    }

    pub fn validate_chain(&self) -> bool {
        self.inner.read().expect(BLOCKCHAIN_LOCK).validate_chain()
    }

    pub fn save_state(&self) {
        self.inner.write().expect(BLOCKCHAIN_LOCK).save_state();
    }

    pub fn grant_initial_balance_to_first_wallet(
        &self,
        wallet_address: &str,
    ) -> Result<bool, StrangecoinError> {
        self.inner.write().expect(BLOCKCHAIN_LOCK).grant_initial_balance_to_first_wallet(wallet_address)
    }

    pub fn calculate_hash(&self, block: &Block) -> String {
        self.inner.read().expect(BLOCKCHAIN_LOCK).calculate_hash(block)
    }

    pub fn rules(&self) -> ConsensusManager {
        self.inner.read().expect(BLOCKCHAIN_LOCK).rules.clone()
    }

    // ------------------------------------------------------------------- wire

    pub fn snapshot_wire(&self) -> ChainSnapshot {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        ChainSnapshot {
            chain: guard.chain.clone(),
            balances: guard.balances.accounts().clone(),
            difficulty: guard.difficulty,
            mempool_txs: Vec::new(),
            total_work: guard.total_work,
        }
    }

    pub fn to_wire_json(&self) -> String {
        serde_json::to_string(&*self.inner.read().expect(BLOCKCHAIN_LOCK)).unwrap()
    }

    pub fn headers_from_height(&self, from_height: u64, max: usize) -> Vec<BlockHeader> {
        super::chain_selector::headers_from_height(
            &self.inner.read().expect(BLOCKCHAIN_LOCK).chain,
            from_height,
            max,
        )
    }

    pub fn blocks_by_hashes(&self, hashes: &[[u8; 32]], max: usize) -> Vec<Block> {
        super::chain_selector::blocks_by_hashes(
            &self.inner.read().expect(BLOCKCHAIN_LOCK).chain,
            hashes,
            max,
        )
    }

    pub fn adopt_wire(&self, snapshot: ChainSnapshot) -> Result<bool, StrangecoinError> {
        self.adopt_candidate(
            snapshot.chain,
            Some(snapshot.balances),
            snapshot.mempool_txs,
            snapshot.difficulty,
        )
    }

    // -------------------------------------------------------------- adoption

    /// Fork-choice adoption via `chain_selector::try_adopt_candidate`.
    pub fn adopt_candidate(
        &self,
        candidate_chain: Vec<Block>,
        candidate_balances: Option<HashMap<String, AccountState>>,
        candidate_mempool_txs: Vec<Transaction>,
        candidate_difficulty: u32,
    ) -> Result<bool, StrangecoinError> {
        let mut guard = self.inner.write().expect(BLOCKCHAIN_LOCK);
        let adopted = super::chain_selector::try_adopt_candidate(
            &mut guard,
            candidate_chain,
            candidate_balances,
            candidate_mempool_txs,
            candidate_difficulty,
        )?;
        drop(guard);
        if adopted {
            self.save_state();
        }
        Ok(adopted)
    }

    // -------------------------------------------------------- escape hatches

    pub fn with_inner<R>(&self, f: impl FnOnce(&Blockchain) -> R) -> R {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        f(&guard)
    }

    pub fn with_inner_mut<R>(&self, f: impl FnOnce(&mut Blockchain) -> R) -> R {
        let mut guard = self.inner.write().expect(BLOCKCHAIN_LOCK);
        f(&mut guard)
    }
}

const BLOCKCHAIN_LOCK: &str = "blockchain lock poisoned";

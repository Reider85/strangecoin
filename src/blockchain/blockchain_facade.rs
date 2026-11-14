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
use crate::storage::{
    block_by_height_key, block_hash_index_key, codec, contract_storage_root_key,
    decode_tx_index_value, tx_index_key, tx_raw_key, Cf,
};
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
    /// Contract code registry (S1.5-P14): bech32 address → WASM bytecode,
    /// reconstructed from the chain's `Create` transactions (invariant #1
    /// style) — never trusted from the wire.
    pub(crate) codes: HashMap<String, Vec<u8>>,
    /// Receipts of the most recently committed block (S1.5-P14, invariant
    /// #18): replaced wholesale on commit/adopt, never mutated in place.
    pub(crate) receipts: Vec<strangecoin_core::vm::Receipt>,
    /// Accounts as they exist in the `state` CF right now (S3-P07): the
    /// diff base for incremental saves — `save_state` writes only what
    /// differs from this snapshot and then refreshes it. Initialized from
    /// the loaded state at open, so restarts rewrite nothing.
    pub(crate) persisted_accounts: HashMap<String, AccountState>,
    /// Contract slots as they exist in the `contract_storage` CF (S3-P07
    /// diff base, same contract as [`Self::persisted_accounts`]).
    pub(crate) persisted_contract_storage: strangecoin_core::state::ContractStorageMap,
    /// `(sender, nonce) → txid` of the mempool entries in the `mempool`
    /// CF (S3-P07 diff base): drives put-new / delete-mined deltas.
    pub(crate) persisted_mempool: HashMap<(String, u64), [u8; 32]>,
    /// Pre-apply account snapshots captured at commit/adoption, waiting
    /// for the next `save_state` to land in `state_history` (ADR-0024).
    pub(crate) pending_state_history: Vec<crate::blockchain::state_cache::PendingHistory>,
}

impl Blockchain {
    /// Open storage for `port` via `state_cache::open_blockchain`.
    pub(crate) fn new(port: u16, chain_id: u32, storage: &crate::config::StorageConfig) -> Self {
        super::state_cache::open_blockchain(port, chain_id, storage)
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
        .with_codes(&self.codes)
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
    /// Open the port-based node database per `storage` (S3-P07: the
    /// config-driven `storage.path`; S3-P08: a leftover historical
    /// `<exe_dir>/blockchain_db_{port}` LevelDB directory is auto-migrated
    /// into it first — see `storage::migrations`).
    pub fn new(port: u16, chain_id: u32, storage: crate::config::StorageConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Blockchain::new(port, chain_id, &storage))),
            event_bus: Arc::new(crate::events::EventBus::new()),
        }
    }

    pub fn with_event_bus(
        port: u16,
        event_bus: Arc<crate::events::EventBus>,
        chain_id: u32,
        storage: crate::config::StorageConfig,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Blockchain::new(port, chain_id, &storage))),
            event_bus,
        }
    }

    pub fn from_blockchain(bc: Blockchain) -> Self {
        Self {
            inner: Arc::new(RwLock::new(bc)),
            event_bus: Arc::new(crate::events::EventBus::new()),
        }
    }

    /// Open a standalone chain at `db_path` for `chain_id` (S2-P20
    /// shadow/staging replay, and the test-support constructor). Creates
    /// the directory, opens the RocksDB backend, loads whatever the
    /// directory already holds (genesis only on an empty database —
    /// S3-P07: a reused directory resumes instead of silently wiping),
    /// and wraps the chain — no port-based DB path, no event bus wiring.
    /// Validation is regtest-permissive (`allow_grant_blocks`/
    /// `allow_zero_state_root`) so replay tooling can exercise genesis
    /// and historical blocks; strict mainnet-rule staging configs are a
    /// Stage 4 CLI concern.
    pub fn open_local(db_path: &std::path::Path, chain_id: u32) -> Self {
        let storage =
            crate::storage::Storage::open_rocks(db_path).expect("Failed to open local DB");
        Self::open_local_with(db_path, chain_id, storage)
    }

    /// [`Self::open_local`] over a caller-supplied [`Storage`] (test
    /// engines — counting mocks inject an in-memory backend, `db_path`
    /// is unused there). Shared by `test_support`.
    pub(crate) fn open_local_with(
        db_path: &std::path::Path,
        chain_id: u32,
        storage: crate::storage::Storage,
    ) -> Self {
        let _ = db_path; // directory creation happens in `Storage::open_rocks`
        let mut bc = Blockchain {
            chain: vec![],
            balances: StateCache::new(),
            difficulty: 0,
            mempool: crate::mempool::Mempool::new(chain_id),
            storage,
            allow_grant_blocks: true,
            allow_zero_state_root: true,
            total_work: strangecoin_core::consensus::U256::default(),
            rules: ConsensusManager::new(),
            chain_id,
            codes: std::collections::HashMap::new(),
            receipts: Vec::new(),
            persisted_accounts: HashMap::new(),
            persisted_contract_storage: strangecoin_core::state::ContractStorageMap::new(),
            persisted_mempool: HashMap::new(),
            pending_state_history: Vec::new(),
        };

        // Load-if-present (S3-P07): fresh databases take the historical
        // `create_genesis_block` path unchanged; a directory with data
        // resumes it (no save at open — first mutation persists).
        let loaded = super::state_cache::load_persisted_state(&bc.storage)
            .expect("Failed to load local DB state");
        bc.persisted_accounts = loaded.balances.clone().unwrap_or_default();
        bc.persisted_contract_storage = loaded.contract_storage.clone().unwrap_or_default();
        bc.persisted_mempool = loaded
            .mempool_txs
            .iter()
            .map(|tx| {
                (
                    (tx.sender().to_string(), tx.nonce()),
                    strangecoin_core::serialize::txid(tx),
                )
            })
            .collect();
        // Whole-field `balances` replacement lives in state_cache (balances
        // gate, BUG-S0-022) — both open paths funnel through the helper.
        super::state_cache::apply_loaded_accounts(&mut bc, &loaded);
        if let Some(chain) = loaded.chain {
            if !chain.is_empty() {
                crate::consensus::validate_genesis(&chain[0], chain_id)
                    .expect("Genesis validation failed");
                bc.chain = chain;
            }
        }
        if bc.chain.is_empty() {
            bc.create_genesis_block();
        }
        bc.total_work = strangecoin_core::consensus::cumulative_work(&bc.chain);
        for tx in loaded.mempool_txs {
            let account = bc.balances.get(tx.sender()).unwrap_or_default();
            let _ = bc.mempool.insert(tx, &account);
        }
        bc.codes = block_executor::codes_from_chain(&bc.chain, chain_id);
        Self::from_blockchain(bc)
    }

    pub fn event_bus(&self) -> Arc<crate::events::EventBus> {
        Arc::clone(&self.event_bus)
    }

    // ------------------------------------------------------------------ chain

    pub fn chain_len(&self) -> usize {
        self.inner.read().expect(BLOCKCHAIN_LOCK).chain.len()
    }

    pub fn tip(&self) -> Option<Block> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .chain
            .last()
            .cloned()
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

    /// Receipts of the most recently committed block (S1.5-P14, invariant
    /// #18). Empty right after a restart until the next commit/adopt —
    /// receipts are derived data, recomputed from the chain.
    pub fn tip_receipts(&self) -> Vec<strangecoin_core::vm::Receipt> {
        self.inner.read().expect(BLOCKCHAIN_LOCK).receipts.clone()
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

    /// Подтверждён ли `tx` в канонической цепочке. S3-P09: O(1) через
    /// `txid → (height, tx_index)` индекс — прежний linear scan по
    /// `chain` удалён.
    pub fn chain_contains_tx(&self, tx: &Transaction) -> bool {
        self.tx_location(&strangecoin_core::serialize::txid(tx))
            .is_some()
    }

    /// S3-P09: O(1) `txid → (height, tx_index)` (point-get в `transactions`
    /// CF). `None` = tx не подтверждён / не проиндексирован.
    pub fn tx_location(&self, txid: &[u8; 32]) -> Option<(u64, u32)> {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        let raw = guard
            .storage
            .engine()
            .get(Cf::Transactions, &tx_index_key(txid))
            .ok()
            .flatten()?;
        decode_tx_index_value(&raw)
    }

    /// Подтверждённая tx по txid: S3-P09 raw-запись в `transactions` CF
    /// (Stage 4 ADR-0030 escrow-валидация читает отсюда). Для tx из
    /// mempool — [`Self::mempool_tx`].
    pub fn get_tx(&self, txid: &[u8; 32]) -> Option<Transaction> {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        let raw = guard
            .storage
            .engine()
            .get(Cf::Transactions, &tx_raw_key(txid))
            .ok()
            .flatten()?;
        codec::decode_tx(&raw).ok()
    }

    /// S3-P09: O(1) кэш `contract_address → storage_root` (point-get в
    /// `contract_storage` CF, tagged sub-space). `None` = нет записи
    /// (не-контракт или контракт без storage-записей). Авторитетный
    /// источник — `AccountState.storage_root`; кэш инвалидируется при
    /// каждой записи слотов (см. `save_state_engine`).
    pub fn contract_storage_root(&self, address: &str) -> Option<[u8; 32]> {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        let raw = guard
            .storage
            .engine()
            .get(Cf::ContractStorage, &contract_storage_root_key(address))
            .ok()
            .flatten()?;
        raw.try_into().ok()
    }

    // ------------------------------------------------------------------ state

    pub fn get_balance(&self, address: &str) -> u64 {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .balance(address)
    }

    pub fn get_nonce(&self, address: &str) -> u64 {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .nonce(address)
    }

    pub fn get_account(&self, address: &str) -> Option<AccountState> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .get(address)
    }

    pub fn has_account(&self, address: &str) -> bool {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .contains_key(address)
    }

    pub fn first_account(&self) -> Option<String> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .keys()
            .next()
            .cloned()
    }

    pub fn account_keys(&self) -> Vec<String> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .keys()
            .cloned()
            .collect()
    }

    pub fn total_supply(&self) -> u64 {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .total_supply()
    }

    pub fn nonzero_balances(&self) -> HashMap<String, u64> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .nonzero_balances()
    }

    pub fn state_snapshot(&self) -> StateCache {
        self.inner.read().expect(BLOCKCHAIN_LOCK).balances.clone()
    }

    pub fn ensure_account(&self, address: &str) {
        self.inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .balances
            .ensure_account(address);
    }

    pub fn with_state_cache_mut<R>(&self, f: impl FnOnce(&mut StateCache) -> R) -> R {
        f(&mut self.inner.write().expect(BLOCKCHAIN_LOCK).balances)
    }

    pub fn rebuild_state_cache(&self) -> Result<(), StrangecoinError> {
        self.inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .rebuild_state_cache()
    }

    // ---------------------------------------------------------------- mempool

    pub fn mempool_len(&self) -> usize {
        self.inner.read().expect(BLOCKCHAIN_LOCK).mempool.len()
    }

    pub fn mempool_is_empty(&self) -> bool {
        self.inner.read().expect(BLOCKCHAIN_LOCK).mempool.is_empty()
    }

    pub fn mempool_contains(&self, txid: &[u8; 32]) -> bool {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .mempool
            .contains(txid)
    }

    /// Тx по txid из mempool (S2-P13 gossip GET_DATA → TX).
    pub fn mempool_tx(&self, txid: &[u8; 32]) -> Option<Transaction> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .mempool
            .get(txid)
            .cloned()
    }

    /// Канонический блок по hex-хешу (S2-P13 gossip GET_DATA → BLOCK).
    ///
    /// S3-P09: O(1) — `hash → height` индекс, затем point-get блока;
    /// прежний scan по `chain` удалён. Stale/битый индекс (прочитанный
    /// блок не совпадает хешем) → `None`, как и невалидный hex.
    pub fn block_by_hash(&self, block_hash: &str) -> Option<Block> {
        let hash: [u8; 32] = hex::decode(block_hash).ok()?.try_into().ok()?;
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        let engine = guard.storage.engine();
        let height_bytes = engine
            .get(Cf::Blocks, &block_hash_index_key(&hash))
            .ok()
            .flatten()?;
        let height = codec::decode_u64(&height_bytes).ok()?;
        let block_bytes = engine
            .get(Cf::Blocks, &block_by_height_key(height))
            .ok()
            .flatten()?;
        let block = codec::decode_block(&block_bytes).ok()?;
        (block.hash == block_hash).then_some(block)
    }

    /// S3-P09: O(1) присутствие блока в канонической цепочке — по
    /// `hash → height` индексу (без чтения тела блока); scan удалён.
    pub fn chain_contains_block(&self, block_hash: &str) -> bool {
        let Some(hash): Option<[u8; 32]> = hex::decode(block_hash)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
        else {
            return false;
        };
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        guard
            .storage
            .engine()
            .get(Cf::Blocks, &block_hash_index_key(&hash))
            .ok()
            .flatten()
            .is_some()
    }

    pub fn mempool_transactions(&self) -> Vec<Transaction> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .mempool
            .transactions()
    }

    pub fn apply_tx(&self, transaction: Transaction) -> Result<(), StrangecoinError> {
        let outcome = self
            .inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .add_transaction(transaction)?;
        if let crate::mempool::InsertOutcome::Replaced(evicted) = outcome {
            for evicted_id in evicted {
                self.event_bus
                    .publish(crate::events::NodeEvent::TxRejected {
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
        self.inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .allow_grant_blocks = allow;
    }

    /// SCIP-0002 / BUG-S1-002: legacy regtest opt-in for zero `state_root`
    /// blocks. Never enabled on mainnet/testnet (`Config::validate`).
    pub fn set_allow_zero_state_root(&self, allow: bool) {
        self.inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .allow_zero_state_root = allow;
    }

    // -------------------------------------------------------------- operations

    pub fn mine_block(
        &self,
        progress_tx: mpsc::Sender<String>,
        shutdown: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Option<Block> {
        self.inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .mine_block(progress_tx, shutdown)
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
        self.inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .grant_initial_balance_to_first_wallet(wallet_address)
    }

    pub fn calculate_hash(&self, block: &Block) -> String {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .calculate_hash(block)
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

// ------------------------------------------- network view (S2-P06)
//
// Cycle break (ARCHITECT3 §17): `strangecoin-net` must not depend on the
// monolith, so the SyncEngine talks to `strangecoin_net::BlockchainView`
// instead of this concrete facade
// (crates/strangecoin-net/src/blockchain_view.rs). Inherent methods win
// over trait methods in resolution, so `self.foo()` below delegates to the
// inherent impl of the same name.

#[cfg(feature = "net")]
impl strangecoin_net::BlockchainView for BlockchainFacade {
    fn tip_hash(&self) -> String {
        self.tip_hash()
    }

    fn chain_len(&self) -> usize {
        self.chain_len()
    }

    fn adopt_candidate(
        &self,
        candidate_chain: Vec<Block>,
        candidate_balances: Option<HashMap<String, AccountState>>,
        candidate_mempool_txs: Vec<Transaction>,
        candidate_difficulty: u32,
    ) -> Result<bool, String> {
        self.adopt_candidate(
            candidate_chain,
            candidate_balances,
            candidate_mempool_txs,
            candidate_difficulty,
        )
        .map_err(|e| e.to_string())
    }

    fn apply_tx(&self, transaction: Transaction) -> Result<(), String> {
        self.apply_tx(transaction).map_err(|e| e.to_string())
    }

    fn add_block(&self, block: Block) -> Result<(), String> {
        self.add_block(block).map_err(|e| e.to_string())
    }

    fn chain_contains_tx(&self, tx: &Transaction) -> bool {
        self.chain_contains_tx(tx)
    }

    fn mempool_contains(&self, txid: &[u8; 32]) -> bool {
        self.mempool_contains(txid)
    }

    fn mempool_tx(&self, txid: &[u8; 32]) -> Option<Transaction> {
        self.mempool_tx(txid)
    }

    fn mempool_txs(&self) -> Vec<Transaction> {
        self.mempool_transactions()
    }

    fn block_by_hash(&self, block_hash: &str) -> Option<Block> {
        self.block_by_hash(block_hash)
    }

    fn save_state(&self) {
        self.save_state();
    }

    fn chain_contains_block(&self, block_hash: &str) -> bool {
        self.chain_contains_block(block_hash)
    }

    fn snapshot_wire(&self) -> ChainSnapshot {
        self.snapshot_wire()
    }

    fn headers_from_height(&self, from_height: u64, max: usize) -> Vec<BlockHeader> {
        self.headers_from_height(from_height, max)
    }

    fn blocks_by_hashes(&self, hashes: &[[u8; 32]], max: usize) -> Vec<Block> {
        self.blocks_by_hashes(hashes, max)
    }
}

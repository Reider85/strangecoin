//! # State cache (ARCHITECT3 §3.4, component 3)
//!
//! The single place where account balances and nonces are read, plus the
//! chain persistence helpers and DB-open migrations that moved out of the
//! facade in S1.5-P04.
//!
//! Invariant #1 («вся валидность — из цепочки»): when the cached accounts and
//! a reconstruction from the chain disagree, the reconstruction wins.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use serde::Serialize;
use strangecoin_core::state::{unapply_block, State};
use strangecoin_core::types::{Block, Transaction};
use strangecoin_storage::{Cf, StorageEngine, WriteBatchOp};
use tracing::{debug, info};

use crate::blockchain::block_executor::{self, BlockView};
use crate::blockchain::blockchain_facade::Blockchain;
use crate::error::StrangecoinError;
use crate::storage::{codec, schema::*};
use crate::AccountState;

/// `Block::hash` (hex blake3) → the 32-byte form the S3-P09 index keys
/// take. Fail-closed: a non-hex/non-32-byte hash is corruption, never a
/// silently skipped index entry.
fn hash_bytes(hash: &str) -> Result<[u8; 32], StrangecoinError> {
    let decoded: Vec<u8> = hex::decode(hash)
        .map_err(|_| strangecoin_storage::StorageError::Corrupt("block hash is not hex"))?;
    Ok(decoded
        .try_into()
        .map_err(|_| strangecoin_storage::StorageError::Corrupt("block hash wrong length"))?)
}

/// Balances/nonces cache plus per-contract storage (S1.5-P15).
///
/// `receipts` and `State::storage_undo` are derived data of the tip block
/// and are not retained here — reorg goes through
/// [`StateCache::rebuild_from_chain`], which replays the chain (and its
/// contract storage) through the VM.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateCache {
    accounts: HashMap<String, AccountState>,
    contract_storage: strangecoin_core::state::ContractStorageMap,
}

impl StateCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_accounts(accounts: HashMap<String, AccountState>) -> Self {
        Self {
            accounts,
            contract_storage: strangecoin_core::state::ContractStorageMap::new(),
        }
    }

    pub fn from_state(state: &State) -> Self {
        Self {
            accounts: state.balances.clone(),
            // S1.5-FBP06: `State.contract_storage` is a shared handle —
            // snapshot the contents into the cache's own map (never share
            // the Arc with a live `State`).
            contract_storage: state
                .contract_storage
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }

    pub fn to_state(&self) -> State {
        State {
            balances: self.accounts.clone(),
            // S1.5-FBP06: fresh Arc over a deep copy — the cache's map
            // must not alias the returned `State` (a failed mid-block
            // apply would otherwise mutate the committed snapshot).
            contract_storage: std::sync::Arc::new(std::sync::RwLock::new(
                self.contract_storage.clone(),
            )),
            receipts: Vec::new(),
            storage_undo: HashMap::new(),
            value_transfers_undo: HashMap::new(),
        }
    }

    pub fn set_contract_storage(
        &mut self,
        contract_storage: strangecoin_core::state::ContractStorageMap,
    ) {
        self.contract_storage = contract_storage;
    }

    pub fn contract_storage(&self) -> &strangecoin_core::state::ContractStorageMap {
        &self.contract_storage
    }

    // ---------------------------------------------------------------- reads

    pub fn get(&self, address: &str) -> Option<AccountState> {
        self.accounts.get(address).cloned()
    }

    pub fn balance(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.balance())
            .unwrap_or(0)
    }

    pub fn nonce(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.nonce())
            .unwrap_or(0)
    }

    pub fn contains_key(&self, address: &str) -> bool {
        self.accounts.contains_key(address)
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.accounts.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &AccountState> {
        self.accounts.values()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &AccountState)> {
        self.accounts.iter()
    }

    pub fn accounts(&self) -> &HashMap<String, AccountState> {
        &self.accounts
    }

    pub fn total_supply(&self) -> u64 {
        self.accounts
            .values()
            .map(|account| account.balance())
            .sum()
    }

    pub fn nonzero_balances(&self) -> HashMap<String, u64> {
        self.accounts
            .iter()
            .filter(|(_, account)| account.balance() != 0)
            .map(|(address, account)| (address.clone(), account.balance()))
            .collect()
    }

    // --------------------------------------------------------------- writes

    pub fn commit(&mut self, state: State) {
        self.accounts = state.balances;
        // S1.5-FBP06: unwrap the shared handle — after `apply`/`unapply`
        // returns, the `State` is the sole owner (typical path: O(1) move,
        // no copy). Fall back to a content clone if another handle is
        // still alive (defensive; should not happen in production paths).
        self.contract_storage = std::sync::Arc::try_unwrap(state.contract_storage)
            .map(|storage| storage.into_inner().unwrap_or_else(|p| p.into_inner()))
            .unwrap_or_else(|shared| {
                shared
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            });
    }

    pub fn replace(&mut self, other: StateCache) {
        self.accounts = other.accounts;
        self.contract_storage = other.contract_storage;
    }

    pub fn invalidate(&mut self) {
        self.accounts.clear();
    }

    pub fn unapply_block(&mut self, block: &Block, chain_id: u32) -> Result<(), StrangecoinError> {
        let rolled_back = unapply_block(&self.to_state(), block, chain_id)?;
        self.commit(rolled_back);
        Ok(())
    }

    pub fn credit(&mut self, address: &str, amount: u64) {
        let account = self.accounts.entry(address.to_string()).or_default();
        account.set_balance(account.balance().saturating_add(amount));
    }

    pub fn ensure_account(&mut self, address: &str) {
        self.accounts.entry(address.to_string()).or_default();
    }

    pub fn rebuild_from_chain(
        chain: &[Block],
        now: u64,
        allow_grant_blocks: bool,
        allow_zero_state_root: bool,
        rules: &super::consensus_manager::ConsensusManager,
        chain_id: u32,
    ) -> Result<(Self, Vec<strangecoin_core::vm::Receipt>), StrangecoinError> {
        let outcome = Self::rebuild_impl(
            chain,
            now,
            allow_grant_blocks,
            allow_zero_state_root,
            rules,
            chain_id,
            false,
        )?;
        Ok((outcome.cache, outcome.receipts))
    }

    /// [`Self::rebuild_from_chain`] **plus** per-height pre-apply account
    /// snapshots (S3-P07 `state_history`). Adoption is the only recorder:
    /// validation/rebuild paths (`validate_chain`, `rebuild_state_cache`)
    /// never persist, so they pay nothing for the extra bookkeeping.
    pub(crate) fn rebuild_from_chain_recorded(
        chain: &[Block],
        now: u64,
        allow_grant_blocks: bool,
        allow_zero_state_root: bool,
        rules: &super::consensus_manager::ConsensusManager,
        chain_id: u32,
    ) -> Result<RebuildOutcome, StrangecoinError> {
        Self::rebuild_impl(
            chain,
            now,
            allow_grant_blocks,
            allow_zero_state_root,
            rules,
            chain_id,
            true,
        )
    }

    fn rebuild_impl(
        chain: &[Block],
        now: u64,
        allow_grant_blocks: bool,
        allow_zero_state_root: bool,
        rules: &super::consensus_manager::ConsensusManager,
        chain_id: u32,
        record_history: bool,
    ) -> Result<RebuildOutcome, StrangecoinError> {
        let mut state = State::new();
        let mut codes: HashMap<String, Vec<u8>> = HashMap::new();
        let mut history: Vec<PendingHistory> = Vec::new();
        for (height, block) in chain.iter().enumerate() {
            let view = BlockView::new(
                &chain[..height],
                now,
                allow_grant_blocks,
                rules.expected_version(height as u64),
                chain_id,
            )
            .with_phase(rules.phase_at(height as u64))
            .with_allow_zero_state_root(allow_zero_state_root)
            .with_codes(&codes);
            if record_history {
                // Pre-apply values: `state` still holds the parent block's
                // post-state at this point of the replay.
                let pre = capture_pre_state(&state.balances, block);
                if !pre.is_empty() {
                    history.push(PendingHistory {
                        height: height as u64,
                        accounts: pre,
                    });
                }
            }
            state = block_executor::validate_and_apply(&state, block, &view).map_err(|e| {
                tracing::warn!(block_index = height, error = %e, "Rebuild stopped at an invalid block");
                e
            })?;
            block_executor::collect_block_codes(block, chain_id, &mut codes);
        }
        let receipts = state.receipts.clone();
        Ok(RebuildOutcome {
            cache: Self::from_state(&state),
            receipts,
            history,
        })
    }
}

impl Serialize for StateCache {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.accounts.serialize(serializer)
    }
}

impl<'a> IntoIterator for &'a StateCache {
    type Item = (&'a String, &'a AccountState);
    type IntoIter = std::collections::hash_map::Iter<'a, String, AccountState>;

    fn into_iter(self) -> Self::IntoIter {
        self.accounts.iter()
    }
}

// ---------------------------------------------------------------------------
// Storage paths + persistence (S1.5-P04; migrated to strangecoin-storage
// in S3-P07: per-block/per-account schema, incremental saves, mempool CF)
// ---------------------------------------------------------------------------

/// Persisted state read from storage on open (both backends).
pub(crate) struct PersistedState {
    pub chain: Option<Vec<Block>>,
    pub balances: Option<HashMap<String, AccountState>>,
    pub contract_storage: Option<strangecoin_core::state::ContractStorageMap>,
    pub difficulty: Option<u32>,
    pub mempool_txs: Vec<Transaction>,
}

/// Pre-apply snapshot of the accounts one block touches (S3-P07,
/// ADR-0024 `state_history` CF, ADR-0025 pruning substrate): the account
/// values **before** the block was applied, keyed by the block's height
/// once written (`height: u64 BE ‖ address`).
///
/// Captured at every apply site (`commit_block`, grant blocks, genesis)
/// and at every adoption (per-height, from the recorded rebuild); only
/// accounts that *exist* pre-apply are recorded — an absent account's
/// pre-state is the zero account, which a `state` lookup already yields.
#[derive(Clone, Debug)]
pub(crate) struct PendingHistory {
    pub height: u64,
    pub accounts: Vec<(String, AccountState)>,
}

/// Result of a recorded chain rebuild (S3-P07): the rebuilt cache, the
/// tip receipts, and the per-height pre-apply snapshots the adoption path
/// hands to `save_state` (`state_history` CF).
pub(crate) struct RebuildOutcome {
    pub cache: StateCache,
    pub receipts: Vec<strangecoin_core::vm::Receipt>,
    pub history: Vec<PendingHistory>,
}

/// Accounts `block` will touch, as they are **now** (pre-apply).
///
/// Affected set = senders + receivers of the block's transactions
/// (coinbase `from` magic strings have no account and drop out via the
/// `balances.get` filter; `Create` contract addresses come back from
/// `Transaction::receiver`). Deduplicated in first-seen order.
pub(crate) fn capture_pre_state(
    balances: &HashMap<String, AccountState>,
    block: &Block,
) -> Vec<(String, AccountState)> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<(String, AccountState)> = Vec::new();
    for tx in &block.transactions {
        for addr in std::iter::once(tx.sender()).chain(tx.receiver()) {
            if seen.contains(&addr) {
                continue;
            }
            seen.push(addr);
            if let Some(account) = balances.get(addr) {
                out.push((addr.to_string(), account.clone()));
            }
        }
    }
    out
}

/// JSON wire form of [`strangecoin_core::state::ContractStorageMap`]
/// (S1.5-P15): serde_json cannot use `[u8; 32]` map keys, so slots are
/// stored as sorted `(hex_key, value)` pairs per address; the sorted order
/// keeps the persisted bytes deterministic. Written by the Stage 0-2 blob
/// schema, read by the S3-P08 migration (`contract_storage_from_json`
/// below); nothing on the per-block path emits it anymore. Public since
/// S3-P08: the legacy fixture seeder (`test_support::LegacySeed`) builds
/// this shape for `tests/leveldb_migration.rs`.
pub type ContractStorageJson = Vec<(String, Vec<(String, Vec<u8>)>)>;

pub(crate) fn contract_storage_from_json(
    raw: &ContractStorageJson,
) -> Option<strangecoin_core::state::ContractStorageMap> {
    let mut map = strangecoin_core::state::ContractStorageMap::new();
    for (address, slots) in raw {
        let mut storage = strangecoin_core::state::ContractStorage::new();
        for (hex_key, value) in slots {
            let bytes = hex::decode(hex_key).ok()?;
            let key: [u8; 32] = bytes.try_into().ok()?;
            storage.insert(key, value.clone());
        }
        map.insert(address.clone(), storage);
    }
    Some(map)
}

pub(crate) fn db_path_for_port(port: u16) -> PathBuf {
    let exe_path =
        std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
    let exe_dir = exe_path
        .parent()
        .expect("Не удалось получить директорию исполняемого файла");
    exe_dir.join(format!("blockchain_db_{}", port))
}

/// Open or create storage for `port` per `storage_cfg`, run the S3-P08
/// LevelDB auto-migration when the historical directory is present, load
/// persisted state, install genesis when the DB is empty, run the
/// open-path migrations, persist. (S1.5-P04: moved from facade; S3-P07:
/// engine selection + per-block schema; S3-P08: one-shot LevelDB →
/// RocksDB.)
///
/// Path rule (ADR-0024 "Config surface"): `RocksDbEngine` at
/// `storage.path`; the historical `<exe_dir>/blockchain_db_{port}`
/// directory (`legacy_path`) is never opened as a backend anymore — on
/// first open it is converted into `storage.path` and kept as
/// `.leveldb.bak` (ADR-0008 rollback capability).
///
/// Fail-closed: a migration error (or a legacy directory present in a
/// build without the reader) panics here, before the RocksDB open — the
/// node never starts over unconverted or divergent data, and the legacy
/// directory is left untouched.
pub(crate) fn open_blockchain(
    port: u16,
    chain_id: u32,
    storage_cfg: &crate::config::StorageConfig,
) -> Blockchain {
    use super::consensus_manager::ConsensusManager;
    use tracing::info;

    let start_time = std::time::SystemTime::now();
    let legacy_path = db_path_for_port(port);
    #[cfg(feature = "legacy-migrate")]
    if let Err(err) =
        crate::storage::migrations::auto_migrate_legacy(&legacy_path, &storage_cfg.path)
    {
        panic!(
            "LevelDB -> RocksDB migration failed: {err} (legacy directory left untouched at {})",
            legacy_path.display()
        );
    }
    #[cfg(not(feature = "legacy-migrate"))]
    if legacy_path.exists() {
        panic!(
            "legacy LevelDB database found at {} but this build lacks the `legacy-migrate` \
             feature; rebuild with `--features legacy-migrate` to convert it \
             (refusing to start over unconverted data)",
            legacy_path.display()
        );
    }
    if !storage_cfg.path.exists() {
        info!("База данных не существует, создаётся новая");
    }
    let storage = crate::storage::Storage::open_rocks(&storage_cfg.path)
        .expect("Не удалось открыть хранилище");

    let mut blockchain = Blockchain {
        chain: vec![],
        balances: StateCache::new(),
        difficulty: 1,
        mempool: crate::mempool::Mempool::new(chain_id),
        storage,
        allow_grant_blocks: false,
        allow_zero_state_root: false,
        total_work: [0, 0, 0, 0],
        rules: ConsensusManager::new(),
        chain_id,
        codes: HashMap::new(),
        receipts: Vec::new(),
        persisted_accounts: HashMap::new(),
        persisted_contract_storage: strangecoin_core::state::ContractStorageMap::new(),
        persisted_mempool: HashMap::new(),
        pending_state_history: Vec::new(),
    };
    blockchain.storage_health_check();

    let loaded = load_persisted_state(&blockchain.storage).expect("persisted state must load");
    // Persisted snapshots = what the DB actually holds. Set BEFORE the
    // open-path migrations so the final `save_state` diffs only what the
    // migrations (or genesis install) changed — restarts rewrite nothing.
    blockchain.persisted_accounts = loaded.balances.clone().unwrap_or_default();
    blockchain.persisted_contract_storage = loaded.contract_storage.clone().unwrap_or_default();
    blockchain.persisted_mempool = loaded
        .mempool_txs
        .iter()
        .map(|tx| {
            (
                (tx.sender().to_string(), tx.nonce()),
                strangecoin_core::serialize::txid(tx),
            )
        })
        .collect();
    // Accounts before the chain branch: on a fresh DB `balances` is
    // `None` (helper no-ops, the genesis install below owns the initial
    // state); on a loaded DB genesis never installs. Whole-field
    // replacement goes through this helper — the balances gate confines
    // it to this file (BUG-S0-022).
    apply_loaded_accounts(&mut blockchain, &loaded);

    if let Some(chain) = loaded.chain {
        if !chain.is_empty() {
            if let Err(e) = crate::consensus::validate_genesis(&chain[0], chain_id) {
                panic!("Genesis validation failed: {}", e);
            }
        }
        blockchain.chain = chain;
    } else {
        Blockchain::install_fresh_genesis(&mut blockchain);
    }

    blockchain.total_work = strangecoin_core::consensus::cumulative_work(&blockchain.chain);
    // Restore the mempool against the *loaded* sender accounts (the old
    // `AccountState::default()` made `Mempool::insert` reject every funded
    // tx on reload — nonce/balance validation needs the real account).
    for tx in loaded.mempool_txs {
        let account = blockchain.balances.get(tx.sender()).unwrap_or_default();
        if let Err(e) = blockchain.mempool.insert(tx, &account) {
            debug!(error = %e, "Пропущена восстановленная mempool-транзакция");
        }
    }
    blockchain.migrate_initial_wallet_balance();
    blockchain.migrate_addresses_to_bech32();
    blockchain.codes = block_executor::codes_from_chain(&blockchain.chain, chain_id);
    blockchain.save_state();
    blockchain.storage_health_check();
    let duration = start_time.elapsed().map(|d| d.as_secs_f64()).unwrap_or(0.0);
    info!(duration_secs = duration, "Создание блокчейна завершено");
    blockchain
}

/// Read chain/balances/difficulty and mempool txs from storage.
///
/// Per-block schema read (S3-P07/S3-P08 — the transitional blob read is
/// gone with the LevelDB engine): fail-closed per-block read — meta tip
/// cross-checked against a single sorted pass over `blocks` (contiguous
/// `0..=tip`, no hash-index keys), `state` full-load, `contract_storage`
/// per-slot (address = `key[..len-32]`), mempool by prefix-iterate (no
/// full-DB scan, ascending nonce per sender — ADR-0024). Corruption →
/// `StorageError::Corrupt`, never a silent partial load.
pub(crate) fn load_persisted_state(
    storage: &crate::storage::Storage,
) -> Result<PersistedState, StrangecoinError> {
    load_persisted_state_engine(storage.engine().as_ref())
}

/// Apply a load's account state onto `blockchain`: whole-field
/// `balances` replacement is confined to this file by the balances gate
/// (BUG-S0-022 / `tests/balances_gate.rs`), so both S3-P07 open paths
/// (production `open_blockchain`, standalone `open_local`) funnel the
/// loaded accounts through here. One-shot at open — the `clone`s are
/// outside any hot path.
pub(crate) fn apply_loaded_accounts(blockchain: &mut Blockchain, loaded: &PersistedState) {
    if let Some(balances) = &loaded.balances {
        blockchain.balances = StateCache::from_accounts(balances.clone());
    }
    if let Some(contract_storage) = &loaded.contract_storage {
        blockchain
            .balances
            .set_contract_storage(contract_storage.clone());
    }
    if let Some(difficulty) = loaded.difficulty {
        blockchain.difficulty = difficulty;
    }
}

/// Per-block schema read (ADR-0024). See `load_persisted_state` docs.
///
/// **Tip-less databases** (S3-P08): a fresh database *or* the result of
/// migrating a legacy database that carried state but no `chain` blob.
/// Stray *blocks* without their tip remain impossible (atomic-batch
/// contract) and stay corrupt; stray *state* is legitimate on this path
/// and is loaded below — `None` (the "replace nothing" marker consumed
/// before the genesis install) is returned only when the state CF is
/// actually empty, byte-compatible with the pre-S3-P08 fresh behaviour.
fn load_persisted_state_engine(
    engine: &dyn StorageEngine,
) -> Result<PersistedState, StrangecoinError> {
    let tip_height: Option<u64> = match engine.get(Cf::Meta, &meta_key(META_TIP_HEIGHT))? {
        None => {
            let stray = engine.iterate_prefix(Cf::Blocks, b"")?;
            if stray
                .iter()
                .any(|(k, _)| decode_block_height_key(k).is_some())
            {
                return Err(strangecoin_storage::StorageError::Corrupt(
                    "blocks present without tip meta",
                )
                .into());
            }
            None
        }
        Some(bytes) => Some(codec::decode_u64(&bytes)?),
    };

    // ---- chain: only with a tip (the migration writes tip meta exactly
    //      when a chain exists) ----
    let chain: Option<Vec<Block>> = match tip_height {
        None => None,
        Some(tip_height) => {
            let tip_hash = engine.get(Cf::Meta, &meta_key(META_TIP_HASH))?.ok_or(
                strangecoin_storage::StorageError::Corrupt("tip_hash missing"),
            )?;
            let tip_hash = codec::decode_hash(&tip_hash)?;

            // One sorted pass over `blocks`; BE height keys come back
            // ascending, so contiguity == no gaps. Tagged hash-index keys
            // (S3-P09, 33 B) do not decode as heights and are skipped.
            let mut chain: Vec<Block> = Vec::new();
            let mut expected = 0u64;
            for (key, value) in engine.iterate_prefix(Cf::Blocks, b"")? {
                let Some(height) = decode_block_height_key(&key) else {
                    continue;
                };
                if height != expected {
                    return Err(
                        strangecoin_storage::StorageError::Corrupt("missing block height").into(),
                    );
                }
                let block = codec::decode_block(&value)?;
                if block.index != height {
                    return Err(strangecoin_storage::StorageError::Corrupt(
                        "block index does not match key",
                    )
                    .into());
                }
                chain.push(block);
                expected += 1;
            }
            if chain.len() as u64 != tip_height + 1 {
                return Err(
                    strangecoin_storage::StorageError::Corrupt("chain shorter than tip").into(),
                );
            }
            if chain.last().map(|b| b.hash.as_str()) != Some(tip_hash.as_str()) {
                return Err(strangecoin_storage::StorageError::Corrupt("tip hash mismatch").into());
            }
            Some(chain)
        }
    };

    let mut balances = HashMap::new();
    for (key, value) in engine.iterate_prefix(Cf::State, b"")? {
        let address = String::from_utf8(key)
            .map_err(|_| strangecoin_storage::StorageError::Corrupt("state key not utf-8"))?;
        balances.insert(address, codec::decode_account(&value)?);
    }

    // `address ‖ slot[32]`: slots are fixed-width (ADR-0024), so the
    // address/slot boundary is the last 32 bytes — no framing needed.
    let mut contract_storage = strangecoin_core::state::ContractStorageMap::new();
    for (key, value) in engine.iterate_prefix(Cf::ContractStorage, b"")? {
        // S3-P09: the address→storage_root cache is a tagged sub-space
        // of this CF — classify by first byte BEFORE the `address ‖ slot`
        // split (its key would otherwise be misparsed as a slot record,
        // or trip the length check on short addresses → false Corrupt).
        if key.first() == Some(&KeyPrefix::ContractStorageRoot.byte()) {
            continue;
        }
        if key.len() < 32 {
            return Err(strangecoin_storage::StorageError::Corrupt(
                "contract storage key too short",
            )
            .into());
        }
        let (address_bytes, slot_bytes) = key.split_at(key.len() - 32);
        let address = String::from_utf8(address_bytes.to_vec())
            .map_err(|_| strangecoin_storage::StorageError::Corrupt("contract key not utf-8"))?;
        let slot: [u8; 32] = slot_bytes
            .try_into()
            .map_err(|_| strangecoin_storage::StorageError::Corrupt("contract slot width"))?;
        contract_storage
            .entry(address)
            .or_default()
            .insert(slot, value);
    }

    let difficulty = engine
        .get(Cf::Meta, &meta_key(META_DIFFICULTY))?
        .map(|bytes| codec::decode_u32(&bytes))
        .transpose()?;

    // Empty prefix = the whole mempool CF (prefix-iterate, NOT the Stage 1
    // full-DB scan): per sender, BE nonces ascend (ADR-0024).
    let mut mempool_txs = Vec::new();
    for (_key, value) in engine.iterate_prefix(Cf::Mempool, b"")? {
        mempool_txs.push(codec::decode_tx(&value)?);
    }

    Ok(PersistedState {
        chain,
        // Tip-less: `None` only when there is nothing to restore — the
        // open path treats `Some` as "replace the state the genesis
        // install starts from" (chain-less migrated legacy DBs land here).
        balances: if tip_height.is_some() || !balances.is_empty() {
            Some(balances)
        } else {
            None
        },
        contract_storage: if tip_height.is_some() || !contract_storage.is_empty() {
            Some(contract_storage)
        } else {
            None
        },
        difficulty,
        mempool_txs,
    })
}

impl Blockchain {
    /// Storage health check (S3-P07 — replaces the LevelDB key dump):
    /// schema version + tip presence, cheap (two point gets), safe to
    /// call at open and after every save.
    pub(crate) fn storage_health_check(&self) {
        let engine = self.storage.engine();
        let schema_version = engine
            .get(Cf::Meta, &meta_key(META_SCHEMA_VERSION))
            .ok()
            .flatten()
            .and_then(|bytes| decode_schema_version(&bytes));
        let tip_height = engine
            .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))
            .ok()
            .flatten()
            .and_then(|bytes| codec::decode_u64(&bytes).ok());
        debug!(
            schema_version = ?schema_version,
            tip_height = ?tip_height,
            "storage health"
        );
    }

    /// Validate a transaction against current state and queue it in the mempool.
    pub(crate) fn add_transaction(
        &mut self,
        transaction: Transaction,
    ) -> Result<crate::mempool::InsertOutcome, StrangecoinError> {
        if transaction.sender().is_empty() || transaction.receiver().is_some_and(|r| r.is_empty()) {
            tracing::warn!("Пустой адрес отправителя или получателя");
            return Err(StrangecoinError::SizeLimitExceeded("empty address"));
        }
        let tx_size = serde_json::to_vec(&transaction)
            .map_err(StrangecoinError::SerializationError)?
            .len();
        if tx_size > crate::network::protocol::MAX_TX_SIZE {
            tracing::warn!(
                tx_size,
                limit = crate::network::protocol::MAX_TX_SIZE,
                "TX size limit exceeded"
            );
            return Err(StrangecoinError::SizeLimitExceeded("transaction"));
        }
        let account_state = self.balances.get(transaction.sender()).unwrap_or_default();
        let outcome = self.mempool.insert(transaction, &account_state)?;
        info!(
            pending_count = self.mempool.len(),
            "Транзакция добавлена в mempool"
        );
        Ok(outcome)
    }

    /// Validate the stored chain against a fresh reconstruction (invariant #1).
    pub(crate) fn validate_chain(&self) -> bool {
        if self.chain.is_empty() {
            tracing::warn!("Цепочка пуста, невалидна");
            return false;
        }
        for block in &self.chain {
            let block_size = strangecoin_core::serialize::serialize_block(block).len();
            if block_size > crate::network::protocol::MAX_BLOCK_SIZE {
                tracing::warn!(
                    block_index = block.index,
                    block_size,
                    limit = crate::network::protocol::MAX_BLOCK_SIZE,
                    "Block size limit exceeded"
                );
                return false;
            }
        }
        let reconstructed = match StateCache::rebuild_from_chain(
            &self.chain,
            block_executor::now_secs(),
            self.allow_grant_blocks,
            self.allow_zero_state_root,
            &self.rules,
            self.chain_id,
        ) {
            Ok((cache, _receipts)) => cache,
            Err(e) => {
                tracing::warn!(error = %e, "Цепочка отклонена: блок не прошёл валидацию");
                return false;
            }
        };
        let mut temp_state = reconstructed.to_state();
        for tx in self.mempool.transactions() {
            let required = tx.value() + tx.fee();
            let sender_balance = temp_state.get_balance(tx.sender());
            if sender_balance < required {
                tracing::warn!(
                    sender = %tx.sender(),
                    nonce = tx.nonce(),
                    required,
                    available = sender_balance,
                    "Недостаточно средств в mempool"
                );
                return false;
            }
            temp_state.set_balance(tx.sender(), sender_balance.saturating_sub(required));
            if let Some(receiver) = tx.receiver() {
                let receiver_balance = temp_state.get_balance(receiver);
                temp_state.set_balance(receiver, receiver_balance.saturating_add(tx.value()));
            }
        }
        let rebuilt = reconstructed.nonzero_balances();
        let stored = self.balances.nonzero_balances();
        if rebuilt != stored {
            tracing::warn!("Восстановленные балансы не совпадают с хранимыми");
            return false;
        }
        info!("Валидация цепочки завершена");
        true
    }

    pub(crate) fn rebuild_state_cache(&mut self) -> Result<(), StrangecoinError> {
        let now = block_executor::now_secs();
        let (balances, receipts) = StateCache::rebuild_from_chain(
            &self.chain,
            now,
            self.allow_grant_blocks,
            self.allow_zero_state_root,
            &self.rules,
            self.chain_id,
        )?;
        self.balances = balances;
        self.receipts = receipts;
        self.codes = block_executor::codes_from_chain(&self.chain, self.chain_id);
        Ok(())
    }

    /// Persist current state (S3-P07, ADR-0024 §Decision "Write path"):
    /// ONE atomic `write_batch` covering **O(changed records)** — blocks
    /// only from the common ancestor with the persisted tip (reorg tail
    /// deleted), account/contract/mempool diffs against the persisted
    /// snapshots, pre-apply `state_history` for the heights written here,
    /// meta tip. Never a full-chain or full-state rewrite. (The Stage 2
    /// blob rewrite died with the LevelDB engine in S3-P08.)
    pub(crate) fn save_state(&mut self) {
        let storage = self.storage.clone();
        self.save_state_engine(storage.engine().as_ref())
            .expect("Ошибка сохранения состояния");
        self.storage_health_check();
    }

    /// Per-block incremental write (ADR-0024). See [`Self::save_state`].
    ///
    /// S3-P09: besides blocks/state/mempool, every save keeps the
    /// O(1) index sub-spaces consistent — `hash → height`,
    /// `txid → (height, tx_index)` + raw tx, and the contract
    /// `address → storage_root` cache — across both the extension
    /// (puts) and the reorg (delete-then-put) directions.
    fn save_state_engine(&mut self, engine: &dyn StorageEngine) -> Result<(), StrangecoinError> {
        let mut ops: Vec<WriteBatchOp> = Vec::new();
        let chain_len = self.chain.len() as u64;

        // ---- 1. blocks + state_history + meta tip (height-scoped) ----
        let persisted_tip: Option<u64> = engine
            .get(Cf::Meta, &meta_key(META_TIP_HEIGHT))?
            .map(|bytes| codec::decode_u64(&bytes))
            .transpose()?;
        let persisted_tip_hash: Option<String> = engine
            .get(Cf::Meta, &meta_key(META_TIP_HASH))?
            .map(|bytes| codec::decode_hash(&bytes))
            .transpose()?;

        // Common ancestor with the persisted chain: O(1) in the common
        // extension case (meta tip hash vs the in-memory block), walking
        // down with point-gets only across an actual reorg. A corrupt
        // stored block reads as "mismatch" — the rewrite below self-heals.
        let common: Option<u64> = match persisted_tip {
            None => None,
            Some(_) if chain_len == 0 => None,
            Some(pt) => {
                let start = pt.min(chain_len - 1);
                let mut height = start;
                loop {
                    let memory_hash = self.chain[height as usize].hash.as_str();
                    let matches = if height == pt {
                        persisted_tip_hash.as_deref() == Some(memory_hash)
                    } else {
                        match engine.get(Cf::Blocks, &block_by_height_key(height))? {
                            Some(bytes) => codec::decode_block(&bytes)
                                .map(|stored| stored.hash == memory_hash)
                                .unwrap_or(false),
                            None => false,
                        }
                    };
                    if matches {
                        break Some(height);
                    }
                    if height == 0 {
                        break None;
                    }
                    height -= 1;
                }
            }
        };

        let new_heights: Vec<u64> = match common {
            Some(c) => ((c + 1)..chain_len).collect(),
            None => (0..chain_len).collect(),
        };
        // Persisted heights above the new chain end (reorg to a shorter
        // chain) — always above `common`, never rewritten by the puts.
        let drop_heights: Vec<u64> = match persisted_tip {
            Some(pt) if pt >= chain_len => (chain_len..=pt).collect(),
            _ => Vec::new(),
        };

        // ---- S3-P09: index maintenance (hash→height, txid→location +
        //      raw tx, contract storage-root cache) ----
        //
        // Ordering contract: index DELETES for the superseded fork (Phase
        // A) are emitted BEFORE the new-tail PUTS, so a tx re-mined by
        // the new chain always ends the batch re-indexed (put wins), and
        // an old-fork txid the new tail carries is skipped by the delete
        // outright. The remaining duplicate-txid case — the txid also
        // lives in the KEPT prefix (identical coinbases) — is re-pointed
        // at its last kept occurrence instead of being deleted (Phase A
        // details below). Convention: one entry per txid = the highest
        // occurrence in the current chain.
        let new_txids: HashSet<[u8; 32]> = new_heights
            .iter()
            .flat_map(|&h| self.chain[h as usize].transactions.iter())
            .map(strangecoin_core::serialize::txid)
            .collect();

        // Addresses whose storage-root cache entry moves this save.
        // BTreeSet ⇒ deterministic op order (Stage 3 checklist #3/#5).
        let mut root_touch: BTreeSet<String> = BTreeSet::new();

        // A database written before S3-P09 (legacy dev DB, one-shot
        // LevelDB migration — `migrations::build_ops` writes blocks
        // without indices) carries a tip but no hash-index entry for it:
        // rebuild every index record for the whole in-memory chain in
        // this same atomic batch. Fresh DBs (no tip) take the normal
        // per-height path below; the keys are additive, so
        // `SCHEMA_VERSION` stays 1 (pre-mainnet, no schema bump).
        //
        // The probe is the **persisted** tip (the DB's last written
        // block), never the in-memory one: a block adopted from a peer
        // or side facade is legitimately unindexed until the new-height
        // puts below land and must not trigger a chain-wide rewrite.
        let tip_index_missing = match (&persisted_tip, &persisted_tip_hash) {
            (Some(_), Some(tip_hash)) => {
                let tip_hash = hash_bytes(tip_hash)?;
                engine
                    .get(Cf::Blocks, &block_hash_index_key(&tip_hash))?
                    .is_none()
            }
            _ => false,
        };
        if tip_index_missing {
            for (height, block) in self.chain.iter().enumerate() {
                let height = height as u64;
                ops.push(WriteBatchOp::Put {
                    cf: Cf::Blocks,
                    key: block_hash_index_key(&hash_bytes(&block.hash)?),
                    value: codec::encode_u64(height),
                });
                for (tx_index, tx) in block.transactions.iter().enumerate() {
                    let txid = strangecoin_core::serialize::txid(tx);
                    ops.push(WriteBatchOp::Put {
                        cf: Cf::Transactions,
                        key: tx_index_key(&txid),
                        value: encode_tx_index_value(height, tx_index as u32).to_vec(),
                    });
                    ops.push(WriteBatchOp::Put {
                        cf: Cf::Transactions,
                        key: tx_raw_key(&txid),
                        value: codec::encode_tx(tx),
                    });
                }
            }
            // Root cache for every contract the chain defines (the
            // emission after §3 refreshes the ones this save touches).
            for (address, account) in self.balances.iter() {
                if account.is_contract() {
                    root_touch.insert(address.clone());
                }
            }
        }

        // Phase A — index cleanup for the superseded fork tail: every
        // persisted height above the common ancestor that the new chain
        // does not keep byte-identically (rewritten heights AND heights
        // dropped by a shorter chain). Point-gets only; a corrupt old
        // block reads as "mismatch" and self-heals through the rewrite,
        // matching the common-ancestor walk's contract.
        //
        // Duplicate txids exist on-chain (identical coinbases: regtest
        // reward is 0 and the miner address is stable — a plateau-height
        // mainnet coinbase of one miner collides the same way). The
        // index convention is "one entry = the HIGHEST occurrence in the
        // current chain", so an old-fork txid that the *kept* prefix also
        // carries must be re-pointed at its last kept occurrence instead
        // of being deleted. Built lazily: only reorg saves pay for it.
        let mut kept_txids: Option<HashMap<[u8; 32], (u64, u32)>> = None;
        let index_drop_heights: Vec<u64> = match (common, persisted_tip) {
            (Some(c), Some(pt)) if pt > c => ((c + 1)..=pt).collect(),
            (None, Some(pt)) => (0..=pt).collect(),
            _ => Vec::new(),
        };
        for height in index_drop_heights {
            let Some(old_bytes) = engine.get(Cf::Blocks, &block_by_height_key(height))? else {
                continue;
            };
            let Ok(old_block) = codec::decode_block(&old_bytes) else {
                continue;
            };
            let superseded =
                height >= chain_len || self.chain[height as usize].hash != old_block.hash;
            if !superseded {
                continue;
            }
            ops.push(WriteBatchOp::Delete {
                cf: Cf::Blocks,
                key: block_hash_index_key(&hash_bytes(&old_block.hash)?),
            });
            for tx in &old_block.transactions {
                let txid = strangecoin_core::serialize::txid(tx);
                if new_txids.contains(&txid) {
                    continue;
                }
                // Kept-prefix duplicate: repoint at the last occurrence
                // the surviving chain still holds (raw bytes are txid-
                // determined, the raw record needs no touch).
                if let Some(c) = common {
                    let kept = kept_txids.get_or_insert_with(|| {
                        let mut map: HashMap<[u8; 32], (u64, u32)> = HashMap::new();
                        for block in self.chain.iter().take(c as usize + 1) {
                            for (index, kept_tx) in block.transactions.iter().enumerate() {
                                map.insert(
                                    strangecoin_core::serialize::txid(kept_tx),
                                    (block.index, index as u32),
                                );
                            }
                        }
                        map
                    });
                    if let Some(&(kept_height, kept_index)) = kept.get(&txid) {
                        ops.push(WriteBatchOp::Put {
                            cf: Cf::Transactions,
                            key: tx_index_key(&txid),
                            value: encode_tx_index_value(kept_height, kept_index).to_vec(),
                        });
                        continue;
                    }
                }
                ops.push(WriteBatchOp::Delete {
                    cf: Cf::Transactions,
                    key: tx_index_key(&txid),
                });
                ops.push(WriteBatchOp::Delete {
                    cf: Cf::Transactions,
                    key: tx_raw_key(&txid),
                });
            }
        }

        for &height in &new_heights {
            let block = &self.chain[height as usize];
            ops.push(WriteBatchOp::Put {
                cf: Cf::Blocks,
                key: block_by_height_key(height),
                value: codec::encode_block(block),
            });
            // S3-P09: hash→height plus per-tx location/raw for this
            // height (the raw copy is what Stage 4 `get_tx` reads).
            ops.push(WriteBatchOp::Put {
                cf: Cf::Blocks,
                key: block_hash_index_key(&hash_bytes(&block.hash)?),
                value: codec::encode_u64(height),
            });
            for (tx_index, tx) in block.transactions.iter().enumerate() {
                let txid = strangecoin_core::serialize::txid(tx);
                ops.push(WriteBatchOp::Put {
                    cf: Cf::Transactions,
                    key: tx_index_key(&txid),
                    value: encode_tx_index_value(height, tx_index as u32).to_vec(),
                });
                ops.push(WriteBatchOp::Put {
                    cf: Cf::Transactions,
                    key: tx_raw_key(&txid),
                    value: codec::encode_tx(tx),
                });
            }
            // A previous fork may have written `state_history` at this
            // height (reorg): replace, don't merge.
            for (key, _) in engine.iterate_prefix(Cf::StateHistory, &height.to_be_bytes())? {
                ops.push(WriteBatchOp::Delete {
                    cf: Cf::StateHistory,
                    key,
                });
            }
            // Pre-apply snapshot captured at commit/adoption for this height.
            for pending in self
                .pending_state_history
                .iter()
                .filter(|pending| pending.height == height)
            {
                for (address, account) in &pending.accounts {
                    ops.push(WriteBatchOp::Put {
                        cf: Cf::StateHistory,
                        key: state_history_key(height, address),
                        value: codec::encode_account(account),
                    });
                }
            }
        }
        for &height in &drop_heights {
            ops.push(WriteBatchOp::Delete {
                cf: Cf::Blocks,
                key: block_by_height_key(height),
            });
            for (key, _) in engine.iterate_prefix(Cf::StateHistory, &height.to_be_bytes())? {
                ops.push(WriteBatchOp::Delete {
                    cf: Cf::StateHistory,
                    key,
                });
            }
        }

        if chain_len > 0 {
            ops.push(WriteBatchOp::Put {
                cf: Cf::Meta,
                key: meta_key(META_TIP_HEIGHT),
                value: codec::encode_u64(chain_len - 1),
            });
            ops.push(WriteBatchOp::Put {
                cf: Cf::Meta,
                key: meta_key(META_TIP_HASH),
                value: codec::encode_hash(&self.chain[chain_len as usize - 1].hash)?,
            });
        }
        ops.push(WriteBatchOp::Put {
            cf: Cf::Meta,
            key: meta_key(META_DIFFICULTY),
            value: codec::encode_u32(self.difficulty),
        });

        // ---- 2. accounts: upsert changed, delete removed ----
        for (address, account) in self.balances.iter() {
            if self.persisted_accounts.get(address) != Some(account) {
                ops.push(WriteBatchOp::Put {
                    cf: Cf::State,
                    key: state_key(address),
                    value: codec::encode_account(account),
                });
                // S3-P09: a changed contract account moves its
                // `storage_root` (core recomputed it on the storage
                // flush) — refresh the cache entry with it.
                if account.is_contract() {
                    root_touch.insert(address.clone());
                }
            }
        }
        let removed_addresses: Vec<String> = self
            .persisted_accounts
            .keys()
            .filter(|address| !self.balances.contains_key(address.as_str()))
            .cloned()
            .collect();
        for address in &removed_addresses {
            ops.push(WriteBatchOp::Delete {
                cf: Cf::State,
                key: state_key(address),
            });
            if self
                .persisted_accounts
                .get(address)
                .is_some_and(AccountState::is_contract)
            {
                root_touch.insert(address.clone());
            }
        }

        // ---- 3. contract storage: per-slot diff ----
        for (address, slots) in self.balances.contract_storage().iter() {
            let persisted_slots = self.persisted_contract_storage.get(address);
            for (slot, value) in slots.iter() {
                let changed = persisted_slots
                    .map(|stored| stored.get(slot) != Some(value))
                    .unwrap_or(true);
                if changed {
                    ops.push(WriteBatchOp::Put {
                        cf: Cf::ContractStorage,
                        key: contract_storage_key(address, slot),
                        value: value.clone(),
                    });
                    root_touch.insert(address.clone());
                }
            }
        }
        for (address, slots) in self.persisted_contract_storage.iter() {
            let current_slots = self.balances.contract_storage().get(address);
            for slot in slots.keys() {
                if current_slots.map(|stored| stored.contains_key(slot)) != Some(true) {
                    ops.push(WriteBatchOp::Delete {
                        cf: Cf::ContractStorage,
                        key: contract_storage_key(address, slot),
                    });
                    root_touch.insert(address.clone());
                }
            }
        }

        // S3-P09: emit the storage-root cache refresh for every address
        // touched above (account change, slot write, removal, backfill).
        // Value = the consensus-derived `AccountState.storage_root` — the
        // cache never recomputes roots itself; a removed/never-contracted
        // address drops its entry (Delete of an absent key is a no-op).
        for address in &root_touch {
            let root = self
                .balances
                .get(address)
                .and_then(|account| account.as_contract().map(|contract| contract.storage_root));
            match root {
                Some(root) => ops.push(WriteBatchOp::Put {
                    cf: Cf::ContractStorage,
                    key: contract_storage_root_key(address),
                    value: root.to_vec(),
                }),
                None => ops.push(WriteBatchOp::Delete {
                    cf: Cf::ContractStorage,
                    key: contract_storage_root_key(address),
                }),
            }
        }

        // ---- 4. mempool: put new/changed, delete mined/replaced/evicted ----
        let current_mempool: Vec<Transaction> = self.mempool.transactions();
        let mut current_mempool_keys: HashMap<(String, u64), [u8; 32]> = HashMap::new();
        for tx in &current_mempool {
            current_mempool_keys.insert(
                (tx.sender().to_string(), tx.nonce()),
                strangecoin_core::serialize::txid(tx),
            );
        }
        for tx in &current_mempool {
            let key = (tx.sender().to_string(), tx.nonce());
            if self.persisted_mempool.get(&key) != Some(&strangecoin_core::serialize::txid(tx)) {
                ops.push(WriteBatchOp::Put {
                    cf: Cf::Mempool,
                    key: mempool_key(&key.0, key.1),
                    value: codec::encode_tx(tx),
                });
            }
        }
        let removed_mempool: Vec<(String, u64)> = self
            .persisted_mempool
            .keys()
            .filter(|key| !current_mempool_keys.contains_key(*key))
            .cloned()
            .collect();
        for key in &removed_mempool {
            ops.push(WriteBatchOp::Delete {
                cf: Cf::Mempool,
                key: mempool_key(&key.0, key.1),
            });
        }

        // ---- commit: atomic batch + durability barrier, then sync the
        // in-memory snapshots to what the DB now holds ----
        engine.write_batch(ops)?;
        engine.flush()?;

        self.persisted_accounts = self.balances.accounts().clone();
        self.persisted_contract_storage = self.balances.contract_storage().clone();
        self.persisted_mempool = current_mempool_keys;
        self.pending_state_history.clear();
        Ok(())
    }

    /// Legacy DB: move genesis placeholder balance to the single real wallet.
    pub(crate) fn migrate_initial_wallet_balance(&mut self) {
        if !self.allow_grant_blocks {
            return;
        }
        if self.chain.len() != 1 {
            return;
        }
        let placeholder_amount = match self.balances.get("initial_wallet_address") {
            Some(a) => a.balance(),
            None => return,
        };
        let real_wallets: Vec<String> = self
            .balances
            .keys()
            .filter(|k| k.as_str() != "initial_wallet_address")
            .cloned()
            .collect();
        if real_wallets.len() != 1 {
            return;
        }
        let wallet = &real_wallets[0];
        if self.balances.get(wallet).map(|a| a.balance()).unwrap_or(0) != 0 {
            return;
        }
        self.create_grant_block(wallet, placeholder_amount);
        info!(amount = placeholder_amount, wallet = %wallet, "Первоначальный баланс перенесён первому кошельку");
    }

    /// Legacy DB: rewrite base64 addresses to bech32; reset chain if needed.
    pub(crate) fn migrate_addresses_to_bech32(&mut self) {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine;

        let network_id = self.chain_id;
        let mut new_balances = HashMap::new();
        let mut migrated_count = 0usize;
        let mut unchanged_count = 0usize;

        for (addr, account) in self.balances.accounts().iter() {
            let new_addr = if crate::address::decode_address(addr).is_ok() {
                unchanged_count += 1;
                addr.clone()
            } else if let Ok(pk_bytes) = BASE64.decode(addr) {
                if let Ok(pk) = secp256k1::PublicKey::from_slice(&pk_bytes) {
                    if let Ok(bech32_addr) = crate::address::encode_address(&pk, network_id) {
                        migrated_count += 1;
                        info!(old_address = %addr, new_address = %bech32_addr, "Миграция адреса");
                        bech32_addr
                    } else {
                        unchanged_count += 1;
                        addr.clone()
                    }
                } else {
                    unchanged_count += 1;
                    addr.clone()
                }
            } else {
                unchanged_count += 1;
                addr.clone()
            };
            *new_balances.entry(new_addr).or_insert(0u64) += account.balance();
        }

        if migrated_count > 0 {
            info!(
                migrated_count,
                unchanged_count, "Миграция балансов завершена"
            );
            let mut new_accounts = HashMap::new();
            for (addr, balance) in new_balances {
                new_accounts.insert(addr, AccountState::eoa(balance, 0));
            }
            self.balances = StateCache::from_accounts(new_accounts);
        }

        let mut legacy_in_chain = false;
        'outer: for block in &self.chain {
            for tx in &block.transactions {
                let fields: [&str; 2] = [tx.sender(), tx.receiver().unwrap_or("")];
                for field in fields {
                    if crate::address::decode_address(field).is_err()
                        && !matches!(
                            field,
                            "genesis"
                                | "coinbase"
                                | "initial_wallet_address"
                                | "regtest_initial_holder"
                                | "recipient"
                        )
                    {
                        if let Ok(pk_bytes) = BASE64.decode(field) {
                            if secp256k1::PublicKey::from_slice(&pk_bytes).is_ok() {
                                legacy_in_chain = true;
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        if legacy_in_chain {
            info!("Обнаружены legacy-адреса в цепи; сбрасываем цепь до генезиса");
            self.chain = vec![];
            Blockchain::install_fresh_genesis(self);
        }
    }
}

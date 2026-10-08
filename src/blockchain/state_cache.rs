//! # State cache (ARCHITECT3 §3.4, component 3)
//!
//! The single place where account balances and nonces are read, plus the
//! chain persistence helpers and DB-open migrations that moved out of the
//! facade in S1.5-P04.
//!
//! Invariant #1 («вся валидность — из цепочки»): when the cached accounts and
//! a reconstruction from the chain disagree, the reconstruction wins.

use std::collections::HashMap;
use std::path::PathBuf;

use rusty_leveldb::LdbIterator;
use serde::Serialize;
use strangecoin_core::state::{unapply_block, State};
use strangecoin_core::types::{Block, Transaction};
use tracing::{debug, info};

use crate::blockchain::block_executor::{self, BlockView};
use crate::blockchain::blockchain_facade::Blockchain;
use crate::error::StrangecoinError;
use crate::AccountState;

/// Balances/nonces cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateCache {
    accounts: HashMap<String, AccountState>,
}

impl StateCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_accounts(accounts: HashMap<String, AccountState>) -> Self {
        Self { accounts }
    }

    pub fn from_state(state: &State) -> Self {
        Self {
            accounts: state.balances.clone(),
        }
    }

    pub fn to_state(&self) -> State {
        State {
            balances: self.accounts.clone(),
        }
    }

    // ---------------------------------------------------------------- reads

    pub fn get(&self, address: &str) -> Option<AccountState> {
        self.accounts.get(address).cloned()
    }

    pub fn balance(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.balance)
            .unwrap_or(0)
    }

    pub fn nonce(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.nonce)
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
        self.accounts.values().map(|account| account.balance).sum()
    }

    pub fn nonzero_balances(&self) -> HashMap<String, u64> {
        self.accounts
            .iter()
            .filter(|(_, account)| account.balance != 0)
            .map(|(address, account)| (address.clone(), account.balance))
            .collect()
    }

    // --------------------------------------------------------------- writes

    pub fn commit(&mut self, state: State) {
        self.accounts = state.balances;
    }

    pub fn replace(&mut self, other: StateCache) {
        self.accounts = other.accounts;
    }

    pub fn invalidate(&mut self) {
        self.accounts.clear();
    }

    pub fn unapply_block(&mut self, block: &Block) -> Result<(), StrangecoinError> {
        let rolled_back = unapply_block(&self.to_state(), block)?;
        self.commit(rolled_back);
        Ok(())
    }

    pub fn credit(&mut self, address: &str, amount: u64) {
        self.accounts
            .entry(address.to_string())
            .or_default()
            .balance += amount;
    }

    pub fn ensure_account(&mut self, address: &str) {
        self.accounts.entry(address.to_string()).or_default();
    }

    pub fn rebuild_from_chain(
        chain: &[Block],
        now: u64,
        allow_grant_blocks: bool,
        rules: &super::consensus_manager::ConsensusManager,
    ) -> Result<Self, StrangecoinError> {
        let mut state = State::new();
        for (height, block) in chain.iter().enumerate() {
            let view = BlockView::new(
                &chain[..height],
                now,
                allow_grant_blocks,
                rules.expected_version(height as u64),
            )
            .with_phase(rules.phase_at(height as u64));
            state = block_executor::validate_and_apply(&state, block, &view).map_err(|e| {
                tracing::warn!(block_index = height, error = %e, "Rebuild stopped at an invalid block");
                e
            })?;
        }
        Ok(Self::from_state(&state))
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
// Storage paths + LevelDB load/save + migrations (S1.5-P04)
// ---------------------------------------------------------------------------

/// Persisted state read from LevelDB on open.
pub(crate) struct PersistedState {
    pub chain: Option<Vec<Block>>,
    pub balances: Option<HashMap<String, u64>>,
    pub difficulty: Option<u32>,
    pub mempool_txs: Vec<Transaction>,
}

pub(crate) fn db_path_for_port(port: u16) -> PathBuf {
    let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
    let exe_dir = exe_path
        .parent()
        .expect("Не удалось получить директорию исполняемого файла");
    exe_dir.join(format!("blockchain_db_{}", port))
}

pub(crate) fn db_path_from_env() -> PathBuf {
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8081".to_string())
        .parse::<u16>()
        .unwrap_or(8081);
    db_path_for_port(port)
}

/// Open or create storage for `port`, load persisted state, install genesis
/// when the DB is empty, run migrations, persist. (S1.5-P04: moved from facade.)
pub(crate) fn open_blockchain(port: u16) -> Blockchain {
    use super::consensus_manager::ConsensusManager;
    use tracing::info;

    let start_time = std::time::SystemTime::now();
    let db_path = db_path_for_port(port);
    if !db_path.exists() {
        info!("База данных не существует, создаётся новая");
    }
    let storage = crate::storage::Storage::new(&db_path).expect("Не удалось открыть LevelDB");

    let mut blockchain = Blockchain {
        chain: vec![],
        balances: StateCache::new(),
        difficulty: 1,
        mempool: crate::mempool::Mempool::new(),
        storage,
        allow_grant_blocks: false,
        total_work: [0, 0, 0, 0],
        rules: ConsensusManager::new(),
    };
    blockchain.debug_db();

    let loaded = load_persisted_state(&blockchain.storage);
    for tx in loaded.mempool_txs {
        let account = AccountState {
            balance: 0,
            nonce: 0,
        };
        let _ = blockchain.mempool.insert(tx, &account);
    }
    if let Some(chain) = loaded.chain {
        let is_regtest = crate::consensus::is_regtest(crate::consensus::current_chain_id());
        if !chain.is_empty() {
            if let Err(e) = crate::consensus::validate_genesis(&chain[0], is_regtest) {
                panic!("Genesis validation failed: {}", e);
            }
        }
        blockchain.chain = chain;
    } else {
        Blockchain::install_fresh_genesis(&mut blockchain);
    }

    if let Some(balances) = loaded.balances {
        blockchain.balances = StateCache::from_accounts(
            balances
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        AccountState {
                            balance: v,
                            nonce: 0,
                        },
                    )
                })
                .collect(),
        );
    }
    if let Some(difficulty) = loaded.difficulty {
        blockchain.difficulty = difficulty;
    }
    blockchain.total_work = strangecoin_core::consensus::cumulative_work(&blockchain.chain);
    blockchain.migrate_initial_wallet_balance();
    blockchain.migrate_addresses_to_bech32();
    blockchain.save_state();
    blockchain.debug_db();
    let duration = start_time
        .elapsed()
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    info!(duration_secs = duration, "Создание блокчейна завершено");
    blockchain
}

/// Read chain/balances/difficulty and mempool txs from LevelDB.
pub(crate) fn load_persisted_state(storage: &crate::storage::Storage) -> PersistedState {
    let db_arc = storage.db();
    let mut db_guard = db_arc
        .lock()
        .expect("Не удалось захватить Mutex для LevelDB");

    let chain = db_guard
        .get(b"chain")
        .and_then(|v| serde_json::from_slice::<Vec<Block>>(&v).ok());
    let balances = db_guard
        .get(b"balances")
        .and_then(|v| serde_json::from_slice::<HashMap<String, u64>>(&v).ok());
    let difficulty = db_guard
        .get(b"difficulty")
        .and_then(|v| serde_json::from_slice::<u32>(&v).ok());

    let mut iterator = db_guard
        .new_iter()
        .expect("Не удалось создать итератор LevelDB");
    let mut mempool_txs = Vec::new();
    while let Some((key, value)) = iterator.next() {
        if key == b"chain" || key == b"balances" || key == b"difficulty" {
            continue;
        }
        if let Ok(transaction) = serde_json::from_slice::<Transaction>(&value) {
            mempool_txs.push(transaction);
        }
    }
    drop(db_guard);

    PersistedState {
        chain,
        balances,
        difficulty,
        mempool_txs,
    }
}

impl Blockchain {
    pub(crate) fn debug_db(&self) {
        let db_arc = self.storage.db();
        let mut db = db_arc
            .lock()
            .expect("Не удалось захватить Mutex для LevelDB");
        let mut iterator = db.new_iter().expect("Не удалось создать итератор LevelDB");
        debug!("Содержимое базы данных:");
        while let Some((key, value)) = iterator.next() {
            let key_str = std::str::from_utf8(&key).unwrap_or("невалидный ключ");
            debug!(key = %key_str, "Ключ");
            if key == b"chain" {
                debug!(value = ?serde_json::from_slice::<Vec<Block>>(&value), "Значение (chain)");
            } else if key == b"balances" {
                debug!(value = ?serde_json::from_slice::<HashMap<String, u64>>(&value), "Значение (balances)");
            } else if key == b"difficulty" {
                debug!(value = ?serde_json::from_slice::<u32>(&value), "Значение (difficulty)");
            } else {
                debug!(value = ?serde_json::from_slice::<Transaction>(&value), "Значение (transaction)");
            }
        }
    }

    /// Validate a transaction against current state and queue it in the mempool.
    pub(crate) fn add_transaction(
        &mut self,
        transaction: Transaction,
    ) -> Result<crate::mempool::InsertOutcome, StrangecoinError> {
        if transaction.sender.is_empty() || transaction.receiver.is_empty() {
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
        let account_state = self.balances.get(&transaction.sender).unwrap_or_default();
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
            &self.rules,
        ) {
            Ok(cache) => cache,
            Err(e) => {
                tracing::warn!(error = %e, "Цепочка отклонена: блок не прошёл валидацию");
                return false;
            }
        };
        let mut temp_state = reconstructed.to_state();
        for tx in self.mempool.transactions() {
            let sender_balance = temp_state.get_balance(&tx.sender);
            if sender_balance < tx.amount {
                tracing::warn!(
                    sender = %tx.sender,
                    nonce = tx.nonce,
                    required = tx.amount,
                    available = sender_balance,
                    "Недостаточно средств в mempool"
                );
                return false;
            }
            temp_state.set_balance(&tx.sender, sender_balance.saturating_sub(tx.amount));
            let receiver_balance = temp_state.get_balance(&tx.receiver);
            temp_state.set_balance(&tx.receiver, receiver_balance.saturating_add(tx.amount));
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
        self.balances =
            StateCache::rebuild_from_chain(&self.chain, now, self.allow_grant_blocks, &self.rules)?;
        Ok(())
    }

    pub(crate) fn save_state(&mut self) {
        let db_arc = self.storage.db();
        let mut db = db_arc
            .lock()
            .expect("Не удалось захватить Mutex для LevelDB");
        for tx in self.mempool.transactions() {
            let key = format!("{}:{}", tx.sender, tx.nonce).into_bytes();
            match db.get(&key) {
                Some(_) => continue,
                None => {
                    let value = serde_json::to_vec(&tx).expect("Ошибка сериализации транзакции");
                    db.put(&key, &value)
                        .expect("Ошибка сохранения транзакции в LevelDB");
                }
            }
        }
        db.put(b"chain", &serde_json::to_vec(&self.chain).unwrap())
            .expect("Ошибка сохранения цепочки блоков");
        db.put(b"balances", &serde_json::to_vec(&self.balances).unwrap())
            .expect("Ошибка сохранения балансов");
        db.put(
            b"difficulty",
            &serde_json::to_vec(&self.difficulty).unwrap(),
        )
        .expect("Ошибка сохранения сложности");
        db.flush().expect("Ошибка при фиксации данных в LevelDB");
        drop(db);
        self.debug_db();
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
            Some(a) => a.balance,
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
        if self.balances.get(wallet).map(|a| a.balance).unwrap_or(0) != 0 {
            return;
        }
        self.create_grant_block(wallet, placeholder_amount);
        info!(amount = placeholder_amount, wallet = %wallet, "Первоначальный баланс перенесён первому кошельку");
    }

    /// Legacy DB: rewrite base64 addresses to bech32; reset chain if needed.
    pub(crate) fn migrate_addresses_to_bech32(&mut self) {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine;

        let network_id = crate::consensus::current_chain_id();
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
            *new_balances.entry(new_addr).or_insert(0u64) += account.balance;
        }

        if migrated_count > 0 {
            info!(
                migrated_count,
                unchanged_count, "Миграция балансов завершена"
            );
            let mut new_accounts = HashMap::new();
            for (addr, balance) in new_balances {
                new_accounts.insert(addr, AccountState { balance, nonce: 0 });
            }
            self.balances = StateCache::from_accounts(new_accounts);
        }

        let mut legacy_in_chain = false;
        'outer: for block in &self.chain {
            for tx in &block.transactions {
                for field in [&tx.sender, &tx.receiver] {
                    if crate::address::decode_address(field).is_err()
                        && !matches!(
                            field.as_str(),
                            "genesis" | "coinbase"
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

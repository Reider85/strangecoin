//! # Blockchain facade (ARCHITECT3 §3.4, component 4+1)
//!
//! Two layers live here:
//!
//! * [`Blockchain`] — the chain data itself (blocks, state cache, mempool,
//!   storage handle, consensus rules). Fields are `pub(crate)`: the only
//!   supported way to reach state from outside this module is
//!   [`BlockchainFacade`].
//! * [`BlockchainFacade`] — the sole public entry point. Wraps
//!   `Arc<RwLock<Blockchain>>`, takes the lock per call, and exposes the
//!   operations consumers need (apply tx, mine, validate, adopt, wire).
//!
//! Fork choice goes through [`ChainSelector`](super::chain_selector::ChainSelector)
//! (work → timestamp → hash) in [`BlockchainFacade::adopt_candidate`]; every
//! adoption site (P2P UPDATE, sync round, UI sync channel, test helpers) must
//! call that one method.
//!
//! Consensus *rules* are never resolved here: the executor receives them via
//! `BlockView` from [`ConsensusManager`](super::consensus_manager::ConsensusManager)
//! held on [`Blockchain::rules`].

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, RwLock,
};
use std::time::{SystemTime, UNIX_EPOCH};

use rusty_leveldb::LdbIterator;
use serde::{Deserialize, Deserializer, Serialize};
use tracing::{debug, error, info, warn};

use super::block_executor::{self, BlockView};
use super::chain_selector::ChainSelector;
use super::consensus_manager::ConsensusManager;
use super::state_cache::StateCache;
use crate::error::StrangecoinError;
use crate::AccountState;
use strangecoin_core::types::{Block, BlockHeader, ChainSnapshot, Transaction};

/// Wire shape of a chain snapshot: what peers send and what `sync_rx` carries.
///
/// Serialize for [`Blockchain`] writes only `mempool_txs`; both mempool fields
/// need `#[serde(default)]` or the receiver rejects the whole payload.
#[derive(Deserialize, Serialize)]
pub struct BlockchainDeserialize {
    pub chain: Vec<Block>,
    pub balances: HashMap<String, AccountState>,
    pub difficulty: u32,
    #[serde(default)]
    pub pending_transactions: Vec<Transaction>,
    #[serde(default)]
    pub mempool_txs: Vec<Transaction>,
    #[serde(default)]
    pub total_work: strangecoin_core::consensus::U256,
}

/// Chain data. Not part of the public API — use [`BlockchainFacade`].
#[derive(Clone)]
pub struct Blockchain {
    pub(crate) chain: Vec<Block>,
    /// Sole read path for balances/nonces (ARCHITECT3 §3.4): every balance
    /// change goes through `block_executor`, every read through `StateCache`.
    pub(crate) balances: StateCache,
    pub(crate) difficulty: u32,
    pub(crate) mempool: crate::mempool::Mempool,
    pub(crate) storage: crate::storage::Storage,
    pub(crate) allow_grant_blocks: bool,
    pub(crate) total_work: strangecoin_core::consensus::U256,
    /// Consensus rules for this chain (single source for the executor).
    pub(crate) rules: ConsensusManager,
}

impl Serialize for Blockchain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Blockchain", 5)?;
        state.serialize_field("chain", &self.chain)?;
        state.serialize_field("balances", &self.balances)?;
        state.serialize_field("difficulty", &self.difficulty)?;
        state.serialize_field("mempool_txs", &self.mempool.transactions())?;
        state.serialize_field("total_work", &self.total_work)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Blockchain {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let BlockchainDeserialize {
            chain,
            balances,
            difficulty,
            pending_transactions,
            mempool_txs,
            total_work,
        } = BlockchainDeserialize::deserialize(deserializer)?;

        let port = std::env::var("PORT")
            .unwrap_or("8081".to_string())
            .parse::<u16>()
            .unwrap_or(8081);
        let exe_path =
            std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path
            .parent()
            .expect("Не удалось получить директорию исполняемого файла");
        let db_path = exe_dir.join(format!("blockchain_db_{}", port));

        let mut mempool = crate::mempool::Mempool::new();
        for tx in pending_transactions {
            let account = AccountState {
                balance: 0,
                nonce: 0,
            };
            let _ = mempool.insert(tx, &account);
        }
        for tx in mempool_txs {
            let account = AccountState {
                balance: 0,
                nonce: 0,
            };
            let _ = mempool.insert(tx, &account);
        }

        let storage = crate::storage::Storage::new(&db_path).map_err(serde::de::Error::custom)?;

        Ok(Blockchain {
            chain,
            balances: StateCache::from_accounts(balances),
            difficulty,
            mempool,
            storage,
            allow_grant_blocks: false,
            total_work,
            rules: ConsensusManager::new(),
        })
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

    pub(crate) fn new(port: u16) -> Self {
        let start_time = SystemTime::now();
        let exe_path =
            std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path
            .parent()
            .expect("Не удалось получить директорию исполняемого файла");
        let db_path = exe_dir.join(format!("blockchain_db_{}", port));
        debug!(path = %db_path.display(), "Проверка базы данных");
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

        let (chain_opt, balances_opt, difficulty_opt) = {
            let db_arc = blockchain.storage.db();
            let mut db_guard = db_arc
                .lock()
                .expect("Не удалось захватить Mutex для LevelDB");

            let chain_opt = db_guard
                .get(b"chain")
                .and_then(|v| serde_json::from_slice::<Vec<Block>>(&v).ok());
            debug!(?chain_opt, "chain_opt");

            let balances_opt = db_guard
                .get(b"balances")
                .and_then(|v| serde_json::from_slice::<HashMap<String, u64>>(&v).ok());
            debug!(?balances_opt, "balances_opt");

            let difficulty_opt = db_guard
                .get(b"difficulty")
                .and_then(|v| serde_json::from_slice::<u32>(&v).ok());
            debug!(?difficulty_opt, "difficulty_opt");

            let mut iterator = db_guard
                .new_iter()
                .expect("Не удалось создать итератор LevelDB");
            debug!("Загрузка транзакций из LevelDB...");
            while let Some((key, value)) = iterator.next() {
                let key_str = std::str::from_utf8(&key).unwrap_or("невалидный ключ");
                debug!(key = %key_str, "Найден ключ");
                if key == b"chain" || key == b"balances" || key == b"difficulty" {
                    debug!(?key, "Пропуск системного ключа");
                    continue;
                }
                match serde_json::from_slice::<Transaction>(&value) {
                    Ok(transaction) => {
                        debug!(?transaction, "Успешно загружена транзакция");
                        let account = AccountState {
                            balance: 0,
                            nonce: 0,
                        };
                        let _ = blockchain.mempool.insert(transaction, &account);
                    }
                    Err(e) => {
                        debug!(key = %key_str, error = %e, "Ошибка десериализации транзакции")
                    }
                }
            }
            debug!(
                count = blockchain.mempool.len(),
                "Загружено транзакций в mempool"
            );
            (chain_opt, balances_opt, difficulty_opt)
        };

        if let Some(chain) = chain_opt {
            let chain_id = crate::consensus::current_chain_id();
            let is_regtest = crate::consensus::is_regtest(chain_id);
            if !chain.is_empty() {
                if let Err(e) = crate::consensus::validate_genesis(&chain[0], is_regtest) {
                    panic!("Genesis validation failed: {}", e);
                }
            }
            blockchain.chain = chain;
        } else {
            let chain_id = crate::consensus::current_chain_id();
            let is_regtest = crate::consensus::is_regtest(chain_id);

            let genesis_block = if is_regtest {
                let genesis_tx = crate::Transaction {
                    sender: "genesis".to_string(),
                    receiver: "regtest_initial_holder".to_string(),
                    amount: 1_000_000_000,
                    nonce: 0,
                    chain_id,
                    signature: Vec::new(),
                    is_coinbase: true,
                };
                let target_bytes =
                    hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
                        .expect("valid target hex");
                let mut target_arr = [0u8; 32];
                target_arr.copy_from_slice(&target_bytes);
                let mut block = crate::Block {
                    index: 0,
                    timestamp: 0,
                    transactions: vec![genesis_tx],
                    previous_hash: "0".repeat(64),
                    hash: String::new(),
                    nonce: 0,
                    target: hex::encode(target_arr),
                    consensus_version: blockchain.rules.expected_version(0),
                    state_root: [0u8; 32],
                    tx_root: [0u8; 32],
                };
                block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
                let hash = strangecoin_core::serialize::block_hash(&block);
                block.hash = hex::encode(hash);
                block
            } else {
                let genesis_path = exe_dir.join("genesis.json");
                crate::consensus::load_genesis(genesis_path.to_str().unwrap())
                    .expect("Failed to load genesis from genesis.json")
            };

            if let Err(e) = crate::consensus::validate_genesis(&genesis_block, is_regtest) {
                panic!("Genesis validation failed: {}", e);
            }
            blockchain.chain.push(genesis_block.clone());

            for tx in &genesis_block.transactions {
                if tx.sender != "genesis" {
                    blockchain.balances.credit(&tx.receiver, tx.amount);
                }
            }
        }

        if let Some(balances) = balances_opt {
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

        if let Some(difficulty) = difficulty_opt {
            blockchain.difficulty = difficulty;
        }

        blockchain.total_work = strangecoin_core::consensus::cumulative_work(&blockchain.chain);

        blockchain.migrate_initial_wallet_balance();
        blockchain.migrate_addresses_to_bech32();

        blockchain.save_state();
        blockchain.debug_db();
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(duration_secs = duration, "Создание блокчейна завершено");
        blockchain
    }

    /// Block view for a block at `height` on the current chain, rules included.
    fn view_for(&self, height: u64) -> BlockView<'_> {
        BlockView::new(
            &self.chain,
            block_executor::now_secs(),
            self.allow_grant_blocks,
            self.rules.expected_version(height),
        )
        .with_phase(self.rules.phase_at(height))
    }

    pub(crate) fn create_genesis_block(&mut self) {
        let start_time = SystemTime::now();
        let genesis_transaction = Transaction {
            sender: "genesis".to_string(),
            receiver: "initial_wallet_address".to_string(),
            amount: 10000,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: true,
        };
        let genesis_block = Block {
            index: 0,
            timestamp: 0,
            transactions: vec![genesis_transaction],
            previous_hash: "0".repeat(64),
            hash: String::new(),
            nonce: 0,
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            consensus_version: self.rules.expected_version(0),
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        let mut genesis_block = genesis_block;
        // tx_root обязан вычисляться ДО хэша: block_hash включает tx_root,
        // иначе сохранённый хэш не совпадёт с пересчётом в validate_chain().
        genesis_block.tx_root =
            strangecoin_core::serialize::compute_tx_root(&genesis_block.transactions);
        genesis_block.hash = self.calculate_hash(&genesis_block);

        // Валидация и применение — через block_executor, а не ручной
        // правкой balances: иначе состояние расходилось бы с пересчётом
        // из цепочки в validate_chain().
        let applied = {
            let view = BlockView::new(
                &[],
                block_executor::now_secs(),
                self.allow_grant_blocks,
                self.rules.expected_version(0),
            )
            .with_phase(self.rules.phase_at(0));
            block_executor::validate_and_apply(
                &strangecoin_core::state::State::new(),
                &genesis_block,
                &view,
            )
        };
        match applied {
            Ok(new_state) => {
                self.chain.push(genesis_block);
                self.balances.commit(new_state);
                self.total_work = strangecoin_core::consensus::cumulative_work(&self.chain);
            }
            Err(e) => {
                error!(error = %e, "Не удалось применить генезис-блок");
                return;
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(duration_secs = duration, "Создание генезис-блока завершено");
    }

    // Создаёт блок первичной эмиссии: переводит 10000 с генезис-адреса на первый реальный кошелёк.
    pub(crate) fn create_grant_block(&mut self, wallet_address: &str, amount: u64) {
        let previous_block = self.chain.last().unwrap().clone();
        let height = previous_block.index + 1;
        let coinbase = Transaction {
            sender: "coinbase".to_string(),
            receiver: wallet_address.to_string(),
            amount: 0,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: true,
        };
        let transfer = Transaction {
            sender: "initial_wallet_address".to_string(),
            receiver: wallet_address.to_string(),
            amount,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: false,
        };
        let mut block = Block {
            index: height,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: vec![coinbase, transfer],
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target: previous_block.target.clone(),
            consensus_version: self.rules.expected_version(height),
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
        block.hash = self.calculate_hash(&block);

        // Как и генезис: состояние получается из block_executor, а не
        // правится вручную — иначе validate_chain() (пересчитывающая
        // состояние из цепочки) расходилась бы с хранимыми балансами.
        let applied = {
            let parent_state = self.balances.to_state();
            let view = self.view_for(height);
            block_executor::validate_and_apply(&parent_state, &block, &view)
        };
        match applied {
            Ok(new_state) => {
                self.chain.push(block);
                self.balances.commit(new_state);
                self.total_work = strangecoin_core::consensus::cumulative_work(&self.chain);
                info!(from = "initial_wallet_address", to = %wallet_address, amount, "Создан блок первичной эмиссии");
            }
            Err(e) => {
                error!(error = %e, "Не удалось применить блок первичной эмиссии к состоянию");
            }
        }
    }

    // Начисляет первоначальный баланс (10000 из генезис-блока) первому реальному кошельку
    pub(crate) fn grant_initial_balance_to_first_wallet(
        &mut self,
        wallet_address: &str,
    ) -> Result<bool, StrangecoinError> {
        if !self.allow_grant_blocks {
            warn!("Grant blocks are disabled (allow_grant_blocks = false)");
            return Err(StrangecoinError::GrantBlocksDisabled);
        }
        if self.chain.len() != 1 {
            return Ok(false);
        }
        if self.balances.contains_key(wallet_address) {
            return Ok(false);
        }
        let is_first_wallet =
            self.balances.len() == 1 && self.balances.contains_key("initial_wallet_address");
        if !is_first_wallet {
            return Ok(false);
        }
        let amount = match self.balances.get("initial_wallet_address") {
            Some(a) if a.balance > 0 => a.balance,
            _ => return Ok(false),
        };
        self.create_grant_block(wallet_address, amount);
        info!(wallet = %wallet_address, amount, "Первому кошельку начислен первоначальный баланс");
        Ok(true)
    }

    // Миграция для существующих баз: переносит баланс генезис-кошелька на единственный реальный кошелёк с нулевым балансом
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

    // Миграция legacy base64-адресов на bech32 при открытии базы данных
    pub(crate) fn migrate_addresses_to_bech32(&mut self) {
        let network_id = crate::consensus::current_chain_id();
        
        // Миграция балансов
        let mut new_balances = std::collections::HashMap::new();
        let mut migrated_count = 0;
        let mut unchanged_count = 0;
        
        for (addr, balance) in self.balances.accounts().iter() {
            let new_addr = if let Ok((_, _)) = crate::address::decode_address(addr) {
                // Уже bech32 - оставляем как есть
                unchanged_count += 1;
                addr.clone()
            } else if let Ok(pk_bytes) = BASE64.decode(addr) {
                // Пытаемся интерпретировать как base64 pubkey
                if let Ok(pk) = secp256k1::PublicKey::from_slice(&pk_bytes) {
                    if let Ok(bech32_addr) = crate::address::encode_address(&pk, network_id) {
                        migrated_count += 1;
                        info!(old_address = %addr, new_address = %bech32_addr, "Миграция адреса");
                        bech32_addr
                    } else {
                        // Не удалось закодировать - оставляем как есть
                        unchanged_count += 1;
                        addr.clone()
                    }
                } else {
                    // Не удалось распарсить как pubkey - оставляем как есть
                    unchanged_count += 1;
                    addr.clone()
                }
            } else {
                // Не base64 - оставляем как есть
                unchanged_count += 1;
                addr.clone()
            };
            
            *new_balances.entry(new_addr).or_insert(0) += balance.balance;
        }
        
        if migrated_count > 0 {
            info!(
                migrated_count = migrated_count,
                unchanged_count = unchanged_count,
                "Миграция балансов завершена"
            );
            
            // Заменяем балансы на новые
            let mut new_accounts = std::collections::HashMap::new();
            for (addr, balance) in new_balances {
                new_accounts.insert(addr, AccountState { balance, nonce: 0 });
            }
            self.balances = StateCache::from_accounts(new_accounts);
        }
        
        // Проверка цепи на наличие legacy-адресов
        let mut legacy_in_chain = false;
        for block in &self.chain {
            for tx in &block.transactions {
                // Проверяем sender и receiver
                for field in [&tx.sender, &tx.receiver] {
                    if crate::address::decode_address(field).is_err() {
                        // Проверяем, это не magic-строка
                        if !matches!(field.as_str(), "genesis" | "coinbase" | "initial_wallet_address" | "regtest_initial_holder" | "recipient") {
                            if let Ok(pk_bytes) = BASE64.decode(field) {
                                if secp256k1::PublicKey::from_slice(&pk_bytes).is_ok() {
                                    legacy_in_chain = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                if legacy_in_chain {
                    break;
                }
            }
            if legacy_in_chain {
                break;
            }
        }
        
        if legacy_in_chain {
            info!("Обнаружены legacy-адреса в цепи; сбрасываем цепь до генезиса");
            // Сбрасываем цепь до генезиса
            self.chain = vec![];
            let is_regtest = crate::consensus::is_regtest(network_id);
            
            let genesis_block = if is_regtest {
                let genesis_tx = crate::Transaction {
                    sender: "genesis".to_string(),
                    receiver: "regtest_initial_holder".to_string(),
                    amount: 1_000_000_000,
                    nonce: 0,
                    chain_id: network_id,
                    signature: Vec::new(),
                    is_coinbase: true,
                };
                let target_bytes =
                    hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")
                        .expect("valid target hex");
                let mut target_arr = [0u8; 32];
                target_arr.copy_from_slice(&target_bytes);
                let mut block = crate::Block {
                    index: 0,
                    timestamp: 0,
                    transactions: vec![genesis_tx],
                    previous_hash: "0".repeat(64),
                    hash: String::new(),
                    nonce: 0,
                    target: hex::encode(target_arr),
                    consensus_version: self.rules.expected_version(0),
                    state_root: [0u8; 32],
                    tx_root: [0u8; 32],
                };
                block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
                let hash = strangecoin_core::serialize::block_hash(&block);
                block.hash = hex::encode(hash);
                block
            } else {
                let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
                let exe_dir = exe_path
                    .parent()
                    .expect("Не удалось получить директорию исполняемого файла");
                let genesis_path = exe_dir.join("genesis.json");
                crate::consensus::load_genesis(genesis_path.to_str().unwrap())
                    .expect("Failed to load genesis from genesis.json")
            };
            
            self.chain.push(genesis_block.clone());
            
            for tx in &genesis_block.transactions {
                if tx.sender != "genesis" {
                    self.balances.credit(&tx.receiver, tx.amount);
                }
            }
        }
    }

    pub(crate) fn calculate_hash(&self, block: &Block) -> String {
        let start_time = SystemTime::now();
        let hash_bytes = strangecoin_core::serialize::block_hash(block);
        let hash = hex::encode(hash_bytes);
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        debug!(duration_secs = duration, hash = %hash, "Вычисление хэша завершено");
        hash
    }

    /// Validate + apply `block` on top of the current chain and persist.
    fn commit_block(&mut self, block: &Block) -> Result<(), StrangecoinError> {
        let new_state = {
            let view = self.view_for(block.index);
            let parent_state = self.balances.to_state();
            block_executor::validate_and_apply(&parent_state, block, &view)?
        };
        self.balances.commit(new_state);
        for tx in &block.transactions {
            if !tx.is_coinbase {
                self.mempool.remove(&strangecoin_core::serialize::txid(tx));
            }
        }
        self.chain.push(block.clone());
        self.total_work = strangecoin_core::consensus::cumulative_work(&self.chain);
        Ok(())
    }

    pub(crate) fn mine_block(
        &mut self,
        progress_tx: mpsc::Sender<String>,
        shutdown: &Arc<AtomicBool>,
    ) -> Option<Block> {
        let total_start_time = SystemTime::now();
        info!("Начало майнинга");
        if self.mempool.is_empty() {
            warn!("Нет транзакций для майнинга");
            let _ = progress_tx.send("Нет транзакций для майнинга".to_string());
            return None;
        }

        let previous_block = self.chain.last().unwrap().clone();
        let transactions: Vec<Transaction> = self.mempool.get_pending(1000);
        if transactions.is_empty() {
            warn!("Все pending_transactions уже включены в блоки");
            let _ = progress_tx.send("Все pending_transactions уже включены в блоки".to_string());
            return None;
        }

        let total_supply = self.balances.total_supply();
        let height = previous_block.index + 1;
        let coinbase_amount =
            crate::economics::emission::block_reward_at_height(height, total_supply);

        let miner_address = self
            .balances
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "miner".to_string());
        let coinbase_tx = Transaction {
            sender: "coinbase".to_string(),
            receiver: miner_address,
            amount: coinbase_amount,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: true,
        };

        let mut all_transactions = vec![coinbase_tx];
        all_transactions.extend(transactions);

        let block = self.mine_block_inner(
            previous_block,
            all_transactions,
            progress_tx.clone(),
            shutdown,
        );

        if let Some(block) = block {
            // Добытый блок проходит тот же validate+apply, что и любой чужой:
            // блок с невалидной подписью или неверным наградом не попадёт в цепочку.
            if let Err(e) = self.commit_block(&block) {
                error!(error = %e, "Failed to apply block to state");
                return None;
            }

            let db_arc = self.storage.db();
            let mut db = db_arc
                .lock()
                .expect("Не удалось захватить Mutex для LevelDB");
            if let Err(e) = db.put(b"chain", &serde_json::to_vec(&self.chain).unwrap()) {
                error!(error = %e, "Ошибка сохранения цепочки блоков в LevelDB");
            }
            if let Err(e) = db.put(b"balances", &serde_json::to_vec(&self.balances).unwrap()) {
                error!(error = %e, "Ошибка сохранения балансов в LevelDB");
            }
            if let Err(e) = db.put(
                b"difficulty",
                &serde_json::to_vec(&self.difficulty).unwrap(),
            ) {
                error!(error = %e, "Ошибка сохранения сложности в LevelDB");
            }
            drop(db);
            let total_duration = SystemTime::now()
                .duration_since(total_start_time)
                .unwrap()
                .as_secs_f64();
            info!(duration_secs = total_duration, block = ?block, "Майнинг завершен, блок добавлен");
            let _ = progress_tx.send(format!("Майнинг завершен за {} секунд", total_duration));
            Some(block)
        } else {
            let total_duration = SystemTime::now()
                .duration_since(total_start_time)
                .unwrap()
                .as_secs_f64();
            warn!(duration_secs = total_duration, "Майнинг не удался");
            let _ = progress_tx.send(format!("Майнинг не удался за {} секунд", total_duration));
            None
        }
    }

    pub(crate) fn mine_block_inner(
        &self,
        previous_block: Block,
        transactions: Vec<Transaction>,
        progress_tx: mpsc::Sender<String>,
        shutdown: &Arc<AtomicBool>,
    ) -> Option<Block> {
        let start_time = SystemTime::now();
        debug!("Начало mine_block_inner");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mtp = crate::consensus::median_time_past(&self.chain, previous_block.index + 1);

        let target =
            if (previous_block.index + 1).is_multiple_of(crate::consensus::RETARGET_INTERVAL) {
                let new_target = crate::consensus::compute_target(&self.chain);
                hex::encode(new_target)
            } else {
                previous_block.target.clone()
            };

        let height = previous_block.index + 1;
        let mut block = Block {
            index: height,
            timestamp: now.max(mtp + 1),
            transactions,
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target,
            consensus_version: self.rules.expected_version(height),
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);

        let target_bytes = hex::decode(&block.target).expect("valid target hex");
        let mut target_arr = [0u8; 32];
        target_arr.copy_from_slice(&target_bytes);
        let target_u256 = crate::consensus::u256_from_bytes(&target_arr);

        let mut iteration_count = 0u64;
        let mut total_hash_time = 0.0;

        loop {
            if shutdown.load(Ordering::Relaxed) {
                info!("Shutdown signal received, stopping mining");
                return None;
            }
            iteration_count += 1;
            let hash_start_time = SystemTime::now();
            let hash = self.calculate_hash(&block);
            let hash_duration = SystemTime::now()
                .duration_since(hash_start_time)
                .unwrap()
                .as_secs_f64();
            total_hash_time += hash_duration;

            let hash_bytes = hex::decode(&hash).expect("valid hash hex");
            let mut hash_arr = [0u8; 32];
            hash_arr.copy_from_slice(&hash_bytes);
            let hash_u256 = crate::consensus::u256_from_bytes(&hash_arr);

            if crate::consensus::u256_le(hash_u256, target_u256) {
                block.hash = hash;
                let total_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                let avg_hash_time = if iteration_count > 0 {
                    total_hash_time / iteration_count as f64
                } else {
                    0.0
                };
                info!(
                    iterations = iteration_count,
                    duration_secs = total_duration,
                    avg_hash_time_secs = avg_hash_time,
                    "Подходящий хэш найден"
                );
                let _ = progress_tx.send(format!(
                    "Подходящий хэш найден после {} итераций за {} секунд",
                    iteration_count, total_duration
                ));
                return Some(block);
            }
            block.nonce += 1;
            if iteration_count.is_multiple_of(10000) {
                let progress_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                let avg_hash_time = if iteration_count > 0 {
                    total_hash_time / iteration_count as f64
                } else {
                    0.0
                };
                debug!(
                    iterations = iteration_count,
                    progress_duration_secs = progress_duration,
                    avg_hash_time_secs = avg_hash_time,
                    "Прогресс майнинга"
                );
                let _ = progress_tx.send(format!(
                    "Прогресс майнинга: {} итераций за {} секунд",
                    iteration_count, progress_duration
                ));
            }
        }
    }

    pub(crate) fn add_transaction(
        &mut self,
        transaction: Transaction,
    ) -> Result<crate::mempool::InsertOutcome, StrangecoinError> {
        let start_time = SystemTime::now();
        debug!(?transaction, "Начало добавления транзакции");
        if transaction.sender.is_empty() || transaction.receiver.is_empty() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            warn!(
                duration_secs = duration,
                "Пустой адрес отправителя или получателя"
            );
            return Err(StrangecoinError::SizeLimitExceeded("empty address"));
        }
        let tx_size = match serde_json::to_vec(&transaction) {
            Ok(v) => v.len(),
            Err(e) => {
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                error!(error = %e, duration_secs = duration, "Ошибка сериализации транзакции для проверки размера");
                return Err(StrangecoinError::SerializationError(e));
            }
        };
        if tx_size > crate::network::protocol::MAX_TX_SIZE {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            warn!(
                tx_size,
                limit = crate::network::protocol::MAX_TX_SIZE,
                duration_secs = duration,
                "TX size limit exceeded"
            );
            return Err(StrangecoinError::SizeLimitExceeded("transaction"));
        }
        let account_state = self.balances.get(&transaction.sender).unwrap_or_default();
        let outcome = self.mempool.insert(transaction, &account_state)?;
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(
            duration_secs = duration,
            pending_count = self.mempool.len(),
            "Транзакция добавлена в mempool"
        );
        Ok(outcome)
    }

    pub(crate) fn validate_chain(&self) -> bool {
        let start_time = SystemTime::now();
        info!("Начало валидации цепочки блоков");
        if self.chain.is_empty() {
            warn!("Цепочка пуста, невалидна");
            return false;
        }

        // Block size validation (invariant #7)
        for block in &self.chain {
            let block_size = strangecoin_core::serialize::serialize_block(block).len();
            if block_size > crate::network::protocol::MAX_BLOCK_SIZE {
                warn!(
                    block_index = block.index,
                    block_size,
                    limit = crate::network::protocol::MAX_BLOCK_SIZE,
                    "Block size limit exceeded"
                );
                return false;
            }
        }

        // Все проверки блоков (consensus_version, подписи, позиция в цепочке,
        // хэш, timestamp, difficulty/target, tx_root, эмиссия, state_root) и
        // пересчёт состояния выполняет block_executor — блок за блоком.
        let reconstructed = match StateCache::rebuild_from_chain(
            &self.chain,
            block_executor::now_secs(),
            self.allow_grant_blocks,
            &self.rules,
        ) {
            Ok(cache) => cache,
            Err(e) => {
                warn!(error = %e, "Цепочка отклонена: блок не прошёл валидацию");
                return false;
            }
        };

        // Проверяем неподтверждённые транзакции (mempool)
        let mut temp_state = reconstructed.to_state();
        for tx in self.mempool.transactions() {
            let sender_balance = temp_state.get_balance(&tx.sender);
            if sender_balance < tx.amount {
                warn!(sender = %tx.sender, nonce = tx.nonce, required = tx.amount, available = sender_balance, "Недостаточно средств в mempool");
                return false;
            }
            temp_state.set_balance(&tx.sender, sender_balance.saturating_sub(tx.amount));
            let receiver_balance = temp_state.get_balance(&tx.receiver);
            temp_state.set_balance(&tx.receiver, receiver_balance.saturating_add(tx.amount));
        }

        // Инвариант №1 («вся валидность — из цепочки»): если хранимый кэш
        // расходится с реконструкцией из цепочки, побеждает реконструкция —
        // здесь расхождение делает цепочку невалидной, а
        // StateCache::rebuild_from_chain() пересчитывает кэш из цепочки.
        let rebuilt = reconstructed.nonzero_balances();
        let stored = self.balances.nonzero_balances();
        debug!(?rebuilt, ?stored, "Сверка восстановленных балансов");
        if rebuilt != stored {
            warn!("Восстановленные балансы не совпадают с хранимыми");
            return false;
        }

        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(duration_secs = duration, "Валидация цепочки завершена");
        true
    }

    /// Пересчитывает кэш балансов из цепочки: единственная авторитетная
    /// версия состояния — та, что следует из цепочки (инвариант №1).
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
            debug!(sender = %tx.sender, nonce = tx.nonce, "Сохранение транзакции в LevelDB");
            match db.get(&key) {
                Some(_) => {
                    debug!(sender = %tx.sender, nonce = tx.nonce, "Транзакция уже существует в LevelDB, пропуск");
                    continue;
                }
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
}

/// Public entry point over `Arc<RwLock<Blockchain>>`.
///
/// Every method takes the lock itself; callers must never hold the lock across
/// a second facade call (RwLock is not re-entrant).
#[derive(Clone)]
pub struct BlockchainFacade {
    inner: Arc<RwLock<Blockchain>>,
    /// Шина событий: RBF-замена публикует `TxRejected { reason: Replaced }`
    /// для каждой вытесненной tx.
    event_bus: Arc<crate::events::EventBus>,
}

impl BlockchainFacade {
    /// Production constructor: opens/creates LevelDB for `port`.
    pub fn new(port: u16) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Blockchain::new(port))),
            event_bus: Arc::new(crate::events::EventBus::new()),
        }
    }

    /// Production constructor sharing the node's event bus (S1-P17 RBF).
    pub fn with_event_bus(port: u16, event_bus: Arc<crate::events::EventBus>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Blockchain::new(port))),
            event_bus,
        }
    }

    /// Wrap an already-built chain (tests, in-crate tooling).
    pub fn from_blockchain(bc: Blockchain) -> Self {
        Self {
            inner: Arc::new(RwLock::new(bc)),
            event_bus: Arc::new(crate::events::EventBus::new()),
        }
    }

    /// Bus this facade publishes to (RBF `TxRejected` events).
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
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        ChainSelector::chain_info(&guard.chain)
    }

    pub fn total_work(&self) -> strangecoin_core::consensus::U256 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).total_work
    }

    pub fn difficulty(&self) -> u32 {
        self.inner.read().expect(BLOCKCHAIN_LOCK).difficulty
    }

    /// Test/debug escape hatch: append without validation. Production paths
    /// must use [`Self::add_block`] or [`Self::adopt_candidate`].
    pub fn push_block_unchecked(&self, block: Block) {
        self.inner.write().expect(BLOCKCHAIN_LOCK).chain.push(block);
    }

    /// Validate + apply a single block on top of the current tip and persist.
    pub fn add_block(&self, block: Block) -> Result<(), StrangecoinError> {
        let mut guard = self.inner.write().expect(BLOCKCHAIN_LOCK);
        guard.commit_block(&block)?;
        drop(guard);
        self.save_state();
        Ok(())
    }

    /// True when `tx` is already confirmed somewhere in the chain.
    pub fn chain_contains_tx(&self, tx: &Transaction) -> bool {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        let id = strangecoin_core::serialize::txid(tx);
        guard.chain.iter().any(|b| {
            b.transactions
                .iter()
                .any(|t| strangecoin_core::serialize::txid(t) == id)
        })
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

    /// Test/debug escape hatch for direct cache manipulation (tamper tests).
    pub fn with_state_cache_mut<R>(&self, f: impl FnOnce(&mut StateCache) -> R) -> R {
        let mut guard = self.inner.write().expect(BLOCKCHAIN_LOCK);
        f(&mut guard.balances)
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

    pub fn mempool_transactions(&self) -> Vec<Transaction> {
        self.inner
            .read()
            .expect(BLOCKCHAIN_LOCK)
            .mempool
            .transactions()
    }

    /// Validate a transaction against current state and queue it in the mempool.
    ///
    /// RBF-замена вытесняет конфликтующие tx: каждая вытесненная публикует
    /// `TxRejected { reason: Replaced }` в event bus (уже вне write-lock).
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
        self.inner
            .write()
            .expect(BLOCKCHAIN_LOCK)
            .allow_grant_blocks = allow;
    }

    // -------------------------------------------------------------- operations

    pub fn mine_block(
        &self,
        progress_tx: mpsc::Sender<String>,
        shutdown: &Arc<AtomicBool>,
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

    /// Clone of the chain for the sync channel: full state, empty mempool.
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
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        serde_json::to_string(&*guard).unwrap()
    }

    /// Headers for the HEADERS response (S1-P16): headers with
    /// `index >= from_height`, ascending, at most `max` of them.
    pub fn headers_from_height(&self, from_height: u64, max: usize) -> Vec<BlockHeader> {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        guard
            .chain
            .iter()
            .filter(|b| b.index >= from_height)
            .take(max)
            .map(|b| b.header())
            .collect()
    }

    /// Full blocks for the BLOCKS response (S1-P16): one block per requested
    /// hash, in request order; unknown hashes are skipped. One hash→index
    /// pass over the chain regardless of how many hashes were asked for.
    pub fn blocks_by_hashes(&self, hashes: &[[u8; 32]], max: usize) -> Vec<Block> {
        if hashes.is_empty() {
            return Vec::new();
        }
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        let mut by_hash: HashMap<&str, &Block> = HashMap::with_capacity(guard.chain.len());
        for block in &guard.chain {
            by_hash.insert(&block.hash, block);
        }
        hashes
            .iter()
            .filter_map(|hash| by_hash.get(hex::encode(hash).as_str()))
            .take(max)
            .map(|b| (*b).clone())
            .collect()
    }

    /// Adopt a deserialized wire snapshot if it beats the current chain.
    pub fn adopt_wire(&self, snapshot: ChainSnapshot) -> Result<bool, StrangecoinError> {
        self.adopt_candidate(
            snapshot.chain,
            Some(snapshot.balances),
            snapshot.mempool_txs,
            snapshot.difficulty,
        )
    }

    // -------------------------------------------------------------- adoption

    /// Fork-choice adoption: install `candidate_chain` iff
    /// [`ChainSelector::is_better`] prefers it over the current chain and it
    /// passes full validation.
    ///
    /// On success: chain/balances/difficulty/total_work are replaced (balances
    /// from the chain reconstruction, invariant #1 — wire balances are only
    /// accepted when they agree), the mempool merges candidate + local txs
    /// skipping confirmed ones, and the result is persisted. Returns `false`
    /// when the candidate loses fork choice or is invalid; `Err` only for
    /// internal failures.
    pub fn adopt_candidate(
        &self,
        candidate_chain: Vec<Block>,
        candidate_balances: Option<HashMap<String, AccountState>>,
        candidate_mempool_txs: Vec<Transaction>,
        candidate_difficulty: u32,
    ) -> Result<bool, StrangecoinError> {
        let mut guard = self.inner.write().expect(BLOCKCHAIN_LOCK);

        if candidate_chain.len() <= 1 {
            return Ok(false);
        }
        let candidate_info = match ChainSelector::chain_info(&candidate_chain) {
            Some(info) => info,
            None => return Ok(false),
        };
        let should_adopt = match ChainSelector::chain_info(&guard.chain) {
            None => true,
            Some(current_info) => ChainSelector::is_better(&candidate_info, &current_info),
        };
        if !should_adopt {
            return Ok(false);
        }

        // Full validation of the candidate (invariant #1: the chain wins).
        let rebuilt = match StateCache::rebuild_from_chain(
            &candidate_chain,
            block_executor::now_secs(),
            guard.allow_grant_blocks,
            &guard.rules,
        ) {
            Ok(cache) => cache,
            Err(e) => {
                warn!(error = %e, "Candidate chain rejected: failed validation");
                return Ok(false);
            }
        };

        // Wire balances, when present, must agree with the reconstruction.
        if let Some(wire_balances) = candidate_balances {
            if !wire_balances.is_empty() {
                let wire_nonzero: HashMap<String, u64> = wire_balances
                    .iter()
                    .filter(|(_, acc)| acc.balance != 0)
                    .map(|(addr, acc)| (addr.clone(), acc.balance))
                    .collect();
                if wire_nonzero != rebuilt.nonzero_balances() {
                    warn!("Candidate balances disagree with chain reconstruction; rejecting");
                    return Ok(false);
                }
            }
        }

        // Merge mempool: candidate txs first, then local; skip confirmed and
        // duplicates; insert validates each against the rebuilt state.
        let local_mempool = guard.mempool.transactions();
        let mut merged = crate::mempool::Mempool::new();
        let mut merged_txids: Vec<[u8; 32]> = Vec::new();
        for tx in candidate_mempool_txs.into_iter().chain(local_mempool) {
            if chain_has_tx(&candidate_chain, &tx) {
                continue;
            }
            let txid = strangecoin_core::serialize::txid(&tx);
            if merged_txids.contains(&txid) {
                continue;
            }
            let account = rebuilt.get(&tx.sender).unwrap_or_default();
            if merged.insert(tx, &account).is_ok() {
                merged_txids.push(txid);
            }
        }

        guard.chain = candidate_chain;
        guard.balances = rebuilt;
        guard.difficulty = candidate_difficulty;
        guard.total_work = strangecoin_core::consensus::cumulative_work(&guard.chain);
        guard.mempool = merged;
        drop(guard);
        self.save_state();
        Ok(true)
    }

    // -------------------------------------------------------- escape hatches

    /// Read-lock escape hatch for compound in-crate operations.
    pub fn with_inner<R>(&self, f: impl FnOnce(&Blockchain) -> R) -> R {
        let guard = self.inner.read().expect(BLOCKCHAIN_LOCK);
        f(&guard)
    }

    /// Write-lock escape hatch for compound in-crate operations.
    pub fn with_inner_mut<R>(&self, f: impl FnOnce(&mut Blockchain) -> R) -> R {
        let mut guard = self.inner.write().expect(BLOCKCHAIN_LOCK);
        f(&mut guard)
    }
}

const BLOCKCHAIN_LOCK: &str = "blockchain lock poisoned";

fn chain_has_tx(chain: &[Block], tx: &Transaction) -> bool {
    let id = strangecoin_core::serialize::txid(tx);
    chain.iter().any(|b| {
        b.transactions
            .iter()
            .any(|t| strangecoin_core::serialize::txid(t) == id)
    })
}

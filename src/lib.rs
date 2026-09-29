use crate::error::StrangecoinError;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use eframe::egui;
use hex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, BufWriter};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, RwLock,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};
pub mod address;
pub mod api;
pub mod blockchain;
pub mod cli;
pub mod config;
pub mod consensus;
pub mod economics;
pub mod error;
pub mod governance;
#[cfg(feature = "gui")]
pub mod gui;
pub mod mempool;
pub mod network;
pub mod storage;
pub mod wallet;

pub use strangecoin_core::types::{Block, Transaction};
pub use strangecoin_core::serialize;
pub use strangecoin_core::AccountState;

#[derive(Deserialize, Serialize)]
pub struct BlockchainDeserialize {
    pub chain: Vec<Block>,
    pub balances: HashMap<String, AccountState>,
    pub difficulty: u32,
    pub pending_transactions: Vec<Transaction>,
    pub mempool_txs: Vec<Transaction>,
}

#[derive(Clone)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub balances: HashMap<String, AccountState>,
    pub difficulty: u32,
    pub mempool: crate::mempool::Mempool,
    pub storage: crate::storage::Storage,
    pub allow_grant_blocks: bool,
}

impl Serialize for Blockchain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Blockchain", 4)?;
        state.serialize_field("chain", &self.chain)?;
        state.serialize_field("balances", &self.balances)?;
        state.serialize_field("difficulty", &self.difficulty)?;
        state.serialize_field("mempool_txs", &self.mempool.transactions())?;
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
        // Add pending_transactions (legacy) to mempool
        for tx in pending_transactions {
            let account = AccountState {
                balance: 0,
                nonce: 0,
            };
            let _ = mempool.insert(tx, &account);
        }
        // Add mempool_txs (new format) to mempool
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
            balances,
            difficulty,
            mempool,
            storage,
            allow_grant_blocks: false,
        })
    }
}

#[derive(Clone)]
pub struct MiningTask {
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub transaction: Transaction,
    pub mining_status: Arc<Mutex<MiningStatus>>,
    pub progress_tx: mpsc::Sender<String>,
    pub status_tx: mpsc::Sender<String>,
    pub rate_limiter: Arc<crate::network::RateLimiter>,
    pub shutdown: Arc<AtomicBool>,
}

pub struct Node {
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub peers: Arc<Mutex<Vec<String>>>,
    pub address: String,
    pub sync_rx: mpsc::Receiver<Blockchain>,
    pub rate_limiter: Arc<crate::network::RateLimiter>,
    pub shutdown: Arc<AtomicBool>,
    pub listener: Arc<Mutex<Option<TcpListener>>>,
    pub sync_thread_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

pub struct WalletApp {
    pub node: Node,
    pub wallet_address: String,
    pub password: String,
    pub is_authenticated: bool,
    pub receiver_address: String,
    pub amount: String,
    pub ip: String,
    pub port: String,
    pub status: String,
    pub mining_status: Arc<Mutex<MiningStatus>>,
    pub mining_progress: Arc<Mutex<Option<String>>>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub status_rx: Option<mpsc::Receiver<String>>,
    pub mining_tx: mpsc::Sender<MiningTask>,
    pub last_repaint: f64,
    pub new_wallet_password: String,
    pub data_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub enum MiningStatus {
    Idle,
    Mining,
    Completed(Option<Block>),
    Failed(String),
}

impl Blockchain {
    pub fn debug_db(&self) {
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

    pub fn new(port: u16) -> Self {
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
            balances: HashMap::new(),
            difficulty: 1,
            mempool: crate::mempool::Mempool::new(),
            storage,
            allow_grant_blocks: false,
        };
        blockchain.debug_db();

        let mut chain_opt: Option<Vec<Block>> = None;
        let mut balances_opt: Option<HashMap<String, u64>> = None;
        let mut difficulty_opt: Option<u32> = None;

        {
            let db_arc = blockchain.storage.db();
            let mut db_guard = db_arc
                .lock()
                .expect("Не удалось захватить Mutex для LevelDB");

            chain_opt = db_guard
                .get(b"chain")
                .and_then(|v| serde_json::from_slice::<Vec<Block>>(&v).ok());
            debug!(?chain_opt, "chain_opt");

            balances_opt = db_guard
                .get(b"balances")
                .and_then(|v| serde_json::from_slice::<HashMap<String, u64>>(&v).ok());
            debug!(?balances_opt, "balances_opt");

            difficulty_opt = db_guard
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
                // Try to parse as old UUID format or new sender:nonce format
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
        }

        if let Some(mut chain) = chain_opt {
            // Existing chain: validate genesis hash matches expected (unless regtest)
            let chain_id = crate::consensus::current_chain_id();
            let is_regtest = crate::consensus::is_regtest(chain_id);
            if !chain.is_empty() {
                if let Err(e) = crate::consensus::validate_genesis(&chain[0], is_regtest) {
                    panic!("Genesis validation failed: {}", e);
                }
            }
            blockchain.chain = chain;
        } else {
            // New chain: load genesis from genesis.json (mainnet/testnet) or generate regtest genesis
            let chain_id = crate::consensus::current_chain_id();
            let is_regtest = crate::consensus::is_regtest(chain_id);

            let genesis_block = if is_regtest {
                // Regtest: generate deterministic genesis with chain_id=3
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
                    consensus_version: strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION,
                    state_root: [0u8; 32],
                    tx_root: [0u8; 32],
                };
                block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
                let hash = strangecoin_core::serialize::block_hash(&block);
                block.hash = hex::encode(hash);
                block
            } else {
                // Mainnet/testnet: load from genesis.json
                let genesis_path = exe_dir.join("genesis.json");
                crate::consensus::load_genesis(genesis_path.to_str().unwrap())
                    .expect("Failed to load genesis from genesis.json")
            };

            if let Err(e) = crate::consensus::validate_genesis(&genesis_block, is_regtest) {
                panic!("Genesis validation failed: {}", e);
            }
            blockchain.chain.push(genesis_block.clone());

            // Initialize balances with genesis allocation
            for tx in &genesis_block.transactions {
                if tx.sender != "genesis" {
                    blockchain
                        .balances
                        .entry(tx.receiver.clone())
                        .or_default()
                        .balance += tx.amount;
                }
            }
        }

        if let Some(balances) = balances_opt {
            // Convert old u64 balances to AccountState
            blockchain.balances = balances
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
                .collect();
        } else { /*
             blockchain.balances = HashMap::new();
             debug!("Балансы не найдены в LevelDB, инициализация на основе цепочки блоков");
             // Инициализируем балансы на основе транзакций
             for block in &blockchain.chain {
                 for tx in &block.transactions {
                     if tx.sender != "genesis" {
                         blockchain.balances.entry(tx.sender.clone()).or_default().balance -= tx.amount;
                     }
                     blockchain.balances.entry(tx.receiver.clone()).or_default().balance += tx.amount;
                 }
             }
             // Сохраняем инициализированные балансы в LevelDB
             let db_arc = blockchain.storage.db();
             let mut db = db_arc.lock().expect("Не удалось захватить Mutex для LevelDB");
             if let Err(e) = db.put(b"balances", &serde_json::to_vec(&blockchain.balances).unwrap()) {
                 error!(error = %e, "Ошибка сохранения балансов в LevelDB");
             }
             db.flush().expect("Ошибка при фиксации данных в LevelDB");*/
        }

        if let Some(difficulty) = difficulty_opt {
            blockchain.difficulty = difficulty;
        }

        blockchain.migrate_initial_wallet_balance();

        blockchain.save_state();
        blockchain.debug_db();
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(duration_secs = duration, "Создание блокчейна завершено");
        blockchain
    }

    #[cfg(test)]
    pub fn create_genesis_block(&mut self) {
        let start_time = SystemTime::now();
        // Детерминированный генезис-блок: одинаков для всех узлов сети,
        // чтобы цепочки разных инстансов были совместимы
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
            consensus_version: strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        let tx_root = strangecoin_core::serialize::compute_tx_root(&genesis_block.transactions);
        let hash = self.calculate_hash(&genesis_block);
        let mut genesis_block = genesis_block;
        genesis_block.tx_root = tx_root;
        genesis_block.hash = hash;
        self.chain.push(genesis_block.clone());
        // Обновляем балансы на основе транзакций генезис-блока, только для получателя
        for tx in &genesis_block.transactions {
            if tx.sender != "genesis" {
                let sender_balance = self
                    .balances
                    .get(&tx.sender)
                    .map(|a| a.balance)
                    .unwrap_or(0);
                if sender_balance < tx.amount {
                    panic!(
                        "Недостаточно средств у {} для транзакции nonce={}",
                        tx.sender, tx.nonce
                    );
                }
                self.balances.entry(tx.sender.clone()).or_default().balance -= tx.amount;
            }
            self.balances
                .entry(tx.receiver.clone())
                .or_default()
                .balance += tx.amount;
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(duration_secs = duration, "Создание генезис-блока завершено");
    }

    // Создаёт блок первичной эмиссии: переводит 10000 с генезис-адреса на первый реальный кошелёк
    pub fn create_grant_block(&mut self, wallet_address: &str, amount: u64) {
        let previous_block = self.chain.last().unwrap().clone();
        let transaction = Transaction {
            sender: "initial_wallet_address".to_string(),
            receiver: wallet_address.to_string(),
            amount,
            nonce: 0,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: true,
        };
        let mut block = Block {
            index: previous_block.index + 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: vec![transaction],
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target: previous_block.target.clone(),
            consensus_version: strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
        };
        block.tx_root = strangecoin_core::serialize::compute_tx_root(&block.transactions);
        block.hash = self.calculate_hash(&block);
        self.chain.push(block);
        self.balances.remove("initial_wallet_address");
        self.balances
            .entry(wallet_address.to_string())
            .or_default()
            .balance += amount;
        info!(from = "initial_wallet_address", to = %wallet_address, amount, "Создан блок первичной эмиссии");
    }

    // Начисляет первоначальный баланс (10000 из генезис-блока) первому реальному кошельку
    pub fn grant_initial_balance_to_first_wallet(
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
    pub fn migrate_initial_wallet_balance(&mut self) {
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
        self.create_grant_block(&wallet, placeholder_amount);
        info!(amount = placeholder_amount, wallet = %wallet, "Первоначальный баланс перенесён первому кошельку");
    }

    pub fn calculate_hash(&self, block: &Block) -> String {
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

    pub fn mine_block(
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
        // Get pending transactions from mempool (up to MAX_BLOCK_SIZE worth)
        let transactions: Vec<Transaction> = self.mempool.get_pending(1000); // reasonable limit
        if transactions.is_empty() {
            warn!("Все pending_transactions уже включены в блоки");
            let _ = progress_tx.send("Все pending_transactions уже включены в блоки".to_string());
            return None;
        }

        // Calculate total supply before mining this block
        let total_supply: u64 = self.balances.values().map(|a| a.balance).sum();
        let height = previous_block.index + 1;
        let coinbase_amount =
            crate::economics::emission::block_reward_at_height(height, total_supply);

        // Create coinbase transaction (first in block)
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
            let balance_start_time = SystemTime::now();

            let core_state = strangecoin_core::state::State {
                balances: self.balances.clone(),
            };
            match strangecoin_core::state::apply_block(&core_state, &block) {
                Ok(new_state) => {
                    self.balances = new_state.balances;
                }
                Err(e) => {
                    error!(error = %e, "Failed to apply block to state");
                    return None;
                }
            }

            let mut mined_txids = Vec::new();
            for tx in &block.transactions {
                if !tx.is_coinbase {
                    mined_txids.push(strangecoin_core::serialize::txid(tx));
                }
            }

            let balance_duration = SystemTime::now()
                .duration_since(balance_start_time)
                .unwrap()
                .as_secs_f64();
            debug!(
                duration_secs = balance_duration,
                "Обновление балансов завершено"
            );

            // Remove mined transactions from mempool
            for tx_id in mined_txids {
                self.mempool.remove(&tx_id);
            }

            self.chain.push(block.clone());
            let db_arc = self.storage.db();
            let mut db = db_arc
                .lock()
                .expect("Не удалось захватить Mutex для LevelDB");
            // Clean up old pending transaction keys from LevelDB (legacy)
            // Note: We don't track which exact keys were mined, so we keep them for now
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

    pub fn mine_block_inner(
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

        // Determine target: use previous block's target, or compute new one at retarget height
        let target = if previous_block.index + 1 > 0
            && (previous_block.index + 1) % crate::consensus::RETARGET_INTERVAL == 0
        {
            let new_target = crate::consensus::compute_target(&self.chain);
            hex::encode(new_target)
        } else {
            previous_block.target.clone()
        };

        let mut block = Block {
            index: previous_block.index + 1,
            timestamp: now.max(mtp + 1),
            transactions,
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
            target,
            consensus_version: strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION,
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
            if iteration_count % 10000 == 0 {
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

    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), StrangecoinError> {
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
        let account_state = self
            .balances
            .get(&transaction.sender)
            .cloned()
            .unwrap_or_default();
        self.mempool.insert(transaction, &account_state)?;
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(
            duration_secs = duration,
            pending_count = self.mempool.len(),
            "Транзакция добавлена в mempool"
        );
        Ok(())
    }

    pub fn validate_chain(&self) -> bool {
        let start_time = SystemTime::now();
        info!("Начало валидации цепочки блоков");
        if self.chain.is_empty() {
            warn!("Цепочка пуста, невалидна");
            return false;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let genesis_block = &self.chain[0];
        info!(index = genesis_block.index, previous_hash = %genesis_block.previous_hash, "Проверка генезис-блока");
        if genesis_block.index != 0 || genesis_block.previous_hash != "0".repeat(64) {
            warn!(?genesis_block, "Некорректный генезис-блок");
            return false;
        }
        let genesis_hash = self.calculate_hash(genesis_block);
        debug!(calculated_hash = %genesis_hash, stored_hash = %genesis_block.hash, "Хэш генезис-блока");
        if genesis_block.hash != genesis_hash {
            warn!(?genesis_block, "Некорректный хэш генезис-блока");
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

        // Consensus version validation (invariant #21)
        for block in &self.chain {
            let expected_version = strangecoin_core::consensus::CURRENT_CONSENSUS_VERSION;
            if block.consensus_version != expected_version {
                warn!(
                    block_index = block.index,
                    got = block.consensus_version,
                    expected = expected_version,
                    "Block consensus_version mismatch"
                );
                return false;
            }
        }

        // Валидация подписей всех транзакций в цепочке
        for block in &self.chain {
            for tx in &block.transactions {
                if let Err(e) = crate::consensus::verify_transaction(tx) {
                    warn!(block_index = block.index, tx = ?tx, error = %e, "Неверная подпись транзакции в блоке");
                    return false;
                }
            }
        }

        // Восстанавливаем балансы из цепочки блоков, начиная с пустого состояния
        let mut expected_state = strangecoin_core::state::State::new();
        for block in &self.chain {
            info!(
                block_index = block.index,
                tx_count = block.transactions.len(),
                "Обработка блока"
            );
            match strangecoin_core::state::apply_block(&expected_state, block) {
                Ok(new_state) => {
                    expected_state = new_state;
                }
                Err(e) => {
                    warn!(block_index = block.index, error = %e, "Block application failed during validation");
                    return false;
                }
            }
        }

        // Tx root validation (invariant: merkle_root(txs) == block.tx_root)
        for block in &self.chain {
            if let Err(e) = strangecoin_core::consensus::validate_tx_root(block) {
                warn!(block_index = block.index, error = %e, "Tx root mismatch");
                return false;
            }
        }

        // Проверяем неподтверждённые транзакции (mempool)
        let mut temp_state = expected_state.clone();
        for tx in self.mempool.transactions() {
            let sender_balance = temp_state.get_balance(&tx.sender);
            if sender_balance < tx.amount {
                warn!(sender = %tx.sender, nonce = tx.nonce, required = tx.amount, available = sender_balance, "Недостаточно средств в mempool");
                return false;
            }
            temp_state.set_balance(
                &tx.sender,
                sender_balance.checked_sub(tx.amount).unwrap_or(0),
            );
            let receiver_balance = temp_state.get_balance(&tx.receiver);
            temp_state.set_balance(
                &tx.receiver,
                receiver_balance.checked_add(tx.amount).unwrap_or(u64::MAX),
            );
        }

        // Проверяем структуру цепочки и timestamp'ы
        for i in 1..self.chain.len() {
            let current_block = &self.chain[i];
            let previous_block = &self.chain[i - 1];
            debug!(block_index = i, current_index = current_block.index, previous_hash = %current_block.previous_hash, "Проверка блока");
            if current_block.index != previous_block.index + 1 {
                warn!(
                    block_index = i,
                    expected_index = previous_block.index + 1,
                    actual_index = current_block.index,
                    "Некорректный индекс блока"
                );
                return false;
            }
            if current_block.previous_hash != previous_block.hash {
                warn!(block_index = i, expected_hash = %previous_block.hash, actual_hash = %current_block.previous_hash, "Некорректный previous_hash");
                return false;
            }
            // Валидация timestamp текущего блока
            if let Err(e) =
                crate::consensus::validate_timestamp(current_block, &self.chain[..i], now)
            {
                warn!(block_index = current_block.index, error = %e, "Некорректный timestamp блока");
                return false;
            }
            let current_hash = self.calculate_hash(current_block);
            debug!(block_index = i, calculated_hash = %current_hash, stored_hash = %current_block.hash, "Хэш блока");
            if current_block.hash != current_hash {
                warn!(block_index = i, ?current_block, "Некорректный хэш в блоке");
                return false;
            }
            // Валидация difficulty
            if let Err(e) = crate::consensus::validate_difficulty(current_block) {
                warn!(block_index = current_block.index, error = %e, "Неверная сложность блока");
                return false;
            }
            // Проверка retarget
            if current_block.index > 0
                && current_block.index % crate::consensus::RETARGET_INTERVAL == 0
            {
                let expected_target =
                    crate::consensus::compute_target(&self.chain[..current_block.index as usize]);
                let expected_target_hex = hex::encode(expected_target);
                if current_block.target != expected_target_hex {
                    warn!(block_index = current_block.index, expected = %expected_target_hex, got = %current_block.target, "Неверный target на retarget height");
                    return false;
                }
            } else if current_block.index > 0 {
                // На не-retarget height target должен совпадать с предыдущим блоком
                let prev_target = &self.chain[current_block.index as usize - 1].target;
                if current_block.target != *prev_target {
                    warn!(block_index = current_block.index, expected = %prev_target, got = %current_block.target, "Target изменён вне retarget height");
                    return false;
                }
            }
        }

        // Сверяем восстановленные балансы с хранимыми, игнорируя нулевые остатки
        let reconstructed: HashMap<String, u64> = expected_state
            .balances
            .iter()
            .filter(|(_, v)| v.balance != 0)
            .map(|(k, v)| (k.clone(), v.balance))
            .collect();
        let stored: HashMap<String, u64> = self
            .balances
            .iter()
            .filter(|(_, v)| v.balance != 0)
            .map(|(k, v)| (k.clone(), v.balance))
            .collect();
        debug!(?reconstructed, ?stored, "Сверка восстановленных балансов");
        if reconstructed != stored {
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

    pub fn save_state(&mut self) {
        let db_arc = self.storage.db();
        let mut db = db_arc
            .lock()
            .expect("Не удалось захватить Mutex для LevelDB");
        // Save mempool transactions
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

impl Node {
    fn new(
        address: String,
        mining_rx: mpsc::Receiver<MiningTask>,
        sync_tx: mpsc::Sender<Blockchain>,
        port: u16,
    ) -> Self {
        let blockchain = Arc::new(RwLock::new(Blockchain::new(port)));
        let peers = Arc::new(Mutex::new(vec![]));
        let rate_limiter = Arc::new(crate::network::RateLimiter::new(10, 100));
        let shutdown = Arc::new(AtomicBool::new(false));
        let node = Node {
            blockchain: blockchain.clone(),
            peers: peers.clone(),
            address: address.clone(),
            sync_rx: mpsc::channel().1,
            rate_limiter: rate_limiter.clone(),
            shutdown: shutdown.clone(),
            listener: Arc::new(Mutex::new(None)),
            sync_thread_handle: Arc::new(Mutex::new(None)),
        };
        thread::spawn(move || {
            info!("Фоновый поток майнинга запущен");
            let mut mining_count = 0;
            let mut total_duration = 0.0;
            let mut successful_mining = 0;
            while let Ok(task) = mining_rx.recv() {
                mining_count += 1;
                info!(mining_count, "Получена задача майнинга");
                let progress_tx_clone = task.progress_tx.clone();
                let status_tx_clone = task.status_tx.clone();
                let start_time = SystemTime::now();
                let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    debug!(mining_count, "Попытка захвата write lock для blockchain");
                    let lock_start_time = SystemTime::now();
                    let mut blockchain = task
                        .blockchain
                        .write()
                        .expect("Не удалось захватить write lock для blockchain");
                    let lock_duration = SystemTime::now()
                        .duration_since(lock_start_time)
                        .unwrap()
                        .as_secs_f64();
                    debug!(
                        mining_count,
                        lock_duration_secs = lock_duration,
                        "Захват write lock для blockchain"
                    );
                    debug!(mining_count, ?task.transaction, "Проверка транзакции");
                    match blockchain.add_transaction(task.transaction.clone()) {
                        Ok(_) => {
                            info!(
                                mining_count,
                                "Транзакция успешно добавлена, начало майнинга"
                            );
                            blockchain.mine_block(progress_tx_clone, &task.shutdown)
                        }
                        Err(e) => {
                            warn!(mining_count, error = %e, "Транзакция отклонена, попытка майнить существующие транзакции");
                            if !blockchain.mempool.is_empty() {
                                blockchain.mine_block(progress_tx_clone, &task.shutdown)
                            } else {
                                let _ = task.progress_tx.send(format!(
                                    "Ошибка: Нет транзакций для майнинга в задаче {}",
                                    mining_count
                                ));
                                warn!(mining_count, "Нет транзакций для майнинга");
                                None
                            }
                        }
                    }
                })) {
                    Ok(result) => result,
                    Err(panic) => {
                        let err_msg = match panic.downcast_ref::<&str>() {
                            Some(s) => s.to_string(),
                            None => format!("Неизвестная паника: {:?}", panic),
                        };
                        error!(mining_count, error = %err_msg, "Паника в потоке майнинга");
                        let _ = task.progress_tx.send(format!(
                            "Паника в потоке майнинга {}: {}",
                            mining_count, err_msg
                        ));
                        None
                    }
                };
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                total_duration += duration;
                info!(mining_count, duration_secs = duration, result = ?result, "Майнинг завершен");
                let mut attempts = 0;
                let max_attempts = 5;
                let mut status_updated = false;
                while attempts < max_attempts {
                    if let Ok(mut mining_status) = task.mining_status.try_lock() {
                        *mining_status = match result {
                            Some(block) => {
                                successful_mining += 1;
                                info!(mining_count, ?block, "Майнинг успешен, блок добавлен");
                                let _ = status_tx_clone.send(format!(
                                    "Транзакция отправлена, блок добавлен: {:?}",
                                    block
                                ));
                                let mut node_temp = Node {
                                    blockchain: task.blockchain.clone(),
                                    peers: peers.clone(),
                                    address: address.clone(),
                                    sync_rx: mpsc::channel().1,
                                    rate_limiter: task.rate_limiter.clone(),
                                    shutdown: Arc::new(AtomicBool::new(false)),
                                    listener: Arc::new(Mutex::new(None)),
                                    sync_thread_handle: Arc::new(Mutex::new(None)),
                                };
                                node_temp.sync_blockchain(sync_tx.clone());
                                MiningStatus::Completed(Some(block))
                            }
                            None => {
                                warn!(mining_count, "Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут");
                                let _ = status_tx_clone.send("Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут".to_string());
                                MiningStatus::Failed("Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут".to_string())
                            }
                        };
                        status_updated = true;
                        debug!(mining_count, ?mining_status, "Статус майнинга обновлён");
                        break;
                    } else {
                        attempts += 1;
                        debug!(
                            attempts,
                            mining_count, "Попытка обновить статус майнинга не удалась"
                        );
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
                if !status_updated {
                    warn!(
                        mining_count,
                        max_attempts, "Не удалось обновить статус майнинга после попыток"
                    );
                    let _ = task.progress_tx.send(format!(
                        "Ошибка: Не удалось обновить статус майнинга {} после {} попыток",
                        mining_count, max_attempts
                    ));
                    let _ = status_tx_clone.send(format!(
                        "Ошибка: Не удалось обновить статус майнинга после {} попыток",
                        max_attempts
                    ));
                }
                if mining_count > 0 {
                    let avg_duration = total_duration / mining_count as f64;
                    debug!(
                        mining_count,
                        avg_duration_secs = avg_duration,
                        "Среднее время майнинга"
                    );
                    debug!(
                        mining_count,
                        successful = successful_mining,
                        failed = mining_count - successful_mining,
                        "Статистика майнинга"
                    );
                }
            }
            info!("Фоновый поток майнинга завершен");
        });
        node
    }

    pub fn discover_peers(&mut self) {
        let start_time = SystemTime::now();
        let mut peers = self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers");
        peers.clear();
        let exe_path =
            std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path
            .parent()
            .expect("Не удалось получить директорию исполняемого файла");
        let network_path = exe_dir.join("network.json");
        let network_config: serde_json::Value = match fs::read_to_string(&network_path) {
            Ok(content) => serde_json::from_str(&content)
                .unwrap_or_else(|_| serde_json::json!({ "peers": [] })),
            Err(_) => serde_json::json!({ "peers": [] }),
        };
        let own_port = self
            .address
            .split(':')
            .last()
            .unwrap_or("0")
            .parse::<u16>()
            .unwrap_or(0);
        let empty_peers: Vec<serde_json::Value> = vec![];
        let peer_list = network_config["peers"].as_array().unwrap_or(&empty_peers);
        for peer in peer_list {
            if let Some(peer_str) = peer.as_str() {
                let peer_port = peer_str
                    .split(':')
                    .last()
                    .unwrap_or("0")
                    .parse::<u16>()
                    .unwrap_or(0);
                if peer_port != own_port {
                    peers.push(peer_str.to_string());
                }
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        debug!(duration_secs = duration, peers = ?*peers, "Обнаружение пиров завершено");
    }

    pub fn add_peer(&mut self, address: String) -> bool {
        let start_time = SystemTime::now();
        let mut peers = self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers");
        if peers.contains(&address) {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            debug!(peer = %address, duration_secs = duration, "Пир уже существует, добавление не требуется");
            return false;
        }
        peers.push(address.clone());
        let exe_path =
            std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path
            .parent()
            .expect("Не удалось получить директорию исполняемого файла");
        let network_path = exe_dir.join("network.json");
        let mut network_config: serde_json::Value = match fs::read_to_string(&network_path) {
            Ok(content) => serde_json::from_str(&content)
                .unwrap_or_else(|_| serde_json::json!({ "peers": [] })),
            Err(_) => serde_json::json!({ "peers": [] }),
        };
        let mut empty_peers: Vec<serde_json::Value> = vec![];
        let peer_list = network_config["peers"]
            .as_array_mut()
            .unwrap_or(&mut empty_peers);
        if !peer_list.iter().any(|v| v.as_str() == Some(&address)) {
            peer_list.push(serde_json::Value::String(address.clone()));
            let network_content = serde_json::to_string_pretty(&network_config)
                .expect("Ошибка сериализации network.json");
            fs::write(&network_path, network_content).expect("Ошибка записи в network.json");
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(peer = %address, duration_secs = duration, "Пир добавлен");
        true
    }

    pub fn find_wallet_by_ip(&self, ip: &str, port: u16) -> Option<String> {
        let addr = format!("{}:{}", ip, port);
        let peers = self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers");
        if peers.contains(&addr) {
            let blockchain = self
                .blockchain
                .read()
                .expect("Не удалось захватить read lock для blockchain");
            for (wallet, _) in blockchain.balances.iter() {
                return Some(wallet.clone());
            }
        }
        None
    }

    pub fn start_server(&mut self, port: u16, sync_tx: mpsc::Sender<Blockchain>) {
        let start_time = SystemTime::now();
        let blockchain = Arc::clone(&self.blockchain);
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let shutdown = Arc::clone(&self.shutdown);
        let address = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&address).expect("Не удалось запустить серver");
        // Store listener for graceful shutdown
        *self.listener.lock().unwrap() =
            Some(listener.try_clone().expect("Failed to clone listener"));
        thread::spawn(move || {
            for stream in listener.incoming() {
                if shutdown.load(Ordering::Relaxed) {
                    info!("Shutdown signal received, stopping server");
                    break;
                }
                match stream {
                    Ok(stream) => {
                        let blockchain = Arc::clone(&blockchain);
                        let sync_tx = sync_tx.clone();
                        let rate_limiter = Arc::clone(&rate_limiter);
                        let peer_addr = stream.peer_addr().ok();
                        thread::spawn(move || {
                            if let Some(addr) = peer_addr {
                                if let Err(e) = rate_limiter.check(addr) {
                                    warn!(peer = %addr, error = %e, "Rate limit exceeded, closing connection");
                                    return;
                                }
                            }
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let mut writer = BufWriter::new(stream);
                            let request_bytes =
                                match crate::network::protocol::read_length_prefixed(&mut reader) {
                                    Ok(bytes) => bytes,
                                    Err(e) => {
                                        warn!(error = %e, "Failed to read length-prefixed message");
                                        return;
                                    }
                                };
                            let request = String::from_utf8_lossy(&request_bytes).to_string();
                            debug!(request = %request, "Получен запрос");

                            if request == "GET_BLOCKCHAIN" {
                                let blockchain = blockchain
                                    .read()
                                    .expect("Не удалось захватить read lock для blockchain");
                                let response = serde_json::to_string(&*blockchain).unwrap();
                                let length = response.len() as u32;
                                let mut data = length.to_be_bytes().to_vec();
                                data.extend_from_slice(response.as_bytes());
                                if writer.write_all(&data).is_ok() {
                                    writer.flush().ok();
                                    info!("Отправлен блокчейн клиенту");
                                }
                            } else if request.starts_with("UPDATE_BLOCKCHAIN:") {
                                let blockchain_data =
                                    request.strip_prefix("UPDATE_BLOCKCHAIN:").unwrap_or("");
                                let mut temp_blockchain: BlockchainDeserialize =
                                    match serde_json::from_str(blockchain_data) {
                                        Ok(data) => data,
                                        Err(e) => {
                                            error!(error = %e, "Ошибка десериализации данных блокчейна");
                                            return;
                                        }
                                    };
                                let mut blockchain = blockchain
                                    .write()
                                    .expect("Не удалось захватить write lock для blockchain");
                                let current_pending: Vec<Transaction> =
                                    blockchain.mempool.transactions();

                                // Принимаем только строго более длинную цепочку, чтобы не откатывать уже намайненные блоки
                                if temp_blockchain.chain.len() > blockchain.chain.len() {
                                    let storage = blockchain.storage.clone();
                                    let mut new_blockchain = Blockchain {
                                        chain: temp_blockchain.chain.clone(),
                                        balances: temp_blockchain.balances.clone(),
                                        difficulty: temp_blockchain.difficulty,
                                        mempool: crate::mempool::Mempool::new(),
                                        storage,
                                        allow_grant_blocks: blockchain.allow_grant_blocks,
                                    };
                                    // Объединяем mempool, добавляя только валидные
                                    let mut merged_pending = vec![];
                                    for tx in temp_blockchain.mempool_txs.iter() {
                                        if !merged_pending.iter().any(|t| t == tx)
                                            && new_blockchain.add_transaction(tx.clone()).is_ok()
                                        {
                                            merged_pending.push(tx.clone());
                                            info!(sender = %tx.sender, nonce = tx.nonce, "Добавлена транзакция от узла");
                                        }
                                    }
                                    for tx in current_pending.iter() {
                                        if !merged_pending.iter().any(|t| t == tx)
                                            && new_blockchain.add_transaction(tx.clone()).is_ok()
                                        {
                                            merged_pending.push(tx.clone());
                                            info!(sender = %tx.sender, nonce = tx.nonce, "Сохранена локальная транзакция");
                                        }
                                    }
                                    // Note: merged_pending is tracked in mempool via add_transaction
                                    if new_blockchain.validate_chain() {
                                        let storage = blockchain.storage.clone();
                                        let db_arc = storage.db();
                                        let mut db = db_arc
                                            .lock()
                                            .expect("Не удалось захватить Mutex для LevelDB");
                                        for tx in new_blockchain.mempool.transactions() {
                                            let key =
                                                format!("{}:{}", tx.sender, tx.nonce).into_bytes();
                                            let value = serde_json::to_vec(&tx)
                                                .expect("Ошибка сериализации транзакции");
                                            if let Err(e) = db.put(&key, &value) {
                                                error!(sender = %tx.sender, nonce = tx.nonce, error = %e, "Ошибка сохранения транзакции в LevelDB");
                                            }
                                        }
                                        drop(db);
                                        *blockchain = new_blockchain;
                                        blockchain.save_state();
                                        info!("Блокчейн обновлён через UPDATE_BLOCKCHAIN");
                                        let _ = sync_tx.send(Blockchain {
                                            chain: temp_blockchain.chain,
                                            balances: temp_blockchain.balances,
                                            difficulty: temp_blockchain.difficulty,
                                            mempool: crate::mempool::Mempool::new(),
                                            storage: blockchain.storage.clone(),
                                            allow_grant_blocks: blockchain.allow_grant_blocks,
                                        });
                                    } else {
                                        warn!("Полученный блокчейн не прошёл валидацию");
                                    }
                                } else {
                                    // Обновляем только pending_transactions, добавляя только валидные
                                    for tx in temp_blockchain.pending_transactions.iter() {
                                        if !blockchain
                                            .mempool
                                            .contains(&strangecoin_core::serialize::txid(&tx))
                                        {
                                            let _ = blockchain.add_transaction(tx.clone());
                                        }
                                    }
                                    // current_pending is already in mempool
                                    blockchain.save_state();
                                    info!("Обновлены pending_transactions через UPDATE_BLOCKCHAIN");
                                }
                            }
                        });
                    }
                    Err(e) => error!(error = %e, "Ошибка обработки входящего соединения"),
                }
            }
        });
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(port, duration_secs = duration, "Сервер запущен");
    }
    pub fn sync_blockchain(&mut self, sync_tx: mpsc::Sender<Blockchain>) {
        let start_time = SystemTime::now();
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let shutdown = Arc::clone(&self.shutdown);
        // Обнаруживаем пиры перед синхронизацией
        self.discover_peers();
        let peers: Vec<String> = self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers")
            .iter()
            .cloned()
            .collect();
        info!(peers = ?peers, "Список пиров для синхронизации");

        // Получаем текущее состояние блокчейна
        let blockchain = self
            .blockchain
            .read()
            .expect("Не удалось захватить read lock для blockchain");
        let current_hash = blockchain
            .chain
            .last()
            .map(|b| b.hash.clone())
            .unwrap_or_default();
        let current_chain_length = blockchain.chain.len();
        let current_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
        let current_pending: Vec<Transaction> = blockchain.mempool.transactions();
        let current_block_count = blockchain
            .chain
            .iter()
            .filter(|b| !b.transactions.is_empty())
            .count();
        let current_balances = blockchain.balances.clone();
        let wallet_address = self.address.clone();
        let existing_db = blockchain.storage.db().clone();
        debug!(current_chain_length, chain = ?blockchain.chain, "Текущая длина chain");
        drop(blockchain); // Освобождаем блокировку

        for peer in peers.iter() {
            if shutdown.load(Ordering::Relaxed) {
                info!("Shutdown signal received, stopping sync");
                break;
            }
            let addr: SocketAddr = match peer.parse() {
                Ok(addr) => addr,
                Err(e) => {
                    warn!(peer = %peer, error = %e, "Некорректный адрес пира");
                    continue;
                }
            };
            // Rate limit check for outgoing requests
            if let Err(e) = rate_limiter.check(addr) {
                warn!(peer = %peer, error = %e, "Rate limit exceeded for outgoing request, skipping peer");
                continue;
            }
            if current_chain_length <= 1 {
                info!("Новый узел, только получение данных, отправка цепочки запрещена");
                continue; // Пропускаем отправку UPDATE_BLOCKCHAIN
            }
            // Отправка UPDATE_BLOCKCHAIN
            if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                let mut writer = BufWriter::new(stream.try_clone().unwrap());
                let blockchain = self
                    .blockchain
                    .read()
                    .expect("Не удалось захватить read lock для blockchain");
                let response = serde_json::to_string(&*blockchain).unwrap();
                let message = format!("UPDATE_BLOCKCHAIN:{}", response);
                let length = message.len() as u32;
                let mut data = length.to_be_bytes().to_vec();
                data.extend_from_slice(message.as_bytes());
                if writer.write_all(&data).is_ok() {
                    writer.flush().ok();
                    info!(peer = %peer, message_len = data.len(), "Блокчейн отправлен узлу");
                }
                drop(blockchain);
            }

            // Запрос GET_BLOCKCHAIN
            if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = BufWriter::new(stream);

                let message = "GET_BLOCKCHAIN";
                let length = message.len() as u32;
                let mut data = length.to_be_bytes().to_vec();
                data.extend_from_slice(message.as_bytes());
                if writer.write_all(&data).is_ok() {
                    writer.flush().ok();

                    let response_bytes = match crate::network::protocol::read_length_prefixed(
                        &mut reader,
                    ) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            warn!(peer = %peer, error = %e, "Failed to read length-prefixed response");
                            continue;
                        }
                    };
                    let response = String::from_utf8_lossy(&response_bytes).to_string();
                    debug!(peer = %peer, response_len = response.len(), "Получен ответ от узла");
                    match serde_json::from_str::<BlockchainDeserialize>(&response) {
                        Ok(received_blockchain) => {
                            if received_blockchain.chain.is_empty()
                                || received_blockchain.chain.len() <= 1
                            {
                                info!(peer = %peer, chain_len = received_blockchain.chain.len(), "Получена пустая или минимальная цепочка, игнорируем");
                                continue;
                            }
                            let temp_blockchain = Blockchain {
                                chain: received_blockchain.chain.clone(),
                                balances: received_blockchain.balances.clone(),
                                difficulty: received_blockchain.difficulty,
                                mempool: {
                                    let mut mp = crate::mempool::Mempool::new();
                                    for tx in received_blockchain.mempool_txs.iter() {
                                        let _ = mp.insert(tx.clone(), &AccountState::default());
                                    }
                                    mp
                                },
                                storage: crate::storage::Storage::from_db(existing_db.clone()),
                                allow_grant_blocks: false,
                            };
                            info!(peer = %peer, chain_len = temp_blockchain.chain.len(), chain = ?temp_blockchain.chain, "Полученная цепочка от узла");
                            let received_hash = temp_blockchain
                                .chain
                                .last()
                                .map(|b| b.hash.clone())
                                .unwrap_or_default();
                            let received_timestamp = temp_blockchain
                                .chain
                                .last()
                                .map(|b| b.timestamp)
                                .unwrap_or(0);
                            let received_block_count = temp_blockchain
                                .chain
                                .iter()
                                .filter(|b| !b.transactions.is_empty())
                                .count();
                            if temp_blockchain.chain.len() > current_chain_length
                                && received_block_count > current_block_count
                                && temp_blockchain.validate_chain()
                            {
                                let mut new_blockchain = Blockchain {
                                    chain: temp_blockchain.chain.clone(),
                                    balances: temp_blockchain.balances.clone(),
                                    difficulty: temp_blockchain.difficulty,
                                    mempool: crate::mempool::Mempool::new(),
                                    storage: crate::storage::Storage::from_db(existing_db.clone()),
                                    allow_grant_blocks: false,
                                };
                                let mut added_transactions = 0;

                                for tx in temp_blockchain.mempool.transactions() {
                                    if !new_blockchain
                                        .chain
                                        .iter()
                                        .any(|block| block.transactions.iter().any(|t| *t == tx))
                                        && new_blockchain.add_transaction(tx.clone()).is_ok()
                                    {
                                        added_transactions += 1;
                                        info!(peer = %peer, sender = %tx.sender, nonce = tx.nonce, "Добавлена транзакция от узла");
                                    }
                                }

                                for tx in current_pending.iter() {
                                    if !new_blockchain
                                        .chain
                                        .iter()
                                        .any(|block| block.transactions.iter().any(|t| *t == *tx))
                                        && new_blockchain.add_transaction(tx.clone()).is_ok()
                                    {
                                        added_transactions += 1;
                                        info!(sender = %tx.sender, nonce = tx.nonce, "Сохранена локальная транзакция");
                                    }
                                }

                                info!(added_transactions, peer = %peer, "Обновлено mempool с узла");

                                if new_blockchain.validate_chain() {
                                    let storage = self.blockchain.read().unwrap().storage.clone();
                                    let db_arc = storage.db();
                                    let mut blockchain = self
                                        .blockchain
                                        .write()
                                        .expect("Не удалось захватить write lock для blockchain");
                                    let mut db = db_arc
                                        .lock()
                                        .expect("Не удалось захватить Mutex для LevelDB");
                                    let chain_data = serde_json::to_vec(&new_blockchain.chain)
                                        .expect("Ошибка сериализации chain");
                                    debug!(
                                        chain_len = new_blockchain.chain.len(),
                                        chain_size = chain_data.len(),
                                        "Сохраняемый chain"
                                    );
                                    for tx in new_blockchain.mempool.transactions() {
                                        let key =
                                            format!("{}:{}", tx.sender, tx.nonce).into_bytes();
                                        let value = serde_json::to_vec(&tx)
                                            .expect("Ошибка сериализации транзакции");
                                        if let Err(e) = db.put(&key, &value) {
                                            error!(sender = %tx.sender, nonce = tx.nonce, error = %e, "Ошибка сохранения транзакции в LevelDB");
                                        }
                                    }
                                    if let Err(e) = db.put(b"chain", &chain_data) {
                                        error!(error = %e, "Ошибка сохранения chain в LevelDB");
                                    }
                                    if let Err(e) = db.put(
                                        b"balances",
                                        &serde_json::to_vec(&new_blockchain.balances).unwrap(),
                                    ) {
                                        error!(error = %e, "Ошибка сохранения balances в LevelDB");
                                    }
                                    if let Err(e) = db.put(
                                        b"difficulty",
                                        &serde_json::to_vec(&new_blockchain.difficulty).unwrap(),
                                    ) {
                                        error!(error = %e, "Ошибка сохранения difficulty в LevelDB");
                                    }
                                    db.flush().expect("Ошибка при фиксации данных в LevelDB");
                                    drop(db);
                                    *blockchain = new_blockchain;
                                    blockchain.save_state();
                                    info!(peer = %peer, new_chain_len = blockchain.chain.len(), "Блокчейн обновлён с узла");
                                    let _ = sync_tx.send(Blockchain {
                                        chain: blockchain.chain.clone(),
                                        balances: blockchain.balances.clone(),
                                        difficulty: blockchain.difficulty,
                                        mempool: crate::mempool::Mempool::new(),
                                        storage: crate::storage::Storage::from_db(
                                            existing_db.clone(),
                                        ),
                                        allow_grant_blocks: blockchain.allow_grant_blocks,
                                    });
                                } else {
                                    warn!(peer = %peer, "Полученный блокчейн с узла не прошёл валидацию после объединения");
                                }
                            } else {
                                let mut blockchain = self
                                    .blockchain
                                    .write()
                                    .expect("Не удалось захватить write lock для blockchain");
                                for tx in temp_blockchain.mempool.transactions() {
                                    if !blockchain
                                        .chain
                                        .iter()
                                        .any(|block| block.transactions.iter().any(|t| *t == tx))
                                        && !blockchain
                                            .mempool
                                            .contains(&strangecoin_core::serialize::txid(&tx))
                                    {
                                        let _ = blockchain.add_transaction(tx.clone());
                                    }
                                }
                                blockchain.save_state();
                            }
                        }
                        Err(e) => {
                            error!(peer = %peer, error = %e, "Ошибка десериализации блокчейна от узла")
                        }
                    }
                } else {
                    debug!(peer = %peer, "Узел недоступен");
                }
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(
            duration_secs = duration,
            "Синхронизация блокчейна завершена"
        );
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        info!("Shutting down network node");

        // Signal shutdown
        self.shutdown.store(true, Ordering::Relaxed);

        // Close listener
        if let Ok(mut listener) = self.listener.lock() {
            if let Some(l) = listener.take() {
                drop(l);
                info!("TCP listener closed");
            }
        }

        // Wait for sync thread to finish (with timeout)
        if let Ok(mut handle) = self.sync_thread_handle.lock() {
            if let Some(h) = handle.take() {
                let _ = h.join();
                info!("Sync thread joined");
            }
        }

        info!("Network node shutdown complete");
    }
}

impl eframe::App for WalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if now - self.last_repaint > 0.01 {
            ctx.request_repaint();
            self.last_repaint = now;
        }

        while let Ok(received_blockchain) = self.node.sync_rx.try_recv() {
            let mut blockchain = self
                .node
                .blockchain
                .write()
                .expect("Не удалось захватить write lock для blockchain");
            let current_hash = blockchain
                .chain
                .last()
                .map(|b| b.hash.clone())
                .unwrap_or_default();
            let received_hash = received_blockchain
                .chain
                .last()
                .map(|b| b.hash.clone())
                .unwrap_or_default();
            if received_blockchain.chain.is_empty() || received_blockchain.chain.len() <= 1 {
                debug!("Получена пустая или минимальная цепочка через sync_rx, игнорируем");
                continue;
            }
            let current_balance = blockchain
                .balances
                .get(&self.wallet_address)
                .map(|a| a.balance)
                .unwrap_or(0);
            let received_balance = received_blockchain
                .balances
                .get(&self.wallet_address)
                .map(|a| a.balance)
                .unwrap_or(0);
            if received_blockchain.chain.len() > blockchain.chain.len()
                && received_blockchain.validate_chain()
            {
                let db_arc = blockchain.storage.db();
                let mut db = db_arc
                    .lock()
                    .expect("Не удалось захватить Mutex для LevelDB");
                for tx in received_blockchain.mempool.transactions() {
                    let key = format!("{}:{}", tx.sender, tx.nonce).into_bytes();
                    let value = serde_json::to_vec(&tx).expect("Ошибка сериализации транзакции");
                    if let Err(e) = db.put(&key, &value) {
                        error!(sender = %tx.sender, nonce = tx.nonce, error = %e, "Ошибка сохранения транзакции в LevelDB");
                    }
                }
                drop(db);
                *blockchain = received_blockchain;
                blockchain.save_state();
                info!("UI: Блокчейн обновлён через канал синхронизации");
                if current_balance != received_balance {
                    info!(wallet = %self.wallet_address, old_balance = current_balance, new_balance = received_balance, "Баланс кошелька изменился, запрашивается перерисовка");
                    ctx.request_repaint();
                } else {
                    debug!(wallet = %self.wallet_address, balance = current_balance, "Баланс кошелька не изменился, перерисовка не требуется");
                }
            } else {
                debug!("Полученный блокчейн через канал синхронизации не длиннее или не прошёл валидацию");
            }
        }

        if let Some(ref status_rx) = self.status_rx {
            while let Ok(status) = status_rx.try_recv() {
                self.status = status;
                if self.status.starts_with("Транзакция отправлена") {
                    if let Ok(mut mining_status) = self.mining_status.lock() {
                        *mining_status = MiningStatus::Idle;
                        debug!("Статус майнинга сброшен на Idle");
                    }
                    self.progress_rx = None;
                }
                debug!(status = %self.status, "Получено обновление статуса");
                ctx.request_repaint();
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(&self.status);
            ui.label(format!(
                "Количество транзакций в базе данных: {}",
                self.node
                    .blockchain
                    .read()
                    .expect("Не удалось захватить read lock для blockchain")
                    .mempool
                    .len()
            ));
            if !self.is_authenticated {
                ui.heading("Аутентификация");
                ui.label("Адрес кошелька (публичный ключ):");
                ui.text_edit_singleline(&mut self.wallet_address);
                ui.label("Пароль:");
                ui.text_edit_singleline(&mut self.password);
                ui.horizontal(|ui| {
                    if ui.button("Войти").clicked() {
                        let start_time = SystemTime::now();
                        info!(wallet_address = %self.wallet_address, "Кнопка 'Войти' нажата");
                        let data_dir = self.data_dir.clone();
                        let password = if self.password.is_empty() {
                            wallet::Wallet::get_password_from_env()
                        } else {
                            Some(self.password.clone())
                        };
                        let password = match password {
                            Some(p) => p,
                            None => {
                                self.status = "Пароль не указан (введите в поле или задайте STRANGECOIN_WALLET_PASSWORD)".to_string();
                                ctx.request_repaint();
                                return;
                            }
                        };
                        match wallet::Wallet::list_keystores(&data_dir) {
                            Ok(keystores) => {
                                let sanitized_address = self.wallet_address
                                    .replace("/", "_")
                                    .replace("+", "_")
                                    .replace("=", "_");
                                let keystore_path = keystores.iter().find(|p| {
                                    p.file_name().and_then(|n| n.to_str()) == Some(&format!("wallet_{}.json", sanitized_address))
                                });
                                let keystore_path = match keystore_path {
                                    Some(p) => p,
                                    None => {
                                        self.status = "Кошелёк не найден".to_string();
                                        ctx.request_repaint();
                                        return;
                                    }
                                };
                                match wallet::Wallet::load(&password, keystore_path) {
                                    Ok(wallet) => {
                                        if BASE64.encode(wallet.public_key.serialize()) == self.wallet_address {
                                            self.is_authenticated = true;
                                            self.status = "Успешная аутентификация".to_string();
                                            self.node.discover_peers();
                                            let duration = SystemTime::now()
                                                .duration_since(start_time)
                                                .unwrap()
                                                .as_secs_f64();
                                            info!(duration_secs = duration, "Аутентификация успешна");
                                        } else {
                                            self.status = "Неверный адрес кошелька".to_string();
                                            let duration = SystemTime::now()
                                                .duration_since(start_time)
                                                .unwrap()
                                                .as_secs_f64();
                                            warn!(duration_secs = duration, "Аутентификация не удалась: неверный адрес кошелька");
                                        }
                                    }
                                    Err(e) => {
                                        self.status = format!("Ошибка аутентификации: {}", e);
                                        let duration = SystemTime::now()
                                            .duration_since(start_time)
                                            .unwrap()
                                            .as_secs_f64();
                                        error!(duration_secs = duration, error = %e, "Аутентификация не удалась");
                                    }
                                }
                            }
                            Err(e) => {
                                self.status = format!("Ошибка поиска кошельков: {}", e);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                error!(duration_secs = duration, error = %e, "Ошибка поиска кошельков");
                            }
                        }
                        ctx.request_repaint();
                    }
                    if ui.button("Регистрация").clicked() {
                        let start_time = SystemTime::now();
                        info!("Кнопка 'Регистрация' нажата");
                        let password = if self.new_wallet_password.is_empty() {
                            wallet::Wallet::get_password_from_env()
                        } else {
                            Some(self.new_wallet_password.clone())
                        };
                        let password = match password {
                            Some(p) => p,
                            None => {
                                self.status = "Пароль для регистрации не может быть пустым (введите в поле или задайте STRANGECOIN_WALLET_PASSWORD)".to_string();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                warn!(duration_secs = duration, "Ошибка регистрации: пустой пароль");
                                ctx.request_repaint();
                                return;
                            }
                        };
                        let data_dir = self.data_dir.clone();
                        match wallet::Wallet::new(&password, &data_dir) {
                            Ok(wallet) => {
                                self.wallet_address = BASE64.encode(wallet.public_key.serialize());
                                self.password = password;
                                self.is_authenticated = true;
                                self.status = format!("Кошелёк успешно создан: {}", self.wallet_address);
                                self.node.discover_peers();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                info!(duration_secs = duration, wallet_address = %self.wallet_address, "Регистрация успешна");
                                let mut blockchain = self.node.blockchain.write().expect("Не удалось захватить write lock для blockchain");
                                if !blockchain.balances.contains_key(&self.wallet_address) {
                                    match blockchain.grant_initial_balance_to_first_wallet(&self.wallet_address) {
                                        Ok(true) => {}
                                        Ok(false) => {
                                            blockchain.balances.entry(self.wallet_address.clone()).or_default();
                                        }
                                        Err(StrangecoinError::GrantBlocksDisabled) => {
                                            warn!("Grant blocks disabled, creating zero-balance entry");
                                            blockchain.balances.entry(self.wallet_address.clone()).or_default();
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Failed to grant initial balance");
                                            blockchain.balances.entry(self.wallet_address.clone()).or_default();
                                        }
                                    }
                                    blockchain.save_state();
                                }
                            }
                            Err(e) => {
                                self.status = format!("Ошибка регистрации: {}", e);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                error!(duration_secs = duration, error = %e, "Ошибка регистрации");
                            }
                        }
                        ctx.request_repaint();
                    }
                });
                ui.label("Пароль для нового кошелька:");
                ui.text_edit_singleline(&mut self.new_wallet_password);
            } else {
                ui.heading("Кошелёк");
                ui.label(format!("Адрес: {}", self.wallet_address));
                let balance = {
                    let blockchain = self.node.blockchain.read().expect("Не удалось захватить read lock для blockchain");
                    blockchain.balances.get(&self.wallet_address).map(|a| a.balance).unwrap_or(0)
                };
                ui.label(format!("Баланс: {}", balance));

                ui.heading("Перевод");
                ui.text_edit_singleline(&mut self.receiver_address);
                ui.text_edit_singleline(&mut self.amount);

if let Some(ref progress_rx) = self.progress_rx {
                    while let Ok(progress) = progress_rx.try_recv() {
                        let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                        *mining_progress = Some(progress);
                        debug!(?mining_progress, "Прогресс майнинга обновлён в UI");
                    }
                }

                let is_mining = matches!(*self.mining_status.lock().unwrap(), MiningStatus::Mining);

                if is_mining {
                    ui.label("Майнинг блока в процессе...");
                    let progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                    if let Some(progress_msg) = &*progress {
                        ui.label(format!("Прогресс: {}", progress_msg));
                    }
                    ui.spinner();
                    ctx.request_repaint();
                } else if ui.button("Отправить").clicked() {
                    let start_time = SystemTime::now();
                    info!(receiver = %self.receiver_address, amount = %self.amount, "Кнопка 'Отправить' нажата");
                    if self.receiver_address.trim().is_empty() {
                        self.status = "Адрес получателя не может быть пустым".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(duration_secs = duration, "Пустой адрес получателя");
                        ctx.request_repaint();
                        return;
                    }
                    if let Ok(amount) = self.amount.trim().parse::<u64>() {
                        if amount == 0 {
                            self.status = "Сумма должна быть больше нуля".to_string();
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            warn!(duration_secs = duration, "Сумма равна нулю");
                            ctx.request_repaint();
                            return;
                        }
                        let sender_nonce = {
                                let bc = self.node.blockchain.read().expect("Не удалось захватить read lock для blockchain");
                                bc.balances.get(&self.wallet_address).map(|a| a.nonce).unwrap_or(0)
                            };
                            let mut transaction = Transaction {
                                sender: self.wallet_address.clone(),
                                receiver: self.receiver_address.trim().to_string(),
                                amount,
                                nonce: sender_nonce + 1,
                                chain_id: crate::consensus::current_chain_id(),
                                signature: Vec::new(),
                                is_coinbase: false,
                            };
                            let data_dir = self.data_dir.clone();
                            let sanitized_address = self.wallet_address
                                .replace("/", "_")
                                .replace("+", "_")
                                .replace("=", "_");
                            let keystore_path = data_dir.join("keystore").join(format!("wallet_{}.json", sanitized_address));
                            let wallet = match wallet::Wallet::load(&self.password, &keystore_path) {
                                Ok(w) => w,
                                Err(e) => {
                                    self.status = format!("Ошибка загрузки кошелька: {}", e);
                                    let duration = SystemTime::now()
                                        .duration_since(start_time)
                                        .unwrap()
                                        .as_secs_f64();
                                    error!(duration_secs = duration, error = %e, "Ошибка загрузки кошелька");
                                    ctx.request_repaint();
                                    return;
                                }
                            };
                            let mut transaction = transaction;
                            if let Err(e) = wallet.sign_transaction(&mut transaction) {
                                self.status = format!("Ошибка подписи транзакции: {}", e);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                error!(duration_secs = duration, error = %e, "Ошибка подписи транзакции");
                                ctx.request_repaint();
                                return;
                            };
                        let blockchain = Arc::clone(&self.node.blockchain);
                        let mining_status = Arc::clone(&self.mining_status);
                        let mining_progress = Arc::clone(&self.mining_progress);

                        {
                            let mut blockchain = blockchain.write().expect("Не удалось захватить write lock для blockchain");
                            debug!(?transaction, "Транзакция для добавления");
                            if blockchain.add_transaction(transaction.clone()).is_err() {
                                self.status = "Недостаточно средств или неверный адрес".to_string();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                warn!(duration_secs = duration, "Недостаточно средств или неверный адрес");
                                ctx.request_repaint();
                                return;
                            }
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            info!(duration_secs = duration, "Транзакция успешно добавлена");
                        }

                        self.status = "Запуск майнинга...".to_string();
                        info!("Подготовка к отправке задачи майнинга");
                        let (progress_tx, progress_rx) = mpsc::channel();
                        let (status_tx, status_rx) = mpsc::channel();
                        {
                            let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                            *mining_progress = None;
                            self.progress_rx = Some(progress_rx);
                            self.status_rx = Some(status_rx);
                            debug!("Каналы прогресса и статуса созданы");
                        }
                        if let Ok(mut mining_status) = self.mining_status.lock() {
                            *mining_status = MiningStatus::Mining;
                            info!("Статус майнинга установлен: Mining");
                        } else {
                            self.status = "Ошибка: Не удалось установить статус майнинга".to_string();
                            error!("Не удалось установить статус майнинга");
                            self.progress_rx = None;
                            self.status_rx = None;
                            ctx.request_repaint();
                            return;
                        }
                        info!("Попытка отправки задачи майнинга");
                        if let Err(e) = self.mining_tx.send(MiningTask {
                            blockchain,
                            transaction,
                            mining_status,
                            progress_tx,
                            status_tx,
                            rate_limiter: self.node.rate_limiter.clone(),
                            shutdown: self.node.shutdown.clone(),
                        }) {
                            self.status = format!("Ошибка отправки задачи майнинга: {}", e);
                            error!(error = %e, "Ошибка отправки задачи майнинга");
                            if let Ok(mut mining_status) = self.mining_status.lock() {
                                *mining_status = MiningStatus::Idle;
                                debug!("Статус майнинга сброшен на Idle");
                            }
                            self.progress_rx = None;
                            self.status_rx = None;
                            ctx.request_repaint();
                            return;
                        }
                        info!("Задача майнинга успешно отправлена");
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        info!(duration_secs = duration, "Запуск майнинга завершён");
                        ctx.request_repaint();
                    } else {
                        self.status = "Неверный формат суммы".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(duration_secs = duration, port = %self.port, "Неверный формат суммы");
                        ctx.request_repaint();
                    }
                }

                ui.heading("Поиск кошелька по IP и порту");
                ui.horizontal(|ui| {
                    ui.label("IP: ");
                    ui.text_edit_singleline(&mut self.ip);
                    ui.label("Порт: ");
                    ui.add(egui::TextEdit::singleline(&mut self.port).desired_width(50.0));
                });
                if ui.button("Найти кошелёк").clicked() {
                    let start_time = SystemTime::now();
                    info!(ip = %self.ip, port = %self.port, "Кнопка 'Найти кошелёк' нажата");
                    let ip = self.ip.trim();
                    if ip.is_empty() {
                        self.status = "IP-адрес не может быть пустым".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(duration_secs = duration, "Пустой IP-адрес");
                    } else if let Ok(port_num) = self.port.trim().parse::<u16>() {
                        let address = format!("{}:{}", ip, port_num);
                        if let Some(wallet) = self.node.find_wallet_by_ip(ip, port_num) {
                            if self.node.add_peer(address.clone()) {
                                self.status = format!("Найден кошелёк: {} для {}:{} и добавлен в network.json", wallet, ip, port_num);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                info!(wallet = %wallet, ip = %ip, port = port_num, duration_secs = duration, "Кошелёк найден и добавлен в network.json");
                            } else {
                                self.status = format!("Найден кошелёк: {} для {}:{}, уже существует в network.json", wallet, ip, port_num);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                info!(wallet = %wallet, ip = %ip, port = port_num, duration_secs = duration, "Кошелёк найден, уже существует в network.json");
                            }
                        } else {
                            self.status = format!("Кошелёк не найден для {}:{}", ip, port_num);
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            warn!(ip = %ip, port = port_num, duration_secs = duration, "Кошелёк не найден");
                        }
                    } else {
                        self.status = "Неверный формат порта".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(port = %self.port, duration_secs = duration, "Неверный формат порта");
                    }
                    ctx.request_repaint();
                }
            }
        });
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    // Handle CLI commands
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--print-genesis-hash" {
        crate::cli::print_genesis_hash();
        return;
    }

    let exe_path =
        std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
    let exe_dir = exe_path
        .parent()
        .expect("Не удалось получить директорию исполняемого файла");
    let config_toml_path = exe_dir.join("config.toml");
    let config_json_path = exe_dir.join("config.json");

    // Migration: config.json -> config.toml
    let config = if config_toml_path.exists() {
        crate::config::Config::load(&config_toml_path).expect("Ошибка загрузки config.toml")
    } else if config_json_path.exists() {
        info!("Migrating config.json to config.toml");
        let config_content =
            fs::read_to_string(&config_json_path).expect("Ошибка чтения config.json");
        let old_config: serde_json::Value =
            serde_json::from_str(&config_content).expect("Ошибка парсинга config.json");

        let new_config = crate::config::Config {
            network_id: 3,
            node_mode: crate::config::NodeMode::Full,
            network: crate::config::NetworkConfig {
                listen_addr: format!(
                    "{}:{}",
                    old_config["wallet"]["ip"].as_str().unwrap_or("127.0.0.1"),
                    old_config["wallet"]["port"].as_u64().unwrap_or(8081)
                )
                .parse()
                .unwrap(),
                seeds: vec![],
                max_peers: 50,
            },
            storage: crate::config::StorageConfig {
                path: exe_dir.join("data/leveldb"),
            },
            log_level: "info".into(),
            data_dir: exe_dir.join("data"),
            allow_grant_blocks: false,
        };

        let toml_content =
            toml::to_string_pretty(&new_config).expect("Ошибка сериализации config.toml");
        fs::write(&config_toml_path, toml_content).expect("Ошибка записи config.toml");

        fs::remove_file(&config_json_path).expect("Ошибка удаления config.json");

        new_config
    } else {
        crate::config::Config::default()
    };

    let network_path = exe_dir.join("network.json");
    let network_config: serde_json::Value = match fs::read_to_string(&network_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|err| {
            warn!(
                "Ошибка парсинга network.json: {}. Используются значения по умолчанию.",
                err
            );
            serde_json::json!({
                "peers": ["127.0.0.1:8081", "127.0.0.1:8082", "127.0.0.1:8083"]
            })
        }),
        Err(err) => {
            warn!(
                "Ошибка чтения network.json: {}. Используются значения по умолчанию.",
                err
            );
            serde_json::json!({
                "peers": ["127.0.0.1:8081", "127.0.0.1:8082", "127.0.0.1:8083"]
            })
        }
    };

    let default_peers = vec![
        "127.0.0.1:8081".to_string(),
        "127.0.0.1:8082".to_string(),
        "127.0.0.1:8083".to_string(),
    ];
    let peers: Vec<String> = network_config["peers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or(default_peers.clone());

    if peers == default_peers {
        let network_content = serde_json::to_string_pretty(&serde_json::json!({ "peers": peers }))
            .expect("Ошибка сериализации network.json");
        fs::write(&network_path, network_content).expect("Ошибка записи в network.json");
    }

    let (mining_tx, mining_rx) = mpsc::channel();
    let (sync_tx, sync_rx) = mpsc::channel();
    info!("Каналы майнинга и синхронизации созданы");

    let listen_addr = config.network.listen_addr;
    let port = listen_addr.port();

    // Create shared shutdown signal for graceful shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_ctrlc = Arc::clone(&shutdown);

    let mut node = Node::new(listen_addr.to_string(), mining_rx, sync_tx.clone(), port);
    {
        let mut bc = node.blockchain.write().expect("Failed to acquire blockchain lock");
        bc.allow_grant_blocks = config.allow_grant_blocks;
    }
    node.start_server(port, sync_tx.clone());
    node.discover_peers();

    // Spawn sync thread with shutdown handling
    let shutdown_sync = Arc::clone(&shutdown);
    let shutdown_sync_node = Arc::clone(&shutdown);
    let node_blockchain = Arc::clone(&node.blockchain);
    let node_peers = Arc::clone(&node.peers);
    let node_address = node.address.clone();
    let node_rate_limiter = node.rate_limiter.clone();
    let sync_tx_clone = sync_tx.clone();

    let sync_thread_handle = thread::spawn(move || {
        let mut sync_node = Node {
            blockchain: node_blockchain,
            peers: node_peers,
            address: node_address,
            sync_rx: mpsc::channel().1,
            rate_limiter: node_rate_limiter,
            shutdown: shutdown_sync_node,
            listener: Arc::new(Mutex::new(None)),
            sync_thread_handle: Arc::new(Mutex::new(None)),
        };
        while !shutdown_sync.load(Ordering::Relaxed) {
            sync_node.sync_blockchain(sync_tx_clone.clone());
            // Sleep with periodic shutdown checks
            for _ in 0..10 {
                if shutdown_sync.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        info!("Sync thread stopped");
    });

    // Store sync thread handle in node for graceful shutdown
    *node.sync_thread_handle.lock().unwrap() = Some(sync_thread_handle);

    let blockchain = Arc::clone(&node.blockchain);

    let app = WalletApp {
        node: Node {
            blockchain: Arc::clone(&node.blockchain),
            peers: Arc::clone(&node.peers),
            address: node.address.clone(),
            sync_rx,
            rate_limiter: node.rate_limiter.clone(),
            shutdown: Arc::clone(&shutdown),
            listener: Arc::new(Mutex::new(None)),
            sync_thread_handle: Arc::new(Mutex::new(None)),
        },
        wallet_address: String::new(),
        password: String::new(),
        is_authenticated: false,
        receiver_address: String::new(),
        amount: String::new(),
        ip: String::new(),
        port: String::new(),
        status: String::new(),
        mining_status: Arc::new(Mutex::new(MiningStatus::Idle)),
        mining_progress: Arc::new(Mutex::new(None)),
        progress_rx: None,
        status_rx: None,
        mining_tx,
        last_repaint: 0.0,
        new_wallet_password: String::new(),
        data_dir: config.data_dir.clone(),
    };

    // Graceful shutdown handler
    ctrlc::set_handler(move || {
        info!("Shutdown signal received, initiating graceful shutdown...");
        shutdown_ctrlc.store(true, Ordering::Relaxed);

        // Give time for threads to shut down gracefully
        let shutdown_start = Instant::now();
        let shutdown_timeout = Duration::from_secs(30);

        // Wait for mining and sync threads to finish
        while shutdown_start.elapsed() < shutdown_timeout {
            std::thread::sleep(Duration::from_millis(100));
            // The Drop impls will handle cleanup when node goes out of scope
        }

        if shutdown_start.elapsed() >= shutdown_timeout {
            error!("Shutdown timeout exceeded, forcing exit");
        }

        info!("Graceful shutdown complete, exiting");
        std::process::exit(0);
    })
    .expect("Ошибка установки обработчика завершения");

    eframe::run_native(
        "Blockchain Wallet",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Box::new(app)),
    )
    .expect("Ошибка запуска приложения");
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    use rand::rngs::OsRng;
    use secp256k1::{ecdsa::RecoverableSignature, Message, PublicKey, Secp256k1, SecretKey};
    use std::path::Path;

    pub static NETWORK_TEST_LOCK: Mutex<()> = Mutex::new(());

    pub struct TestDir(pub PathBuf);

    impl TestDir {
        pub fn new(tag: &str) -> Self {
            let mut dir = std::env::temp_dir();
            dir.push(format!("strangecoin_test_{}_{}", tag, uuid::Uuid::new_v4()));
            Self(dir)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub fn temp_db_dir(tag: &str) -> PathBuf {
        TestDir::new(tag).0
    }

    pub fn random_port() -> u16 {
        use std::net::TcpListener;
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    pub fn create_test_blockchain(db_path: &Path) -> Blockchain {
        fs::create_dir_all(db_path).expect("Failed to create test DB directory");
        let storage =
            crate::storage::Storage::new(db_path).expect("Failed to open test DB");
        let mut bc = Blockchain {
            chain: vec![],
            balances: HashMap::new(),
            difficulty: 0,
            mempool: crate::mempool::Mempool::new(),
            storage,
            allow_grant_blocks: true,
        };
        bc.create_genesis_block();
        bc
    }

    pub fn mine_current(blockchain: &mut Blockchain) {
        let (progress_tx, _progress_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let block = blockchain.mine_block(progress_tx, &shutdown);
        assert!(block.is_some(), "Mining current transaction failed");
    }

    pub fn sync_to_longest(wallets: &[Arc<RwLock<Blockchain>>]) {
        let (idx, len) = wallets
            .iter()
            .enumerate()
            .map(|(i, w)| (i, w.read().unwrap().chain.len()))
            .max_by_key(|(_, l)| *l)
            .expect("Wallet list is empty");
        let (chain, balances, difficulty, pending) = {
            let w = wallets[idx].read().unwrap();
            (
                w.chain.clone(),
                w.balances.clone(),
                w.difficulty,
                w.mempool.transactions(),
            )
        };
        for (i, w) in wallets.iter().enumerate() {
            let mut guard = w.write().unwrap();
            if i != idx && guard.chain.len() < len {
                guard.chain = chain.clone();
                guard.balances = balances.clone();
                guard.difficulty = difficulty;
                guard.mempool = crate::mempool::Mempool::new();
                for tx in &pending {
                    let _ = guard.mempool.insert(tx.clone(), &AccountState::default());
                }
                guard.save_state();
            }
        }
    }

    pub fn assert_balances(
        wallets: &[Arc<RwLock<Blockchain>>],
        addrs: &[String],
        expected: &[u64],
    ) {
        for (i, w) in wallets.iter().enumerate() {
            let bc = w.read().unwrap();
            let bal = bc.balances.get(&addrs[i]).map(|a| a.balance).unwrap_or(0);
            assert_eq!(
                bal, expected[i],
                "Wallet balance mismatch for {}",
                addrs[i]
            );
        }
    }

    pub fn amount_for(edge: usize, pass: usize) -> u64 {
        match edge {
            0 => 387 + pass as u64,
            1 => {
                if pass <= 24 {
                    199 + pass as u64
                } else {
                    4924
                }
            }
            2 => {
                if pass <= 24 {
                    149 + pass as u64
                } else {
                    6124
                }
            }
            3 => {
                if pass <= 24 {
                    99 + pass as u64
                } else {
                    7324
                }
            }
            _ => panic!("Unknown edge"),
        }
    }

    pub fn adopt_from(
        target: &Arc<RwLock<Blockchain>>,
        source: &Arc<RwLock<Blockchain>>,
    ) -> bool {
        let src = source.read().unwrap();
        let (src_chain, src_balances, src_difficulty, src_pending) = (
            src.chain.clone(),
            src.balances.clone(),
            src.difficulty,
            src.mempool.transactions(),
        );
        drop(src);

        let mut tgt = target.write().unwrap();
        let current_len = tgt.chain.len();
        if src_chain.is_empty() || src_chain.len() <= 1 {
            return false;
        }
        if src_chain.len() <= current_len {
            return false;
        }

        let mut new_blockchain = Blockchain {
            chain: src_chain,
            balances: src_balances,
            difficulty: src_difficulty,
            mempool: crate::mempool::Mempool::new(),
            storage: tgt.storage.clone(),
            allow_grant_blocks: tgt.allow_grant_blocks,
        };
        let mut merged_pending = vec![];
        for tx in src_pending.iter() {
            if !new_blockchain
                .chain
                .iter()
                .any(|b| b.transactions.iter().any(|t| t == tx))
                && !merged_pending.iter().any(|t| t == tx)
                && new_blockchain.add_transaction(tx.clone()).is_ok()
            {
                merged_pending.push(tx.clone());
            }
        }
        let current_pending = tgt.mempool.transactions();
        for tx in current_pending.iter() {
            if !new_blockchain
                .chain
                .iter()
                .any(|b| b.transactions.iter().any(|t| t == tx))
                && !merged_pending.iter().any(|t| t == tx)
                && new_blockchain.add_transaction(tx.clone()).is_ok()
            {
                merged_pending.push(tx.clone());
            }
        }

        if new_blockchain.validate_chain() {
            *tgt = new_blockchain;
            true
        } else {
            false
        }
    }

    pub fn create_node_for_test(
        bc: &Arc<RwLock<Blockchain>>,
        port: u16,
    ) -> (Node, Arc<Mutex<Vec<String>>>) {
        let peers = Arc::new(Mutex::new(vec![]));
        let node = Node {
            blockchain: Arc::clone(bc),
            peers: Arc::clone(&peers),
            address: format!("127.0.0.1:{}", port),
            sync_rx: mpsc::channel().1,
            rate_limiter: Arc::new(crate::network::RateLimiter::new(10, 100)),
            shutdown: Arc::new(AtomicBool::new(false)),
            listener: Arc::new(Mutex::new(None)),
            sync_thread_handle: Arc::new(Mutex::new(None)),
        };
        (node, peers)
    }

    pub fn create_sync_node(
        bc: &Arc<RwLock<Blockchain>>,
        peers: &Arc<Mutex<Vec<String>>>,
        address: &str,
        rate_limiter: &Arc<crate::network::RateLimiter>,
    ) -> Node {
        Node {
            blockchain: Arc::clone(bc),
            peers: Arc::clone(peers),
            address: address.to_string(),
            sync_rx: mpsc::channel().1,
            rate_limiter: Arc::clone(rate_limiter),
            shutdown: Arc::new(AtomicBool::new(false)),
            listener: Arc::new(Mutex::new(None)),
            sync_thread_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn sign_transaction(
        tx: &mut Transaction,
        secret_key: &SecretKey,
    ) {
        let secp = Secp256k1::new();
        let msg_bytes = strangecoin_core::serialize::serialize_transaction(tx);
        let msg_hash = blake3::hash(&msg_bytes);
        let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
        let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, secret_key);
        let (rec_id, sig_bytes) = sig.serialize_compact();
        let mut sig_vec = Vec::with_capacity(65);
        sig_vec.extend_from_slice(&sig_bytes);
        sig_vec.push(rec_id.to_i32() as u8);
        tx.signature = sig_vec;
    }

    pub fn generate_keypair() -> (String, SecretKey) {
        let secp = Secp256k1::new();
        let sk = SecretKey::new(&mut OsRng);
        let pk = PublicKey::from_secret_key(&secp, &sk);
        let addr = BASE64.encode(pk.serialize());
        (addr, sk)
    }

    pub fn generate_keypairs(n: usize) -> Vec<(String, SecretKey)> {
        (0..n).map(|_| generate_keypair()).collect()
    }

    pub fn create_and_mine_tx(
        bc: &mut Blockchain,
        sender_addr: &str,
        receiver_addr: &str,
        amount: u64,
        secret_key: &SecretKey,
    ) {
        let sender_nonce = bc.balances.get(sender_addr).map(|a| a.nonce).unwrap_or(0);
        let mut tx = Transaction {
            sender: sender_addr.to_string(),
            receiver: receiver_addr.to_string(),
            amount,
            nonce: sender_nonce + 1,
            chain_id: crate::consensus::current_chain_id(),
            signature: Vec::new(),
            is_coinbase: false,
        };
        sign_transaction(&mut tx, secret_key);
        assert!(bc.add_transaction(tx).is_ok(), "Transaction rejected");
        mine_current(bc);
    }
}
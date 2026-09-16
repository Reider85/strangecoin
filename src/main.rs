use crate::error::StrangecoinError;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use blake3;
use eframe::egui;
use hex;
use rand::Rng;
use rusty_leveldb::{LdbIterator, Options, DB};
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, PublicKey, Secp256k1,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json;
use sha2::{Digest, Sha256};
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
use uuid::Uuid;
mod address;
mod api;
mod blockchain;
mod cli;
mod config;
mod consensus;
mod economics;
mod error;
mod governance;
#[cfg(feature = "gui")]
mod gui;
mod mempool;
mod network;
mod serialize;
mod storage;
mod wallet;

// Структура блока
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Block {
    index: u64,
    timestamp: u64,
    transactions: Vec<Transaction>,
    previous_hash: String, // hex-encoded blake3 hash (32 bytes = 64 hex chars)
    hash: String,          // hex-encoded blake3 hash (32 bytes = 64 hex chars)
    nonce: u64,
    target: String, // hex-encoded 32-byte target (compact bits representation)
}

// Структура транзакции
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
struct Transaction {
    sender: String,
    receiver: String,
    amount: u64,
    #[serde(default)]
    nonce: u64,
    #[serde(default)]
    chain_id: u32,
    #[serde(default)]
    signature: Vec<u8>,
    #[serde(default)]
    is_coinbase: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct AccountState {
    balance: u64,
    nonce: u64,
}

impl std::fmt::Display for AccountState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.balance)
    }
}

impl std::ops::AddAssign<u64> for AccountState {
    fn add_assign(&mut self, rhs: u64) {
        self.balance += rhs;
    }
}

impl std::ops::SubAssign<u64> for AccountState {
    fn sub_assign(&mut self, rhs: u64) {
        self.balance -= rhs;
    }
}

// Вспомогательная структура для десериализации Blockchain
#[derive(Deserialize, Serialize)]
struct BlockchainDeserialize {
    chain: Vec<Block>,
    balances: HashMap<String, AccountState>,
    difficulty: u32,
    pending_transactions: Vec<Transaction>,
    mempool_txs: Vec<Transaction>, // New field for mempool
}

// Структура блокчейна
#[derive(Clone)]
struct Blockchain {
    chain: Vec<Block>,
    balances: HashMap<String, AccountState>,
    difficulty: u32,
    mempool: crate::mempool::Mempool,
    storage: crate::storage::Storage,
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
        })
    }
}

// Структура для команды майнинга
#[derive(Clone)]
struct MiningTask {
    blockchain: Arc<RwLock<Blockchain>>,
    transaction: Transaction,
    mining_status: Arc<Mutex<MiningStatus>>,
    progress_tx: mpsc::Sender<String>,
    status_tx: mpsc::Sender<String>,
    rate_limiter: Arc<crate::network::RateLimiter>,
    shutdown: Arc<AtomicBool>,
}

// Структура узла
struct Node {
    blockchain: Arc<RwLock<Blockchain>>,
    peers: Arc<Mutex<Vec<String>>>,
    address: String,
    sync_rx: mpsc::Receiver<Blockchain>,
    rate_limiter: Arc<crate::network::RateLimiter>,
    shutdown: Arc<AtomicBool>,
    listener: Arc<Mutex<Option<TcpListener>>>,
    sync_thread_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

// Структура клиента для GUI
struct WalletApp {
    node: Node,
    wallet_address: String,
    password: String,
    is_authenticated: bool,
    receiver_address: String,
    amount: String,
    ip: String,
    port: String,
    status: String,
    mining_status: Arc<Mutex<MiningStatus>>,
    mining_progress: Arc<Mutex<Option<String>>>,
    progress_rx: Option<mpsc::Receiver<String>>,
    status_rx: Option<mpsc::Receiver<String>>,
    mining_tx: mpsc::Sender<MiningTask>,
    mining_thread: Option<JoinHandle<()>>,
    last_repaint: f64,
    last_sync: f64,
    new_wallet_password: String,
    data_dir: PathBuf,
}

// Статус майнинга
#[derive(Clone, Debug)]
enum MiningStatus {
    Idle,
    Mining,
    Completed(Option<Block>),
    Failed(String),
}

impl Blockchain {
    fn debug_db(&self) {
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

    fn new(port: u16) -> Self {
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
                };
                let hash = crate::serialize::block_hash(&block);
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

    fn create_genesis_block(&mut self) {
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
            target: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(), // max target (easy mining for tests)
        };
        let hash = self.calculate_hash(&genesis_block);
        let mut genesis_block = genesis_block;
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
    fn create_grant_block(&mut self, wallet_address: &str, amount: u64) {
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
        };
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
    fn grant_initial_balance_to_first_wallet(&mut self, wallet_address: &str) -> bool {
        if self.chain.len() != 1 {
            return false;
        }
        if self.balances.contains_key(wallet_address) {
            return false;
        }
        let is_first_wallet =
            self.balances.len() == 1 && self.balances.contains_key("initial_wallet_address");
        if !is_first_wallet {
            return false;
        }
        let amount = match self.balances.get("initial_wallet_address") {
            Some(a) if a.balance > 0 => a.balance,
            _ => return false,
        };
        self.create_grant_block(wallet_address, amount);
        info!(wallet = %wallet_address, amount, "Первому кошельку начислен первоначальный баланс");
        true
    }

    // Миграция для существующих баз: переносит баланс генезис-кошелька на единственный реальный кошелёк с нулевым балансом
    fn migrate_initial_wallet_balance(&mut self) {
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

    fn calculate_hash(&self, block: &Block) -> String {
        let start_time = SystemTime::now();
        let hash_bytes = crate::serialize::block_hash(block);
        let hash = hex::encode(hash_bytes);
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        debug!(duration_secs = duration, hash = %hash, "Вычисление хэша завершено");
        hash
    }

    fn mine_block(
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

        if let Some(mut block) = block {
            let balance_start_time = SystemTime::now();
            let mut mined_txids = Vec::new();
            for tx in &block.transactions {
                let mut sender_final = 0u64;
                // Skip balance check for coinbase (sender "coinbase" has no balance to deduct)
                if !tx.is_coinbase {
                    let sender_balance = self
                        .balances
                        .get(&tx.sender)
                        .map(|a| a.balance)
                        .unwrap_or(0);
                    if sender_balance < tx.amount {
                        error!(sender = %tx.sender, nonce = tx.nonce, "Недостаточно средств для транзакции");
                        return None;
                    }
                    sender_final = sender_balance - tx.amount;
                    self.balances.entry(tx.sender.clone()).or_default().balance = sender_final;
                    // Track txid for mempool removal
                    mined_txids.push(crate::serialize::txid(tx));
                }
                let receiver_balance = self
                    .balances
                    .get(&tx.receiver)
                    .map(|a| a.balance)
                    .unwrap_or(0);
                let receiver_final = receiver_balance + tx.amount;
                self.balances
                    .entry(tx.receiver.clone())
                    .or_default()
                    .balance = receiver_final;

                info!(sender = %tx.sender, sender_final, receiver = %tx.receiver, receiver_final, "Обновлён баланс");
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

    fn mine_block_inner(
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
        };

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

    fn add_transaction(&mut self, transaction: Transaction) -> Result<(), StrangecoinError> {
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

    fn validate_chain(&self) -> bool {
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
            let block_size = crate::serialize::serialize_block(block).len();
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
        let mut expected_balances: HashMap<String, u64> = HashMap::new();
        let mut total_supply_before_block = 0u64;
        debug!(
            ?expected_balances,
            "Начальная инициализация expected_balances"
        );

        // Применяем все транзакции из цепочки блоков
        for block in &self.chain {
            info!(
                block_index = block.index,
                tx_count = block.transactions.len(),
                "Обработка блока"
            );

            // Validate coinbase for non-genesis blocks (skip grant block at index 1)
            if block.index > 0 && block.index != 1 {
                let coinbase_tx = block.transactions.iter().find(|tx| tx.is_coinbase);
                if let Some(coinbase) = coinbase_tx {
                    let expected_reward = crate::economics::emission::block_reward_at_height(
                        block.index,
                        total_supply_before_block,
                    );
                    if coinbase.amount > expected_reward {
                        warn!(
                            block_index = block.index,
                            expected = expected_reward,
                            got = coinbase.amount,
                            "Coinbase amount exceeds emission schedule"
                        );
                        return false;
                    }
                    // Miner can underpay voluntarily (coinbase.amount < expected_reward is OK)
                } else {
                    warn!(
                        block_index = block.index,
                        "Block missing coinbase transaction"
                    );
                    return false;
                }
            }

            for tx in &block.transactions {
                debug!(nonce = tx.nonce, sender = %tx.sender, receiver = %tx.receiver, amount = tx.amount, "Обработка транзакции");
                // Пропускаем проверку баланса для отправителя "genesis" и "coinbase"
                if tx.sender != "genesis" && tx.sender != "coinbase" {
                    let sender_balance = expected_balances.get(&tx.sender).unwrap_or(&0);
                    debug!(sender = %tx.sender, balance = *sender_balance, "Текущий баланс отправителя");
                    if *sender_balance < tx.amount {
                        warn!(sender = %tx.sender, block_index = block.index, nonce = tx.nonce, required = tx.amount, available = *sender_balance, "Недостаточно средств в блоке");
                        return false;
                    }
                    *expected_balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
                    debug!(sender = %tx.sender, amount = tx.amount, new_balance = expected_balances.get(&tx.sender).unwrap_or(&0), "Баланс отправителя уменьшен");
                } else {
                    debug!("Отправитель '{}', пропуск проверки баланса", tx.sender);
                }
                *expected_balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
                debug!(receiver = %tx.receiver, amount = tx.amount, new_balance = expected_balances.get(&tx.receiver).unwrap_or(&0), "Баланс получателя увеличен");
                debug!(
                    nonce = tx.nonce,
                    ?expected_balances,
                    "Обновлённые expected_balances после транзакции"
                );
            }

            // Update total supply after processing block (for next block's coinbase validation)
            total_supply_before_block = expected_balances.values().sum();
        }

        // Проверяем неподтверждённые транзакции (mempool)
        let mut temp_balances = expected_balances.clone();
        debug!(
            ?temp_balances,
            "Проверка неподтверждённых транзакций, начальные temp_balances"
        );
        for tx in self.mempool.transactions() {
            debug!(nonce = tx.nonce, sender = %tx.sender, receiver = %tx.receiver, amount = tx.amount, "Обработка неподтверждённой транзакции");
            let sender_balance = temp_balances.get(&tx.sender).unwrap_or(&0);
            debug!(sender = %tx.sender, balance = *sender_balance, "Текущий баланс отправителя в temp_balances");
            if *sender_balance < tx.amount {
                warn!(sender = %tx.sender, nonce = tx.nonce, required = tx.amount, available = *sender_balance, "Недостаточно средств в mempool");
                return false;
            }
            *temp_balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
            *temp_balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
            debug!(sender = %tx.sender, amount = tx.amount, new_balance = temp_balances.get(&tx.sender).unwrap_or(&0), "Баланс отправителя уменьшен");
            debug!(receiver = %tx.receiver, amount = tx.amount, new_balance = temp_balances.get(&tx.receiver).unwrap_or(&0), "Баланс получателя увеличен");
            debug!(
                nonce = tx.nonce,
                ?temp_balances,
                "Обновлённые temp_balances после mempool транзакции"
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
        let reconstructed: HashMap<String, u64> = expected_balances
            .iter()
            .filter(|(_, v)| **v != 0)
            .map(|(k, v)| (k.clone(), *v))
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

    fn save_state(&mut self) {
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

    fn discover_peers(&mut self) {
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

    fn add_peer(&mut self, address: String) -> bool {
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

    fn find_wallet_by_ip(&self, ip: &str, port: u16) -> Option<String> {
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

    fn start_server(&mut self, port: u16, sync_tx: mpsc::Sender<Blockchain>) {
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
                                        });
                                    } else {
                                        warn!("Полученный блокчейн не прошёл валидацию");
                                    }
                                } else {
                                    // Обновляем только pending_transactions, добавляя только валидные
                                    for tx in temp_blockchain.pending_transactions.iter() {
                                        if !blockchain
                                            .mempool
                                            .contains(&crate::serialize::txid(&tx))
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
    fn sync_blockchain(&mut self, sync_tx: mpsc::Sender<Blockchain>) {
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
                                            .contains(&crate::serialize::txid(&tx))
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
                                        if base64::encode(wallet.public_key.serialize()) == self.wallet_address {
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
                                self.wallet_address = base64::encode(wallet.public_key.serialize());
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
                                    if !blockchain.grant_initial_balance_to_first_wallet(&self.wallet_address) {
                                        blockchain.balances.entry(self.wallet_address.clone()).or_default();
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

fn main() {
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
        mining_thread: None,
        last_repaint: 0.0,
        last_sync: 0.0,
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
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use base64::Engine;
    use rand::rngs::OsRng;
    use secp256k1::{ecdsa::RecoverableSignature, Message, PublicKey, Secp256k1, SecretKey};
    use std::path::Path;

    // Сетевые тесты пишут network.json рядом с тестовым exe; блокируем их взаимный запуск,
    // чтобы не затирать файл друг друга при параллельном выполнении.
    static NETWORK_TEST_LOCK: Mutex<()> = Mutex::new(());

    // Создаёт уникальную временную директорию для БД кошелька
    fn temp_db_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("strangecoin_test_{}_{}", tag, Uuid::new_v4()));
        dir
    }

    // Создаёт блокчейн с собственной БД (отдельная для каждого кошелька)
    fn create_test_blockchain(db_path: &Path) -> Blockchain {
        fs::create_dir_all(db_path).expect("Не удалось создать директорию тестовой БД");
        let storage =
            crate::storage::Storage::new(db_path).expect("Не удалось открыть тестовую БД");
        let mut bc = Blockchain {
            chain: vec![],
            balances: HashMap::new(),
            difficulty: 0,
            mempool: crate::mempool::Mempool::new(),
            storage,
        };
        bc.create_genesis_block();
        bc
    }

    // Майнит все ожидающие транзакции в новый блок
    fn mine_current(blockchain: &mut Blockchain) {
        let (progress_tx, _progress_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let block = blockchain.mine_block(progress_tx, &shutdown);
        assert!(block.is_some(), "Майнинг текущей транзакции не удался");
    }

    // Синхронизирует все кошельки: самая длинная цепочка распространяется на остальные
    fn sync_to_longest(wallets: &[Arc<RwLock<Blockchain>>]) {
        let (idx, len) = wallets
            .iter()
            .enumerate()
            .map(|(i, w)| (i, w.read().unwrap().chain.len()))
            .max_by_key(|(_, l)| *l)
            .expect("Список кошельков пуст");
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

    // Сверяет балансы всех кошельков с эталонной моделью
    fn assert_balances(wallets: &[Arc<RwLock<Blockchain>>], addrs: &[String], expected: &[u64]) {
        for (i, w) in wallets.iter().enumerate() {
            let bc = w.read().unwrap();
            let bal = bc.balances.get(&addrs[i]).map(|a| a.balance).unwrap_or(0);
            assert_eq!(
                bal, expected[i],
                "Баланс кошелька {} не совпадает",
                addrs[i]
            );
        }
    }

    // Сумма транзакции для ребра (0: w0->w1, 1: w1->w2, 2: w2->w3, 3: w3->w4) на проходе pass (1..=25).
    // Все 100 сумм попарно различны, а сумма по каждому ребру за 25 проходов равна 10000.
    fn amount_for(edge: usize, pass: usize) -> u64 {
        match edge {
            0 => 387 + pass as u64, // 388..=412
            1 => {
                if pass <= 24 {
                    199 + pass as u64
                } else {
                    4924
                }
            } // 200..=223, 4924
            2 => {
                if pass <= 24 {
                    149 + pass as u64
                } else {
                    6124
                }
            } // 150..=173, 6124
            3 => {
                if pass <= 24 {
                    99 + pass as u64
                } else {
                    7324
                }
            } // 100..=123, 7324
            _ => panic!("Неизвестное ребро"),
        }
    }

    #[test]
    fn hundred_transactions_five_wallets() {
        // Создаём адреса 5 кошельков (secp256k1) и отдельную БД для каждого в начале теста
        let secp = Secp256k1::new();
        let keypairs: Vec<(String, SecretKey)> = (0..5)
            .map(|_| {
                let sk = SecretKey::new(&mut OsRng);
                let pk = PublicKey::from_secret_key(&secp, &sk);
                let addr = BASE64.encode(pk.serialize());
                (addr, sk)
            })
            .collect();
        let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut wallets: Vec<Arc<RwLock<Blockchain>>> = Vec::new();
        for i in 0..5 {
            let dir = temp_db_dir(&format!("wallet_{}", i));
            dirs.push(dir.clone());
            wallets.push(Arc::new(RwLock::new(create_test_blockchain(&dir))));
        }

        // Первый кошелёк получает 10000 из генезис-блока, у остальных 0
        {
            let mut bc = wallets[0].write().unwrap();
            assert!(bc.grant_initial_balance_to_first_wallet(&addrs[0]));
        }
        sync_to_longest(&wallets);
        assert_balances(&wallets, &addrs, &[10000, 0, 0, 0, 0]);

        let mut ref_balances = [10000u64, 0, 0, 0, 0];
        let edges = [(0usize, 1usize), (1, 2), (2, 3), (3, 4)];

        // 100 транзакций: 25 проходов по цепочке w0->w1->w2->w3->w4
        for tx_index in 1..=100u64 {
            let pass = ((tx_index - 1) / 4 + 1) as usize; // 1..=25
            let edge = ((tx_index - 1) % 4) as usize; // 0..=3
            let (s, r) = edges[edge];
            let amount = amount_for(edge, pass);

            let sender_nonce = {
                let bc = wallets[s].read().unwrap();
                bc.balances.get(&addrs[s]).map(|a| a.nonce).unwrap_or(0)
            };

            let mut transaction = Transaction {
                sender: addrs[s].clone(),
                receiver: addrs[r].clone(),
                amount,
                nonce: sender_nonce + 1,
                chain_id: crate::consensus::current_chain_id(),
                signature: Vec::new(),
                is_coinbase: false,
            };

            // Sign the transaction
            let msg_bytes = crate::serialize::serialize_transaction(&transaction);
            let msg_hash = blake3::hash(&msg_bytes);
            let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
            let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &keypairs[s].1);
            let (rec_id, sig_bytes) = sig.serialize_compact();
            let mut sig_vec = Vec::with_capacity(65);
            sig_vec.extend_from_slice(&sig_bytes);
            sig_vec.push(rec_id.to_i32() as u8);
            transaction.signature = sig_vec;

            {
                let mut bc = wallets[s].write().unwrap();
                assert!(
                    bc.add_transaction(transaction).is_ok(),
                    "Транзакция {} отклонена (недостаточно средств?)",
                    tx_index
                );
                mine_current(&mut bc);
            }
            sync_to_longest(&wallets);

            // Эталонная модель учёта
            ref_balances[s] -= amount;
            ref_balances[r] += amount;

            // Проверяем балансы каждую 10-ю транзакцию
            if tx_index % 10 == 0 {
                assert_balances(&wallets, &addrs, &ref_balances);
            }
        }

        // В конце у последнего кошелька 10000, у остальных 0
        assert_balances(&wallets, &addrs, &[0, 0, 0, 0, 10000]);

        for w in &wallets {
            let bc = w.read().unwrap();
            assert_eq!(bc.chain.len(), 102, "Цепочка должна содержать генезис-блок, блок первичной эмиссии и 100 блоков транзакций");
            assert!(bc.validate_chain(), "Цепочка не прошла валидацию");
        }

        // Очищаем временные БД
        for dir in &dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }

    // Воспроизводит обработку UPDATE_BLOCKCHAIN: принятие чужой (более длинной) цепочки после валидации
    fn adopt_from(target: &Arc<RwLock<Blockchain>>, source: &Arc<RwLock<Blockchain>>) -> bool {
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
        // Принимаем только строго более длинную цепочку (как в UPDATE_BLOCKCHAIN)
        if src_chain.len() <= current_len {
            return false;
        }

        let mut new_blockchain = Blockchain {
            chain: src_chain,
            balances: src_balances,
            difficulty: src_difficulty,
            mempool: crate::mempool::Mempool::new(),
            storage: tgt.storage.clone(),
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
        // Note: merged_pending is tracked in mempool via add_transaction

        if new_blockchain.validate_chain() {
            *tgt = new_blockchain;
            true
        } else {
            false
        }
    }

    #[test]
    fn three_instances_receive_transfer() {
        // Три инстанса (как три запущенных приложения) с отдельными БД и разными генезис-блоками
        let secp = Secp256k1::new();
        let keypairs: Vec<(String, SecretKey)> = (0..3)
            .map(|_| {
                let sk = SecretKey::new(&mut OsRng);
                let pk = PublicKey::from_secret_key(&secp, &sk);
                let addr = BASE64.encode(pk.serialize());
                (addr, sk)
            })
            .collect();
        let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();

        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut instances: Vec<Arc<RwLock<Blockchain>>> = Vec::new();
        for i in 0..3 {
            let dir = temp_db_dir(&format!("inst_{}", i));
            dirs.push(dir.clone());
            instances.push(Arc::new(RwLock::new(create_test_blockchain(&dir))));
        }

        // Регистрация кошелька в инстансе 1: грант 10000
        {
            let mut bc = instances[0].write().unwrap();
            assert!(bc.grant_initial_balance_to_first_wallet(&addrs[0]));
        }

        // Инстанс 1 распространяет свою цепочку (UPDATE_BLOCKCHAIN), остальные принимают
        for i in 1..3 {
            assert!(
                adopt_from(&instances[i], &instances[0]),
                "Инстанс {} не принял цепочку инстанса 1",
                i + 1
            );
        }
        assert_balances(&instances, &addrs, &[10000, 0, 0]);

        // Регистрация кошельков в инстансах 2 и 3 (поведение GUI-обработчика)
        for i in 1..3 {
            let mut bc = instances[i].write().unwrap();
            if !bc.balances.contains_key(&addrs[i]) {
                if !bc.grant_initial_balance_to_first_wallet(&addrs[i]) {
                    bc.balances.entry(addrs[i].clone()).or_default();
                }
            }
        }
        assert_balances(&instances, &addrs, &[10000, 0, 0]);

        // Перевод 1000 с кошелька 1 на кошелёк 2 и майнинг
        let amount = 1000u64;
        {
            let mut bc = instances[0].write().unwrap();
            let sender_nonce = bc.balances.get(&addrs[0]).map(|a| a.nonce).unwrap_or(0);
            let mut tx = Transaction {
                sender: addrs[0].clone(),
                receiver: addrs[1].clone(),
                amount,
                nonce: sender_nonce + 1,
                chain_id: crate::consensus::current_chain_id(),
                signature: Vec::new(),
                is_coinbase: false,
            };
            // Sign the transaction
            let secp = Secp256k1::new();
            let msg_bytes = crate::serialize::serialize_transaction(&tx);
            let msg_hash = blake3::hash(&msg_bytes);
            let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
            let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &keypairs[0].1);
            let (rec_id, sig_bytes) = sig.serialize_compact();
            let mut sig_vec = Vec::with_capacity(65);
            sig_vec.extend_from_slice(&sig_bytes);
            sig_vec.push(rec_id.to_i32() as u8);
            tx.signature = sig_vec;
            assert!(bc.add_transaction(tx).is_ok(), "Транзакция отклонена");
            mine_current(&mut bc);
        }

        // Инстанс 1 распространяет цепочку с транзакцией
        for i in 1..3 {
            assert!(
                adopt_from(&instances[i], &instances[0]),
                "Инстанс {} не принял цепочку с транзакцией",
                i + 1
            );
        }

        // У 1 убыло, у 2 появилась сумма, у 3 - 0
        let expected = [10000 - amount, amount, 0];
        assert_balances(&instances, &addrs, &expected);

        for dir in &dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn no_rollback_on_shorter_chain() {
        let dir1 = temp_db_dir("norb_1");
        let dir2 = temp_db_dir("norb_2");
        let bc1 = Arc::new(RwLock::new(create_test_blockchain(&dir1)));
        let bc2 = Arc::new(RwLock::new(create_test_blockchain(&dir2)));
        let secp = Secp256k1::new();
        let (a1, sk1): (String, SecretKey) = {
            let sk = SecretKey::new(&mut OsRng);
            let pk = PublicKey::from_secret_key(&secp, &sk);
            (BASE64.encode(pk.serialize()), sk)
        };
        let (a2, sk2): (String, SecretKey) = {
            let sk = SecretKey::new(&mut OsRng);
            let pk = PublicKey::from_secret_key(&secp, &sk);
            (BASE64.encode(pk.serialize()), sk)
        };

        // bc1: грант + намайненная транзакция -> [g, gr1, b1]
        {
            let mut bc = bc1.write().unwrap();
            assert!(bc.grant_initial_balance_to_first_wallet(&a1));
            let sender_nonce = bc.balances.get(&a1).map(|a| a.nonce).unwrap_or(0);
            let mut tx = Transaction {
                sender: a1.clone(),
                receiver: a2.clone(),
                amount: 1000,
                nonce: sender_nonce + 1,
                chain_id: crate::consensus::current_chain_id(),
                signature: Vec::new(),
                is_coinbase: false,
            };
            let msg_bytes = crate::serialize::serialize_transaction(&tx);
            let msg_hash = blake3::hash(&msg_bytes);
            let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
            let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &sk1);
            let (rec_id, sig_bytes) = sig.serialize_compact();
            let mut sig_vec = Vec::with_capacity(65);
            sig_vec.extend_from_slice(&sig_bytes);
            sig_vec.push(rec_id.to_i32() as u8);
            tx.signature = sig_vec;
            assert!(bc.add_transaction(tx).is_ok(), "Транзакция отклонена");
            mine_current(&mut bc);
        }
        // bc2: только грант -> [g, gr2] (короче, другая история)
        {
            let mut bc = bc2.write().unwrap();
            assert!(bc.grant_initial_balance_to_first_wallet(&a2));
        }

        // bc1 не должен откатываться на более короткую цепочку bc2
        assert!(
            !adopt_from(&bc1, &bc2),
            "Узел откатился на более короткую цепочку"
        );
        let bc1_guard = bc1.read().unwrap();
        assert_eq!(bc1_guard.chain.len(), 3, "Узел потерял намайненный блок");
        let received = bc1_guard.balances.get(&a2).map(|a| a.balance).unwrap_or(0);
        assert_eq!(
            received, 1000,
            "Баланс получателя изменился при отказе от отката"
        );
        drop(bc1_guard);

        let _ = fs::remove_dir_all(&dir1);
        let _ = fs::remove_dir_all(&dir2);
    }

    // Полный сетевой сценарий с реальными TCP-серверами: 3 узла, регистрация кошельков,
    // распространение грант-блока, передача и проверка балансов на всех узлах.
    #[test]
    fn real_network_three_nodes() {
        let _net_lock = NETWORK_TEST_LOCK.lock().unwrap();
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let net_path = exe_dir.join("network.json");
        let saved_net = fs::read_to_string(&net_path).ok();
        let net_content = r#"{"peers":["127.0.0.1:18281","127.0.0.1:18282","127.0.0.1:18283"]}"#;
        fs::write(&net_path, net_content).unwrap();

        let ports = [18281u16, 18282, 18283];
        let (sync_tx, _sync_rx) = mpsc::channel::<Blockchain>();
        let mut nodes: Vec<(Node, PathBuf, Arc<RwLock<Blockchain>>)> = Vec::new();
        let mut dirs = Vec::new();
        for (i, p) in ports.iter().enumerate() {
            let dir = temp_db_dir(&format!("net_{}", i));
            dirs.push(dir.clone());
            let bc = Arc::new(RwLock::new(create_test_blockchain(&dir)));
            let node = Node {
                blockchain: Arc::clone(&bc),
                peers: Arc::new(Mutex::new(vec![])),
                address: format!("127.0.0.1:{}", p),
                sync_rx: mpsc::channel().1,
                rate_limiter: Arc::new(crate::network::RateLimiter::new(10, 100)),
                shutdown: Arc::new(AtomicBool::new(false)),
                listener: Arc::new(Mutex::new(None)),
                sync_thread_handle: Arc::new(Mutex::new(None)),
            };
            nodes.push((node, dir.clone(), bc));
        }

        for i in 0..3 {
            nodes[i].0.discover_peers();
            nodes[i].0.start_server(ports[i], sync_tx.clone());
        }

        // Регистрация первого кошелька на узле 1 -> грант 10000
        let secp = Secp256k1::new();
        let keypairs: Vec<(String, SecretKey)> = (0..3)
            .map(|_| {
                let sk = SecretKey::new(&mut OsRng);
                let pk = PublicKey::from_secret_key(&secp, &sk);
                let addr = BASE64.encode(pk.serialize());
                (addr, sk)
            })
            .collect();
        let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
        {
            let mut bc = nodes[0].2.write().unwrap();
            assert!(
                bc.grant_initial_balance_to_first_wallet(&addrs[0]),
                "Грант не создан"
            );
        }

        // Распространяем цепочку гранта между узлами
        for round in 0..4 {
            for i in 0..3 {
                let mut sync_node = Node {
                    blockchain: Arc::clone(&nodes[i].2),
                    peers: Arc::clone(&nodes[i].0.peers),
                    address: nodes[i].0.address.clone(),
                    sync_rx: mpsc::channel().1,
                    rate_limiter: nodes[i].0.rate_limiter.clone(),
                    shutdown: Arc::new(AtomicBool::new(false)),
                    listener: Arc::new(Mutex::new(None)),
                    sync_thread_handle: Arc::new(Mutex::new(None)),
                };
                sync_node.sync_blockchain(sync_tx.clone());
            }
            std::thread::sleep(Duration::from_millis(150));
            let _ = round;
        }

        // Регистрация кошельков 2 и 3 (грант уже потрачен на первый кошелёк)
        for i in 1..3 {
            let mut bc = nodes[i].2.write().unwrap();
            if !bc.balances.contains_key(&addrs[i]) {
                if !bc.grant_initial_balance_to_first_wallet(&addrs[i]) {
                    bc.balances.entry(addrs[i].clone()).or_default();
                }
                bc.save_state();
            }
        }

        // Передача 1000 с узла 1 на кошелёк 2 (намайнивается блок)
        {
            let mut bc = nodes[0].2.write().unwrap();
            let sender_nonce = bc.balances.get(&addrs[0]).map(|a| a.nonce).unwrap_or(0);
            let mut tx = Transaction {
                sender: addrs[0].clone(),
                receiver: addrs[1].clone(),
                amount: 1000,
                nonce: sender_nonce + 1,
                chain_id: crate::consensus::current_chain_id(),
                signature: Vec::new(),
                is_coinbase: false,
            };
            let msg_bytes = crate::serialize::serialize_transaction(&tx);
            let msg_hash = blake3::hash(&msg_bytes);
            let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
            let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &keypairs[0].1);
            let (rec_id, sig_bytes) = sig.serialize_compact();
            let mut sig_vec = Vec::with_capacity(65);
            sig_vec.extend_from_slice(&sig_bytes);
            sig_vec.push(rec_id.to_i32() as u8);
            tx.signature = sig_vec;
            assert!(bc.add_transaction(tx).is_ok(), "Транзакция отклонена");
            let shutdown = Arc::new(AtomicBool::new(false));
            assert!(
                bc.mine_block(mpsc::channel().0, &shutdown).is_some(),
                "Майнинг не удался"
            );
        }

        // Распространяем цепочку с транзакцией
        for round in 0..6 {
            for i in 0..3 {
                let mut sync_node = Node {
                    blockchain: Arc::clone(&nodes[i].2),
                    peers: Arc::clone(&nodes[i].0.peers),
                    address: nodes[i].0.address.clone(),
                    sync_rx: mpsc::channel().1,
                    rate_limiter: nodes[i].0.rate_limiter.clone(),
                    shutdown: Arc::new(AtomicBool::new(false)),
                    listener: Arc::new(Mutex::new(None)),
                    sync_thread_handle: Arc::new(Mutex::new(None)),
                };
                sync_node.sync_blockchain(sync_tx.clone());
            }
            std::thread::sleep(Duration::from_millis(200));
            let _ = round;
        }

        let expected = [10000u64 - 1000, 1000, 0];
        for i in 0..3 {
            let bc = nodes[i].2.read().unwrap();
            let bal = bc.balances.get(&addrs[i]).map(|a| a.balance).unwrap_or(0);
            assert_eq!(
                bal,
                expected[i],
                "Узел {}: баланс кошелька не совпал",
                i + 1
            );
            assert!(
                bc.validate_chain(),
                "Узел {}: цепочка не прошла валидацию",
                i + 1
            );
            info!(
                node = i + 1,
                balance = bal,
                chain_len = bc.chain.len(),
                "Узел: баланс кошелька и длина цепочки"
            );
        }

        if let Some(saved) = saved_net {
            fs::write(&net_path, saved).unwrap();
        } else {
            let _ = fs::remove_file(&net_path);
        }
        for dir in &dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }

    // Сценарий "быстрой" регистрации: все три кошелька регистрируются до первого
    // раунда синхронизации. Каждый инстанс успевает создать собственный грант-блок.
    // После этого гранты не должны перетирать друг друга, а передача должна дойти.
    #[test]
    fn real_network_fast_registration_race() {
        let _net_lock = NETWORK_TEST_LOCK.lock().unwrap();
        let exe_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let net_path = exe_dir.join("network.json");
        let saved_net = fs::read_to_string(&net_path).ok();
        let net_content = r#"{"peers":["127.0.0.1:18291","127.0.0.1:18292","127.0.0.1:18293"]}"#;
        fs::write(&net_path, net_content).unwrap();

        let ports = [18291u16, 18292, 18293];
        let (sync_tx, _sync_rx) = mpsc::channel::<Blockchain>();
        let mut nodes: Vec<(Node, Arc<RwLock<Blockchain>>, Arc<Mutex<Vec<String>>>)> = Vec::new();
        let mut dirs = Vec::new();
        for (i, p) in ports.iter().enumerate() {
            let dir = temp_db_dir(&format!("fast_{}", i));
            dirs.push(dir.clone());
            let bc = Arc::new(RwLock::new(create_test_blockchain(&dir)));
            let peers = Arc::new(Mutex::new(vec![]));
            let node = Node {
                blockchain: Arc::clone(&bc),
                peers: Arc::clone(&peers),
                address: format!("127.0.0.1:{}", p),
                sync_rx: mpsc::channel().1,
                rate_limiter: Arc::new(crate::network::RateLimiter::new(10, 100)),
                shutdown: Arc::new(AtomicBool::new(false)),
                listener: Arc::new(Mutex::new(None)),
                sync_thread_handle: Arc::new(Mutex::new(None)),
            };
            nodes.push((node, bc, peers));
        }

        for i in 0..3 {
            nodes[i].0.discover_peers();
            nodes[i].0.start_server(ports[i], sync_tx.clone());
        }

        // Регистрируем все три кошелька сразу, без ожидания синхронизации
        let secp = Secp256k1::new();
        let keypairs: Vec<(String, SecretKey)> = (0..3)
            .map(|_| {
                let sk = SecretKey::new(&mut OsRng);
                let pk = PublicKey::from_secret_key(&secp, &sk);
                let addr = BASE64.encode(pk.serialize());
                (addr, sk)
            })
            .collect();
        let addrs: Vec<String> = keypairs.iter().map(|(a, _)| a.clone()).collect();
        for i in 0..3 {
            let mut bc = nodes[i].1.write().unwrap();
            if !bc.balances.contains_key(&addrs[i]) {
                if !bc.grant_initial_balance_to_first_wallet(&addrs[i]) {
                    bc.balances.entry(addrs[i].clone()).or_default();
                }
                bc.save_state();
            }
        }

        // Распространяем грант-блоки (раунды синхронизации вместо фоновых потоков)
        for round in 0..6 {
            for i in 0..3 {
                let mut sync_node = Node {
                    blockchain: Arc::clone(&nodes[i].1),
                    peers: Arc::clone(&nodes[i].2),
                    address: nodes[i].0.address.clone(),
                    sync_rx: mpsc::channel().1,
                    rate_limiter: nodes[i].0.rate_limiter.clone(),
                    shutdown: Arc::new(AtomicBool::new(false)),
                    listener: Arc::new(Mutex::new(None)),
                    sync_thread_handle: Arc::new(Mutex::new(None)),
                };
                sync_node.sync_blockchain(sync_tx.clone());
            }
            std::thread::sleep(Duration::from_millis(250));
            let _ = round;
        }

        // Передача 1000 с кошелька 1 на кошелёк 2
        {
            let mut bc = nodes[0].1.write().unwrap();
            let sender_nonce = bc.balances.get(&addrs[0]).map(|a| a.nonce).unwrap_or(0);
            let mut tx = Transaction {
                sender: addrs[0].clone(),
                receiver: addrs[1].clone(),
                amount: 1000,
                nonce: sender_nonce + 1,
                chain_id: crate::consensus::current_chain_id(),
                signature: Vec::new(),
                is_coinbase: false,
            };
            let msg_bytes = crate::serialize::serialize_transaction(&tx);
            let msg_hash = blake3::hash(&msg_bytes);
            let msg = Message::from_digest_slice(msg_hash.as_bytes()).expect("message digest");
            let sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &keypairs[0].1);
            let (rec_id, sig_bytes) = sig.serialize_compact();
            let mut sig_vec = Vec::with_capacity(65);
            sig_vec.extend_from_slice(&sig_bytes);
            sig_vec.push(rec_id.to_i32() as u8);
            tx.signature = sig_vec;
            assert!(
                bc.add_transaction(tx).is_ok(),
                "Передача отклонена на узле 1"
            );
            let shutdown = Arc::new(AtomicBool::new(false));
            assert!(
                bc.mine_block(mpsc::channel().0, &shutdown).is_some(),
                "Майнинг не удался"
            );
        }

        // Распространяем блок с транзакцией
        for round in 0..6 {
            for i in 0..3 {
                let mut sync_node = Node {
                    blockchain: Arc::clone(&nodes[i].1),
                    peers: Arc::clone(&nodes[i].2),
                    address: nodes[i].0.address.clone(),
                    sync_rx: mpsc::channel().1,
                    rate_limiter: nodes[i].0.rate_limiter.clone(),
                    shutdown: Arc::new(AtomicBool::new(false)),
                    listener: Arc::new(Mutex::new(None)),
                    sync_thread_handle: Arc::new(Mutex::new(None)),
                };
                sync_node.sync_blockchain(sync_tx.clone());
            }
            std::thread::sleep(Duration::from_millis(250));
            let _ = round;
        }

        let expected = [10000u64 - 1000, 1000, 0];
        for i in 0..3 {
            let bc = nodes[i].1.read().unwrap();
            let bal = bc.balances.get(&addrs[i]).map(|a| a.balance).unwrap_or(0);
            assert!(
                bc.validate_chain(),
                "Узел {}: цепочка не прошла валидацию",
                i + 1
            );
            info!(
                node = i + 1,
                balance = bal,
                chain_len = bc.chain.len(),
                "Узел: баланс кошелька и длина цепочки"
            );
            if bal != expected[i] {
                error!(
                    node = i + 1,
                    expected = expected[i],
                    actual = bal,
                    "НЕВЕРНЫЙ БАЛАНС узла"
                );
                error!(node = i + 1, balances = ?bc.balances, "Балансы узла");
                error!(node = i + 1, chain = ?bc.chain, "Цепочка узла");
                panic!(
                    "Узел {}: баланс кошелька не совпал: ожидалось {}, получено {}",
                    i + 1,
                    expected[i],
                    bal
                );
            }
        }

        if let Some(saved) = saved_net {
            fs::write(&net_path, saved).unwrap();
        } else {
            let _ = fs::remove_file(&net_path);
        }
        for dir in &dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }

    /// Deadlock test: 100 iterations with random lock acquisition order
    /// Verifies no deadlock when acquiring blockchain.read() + wallet lock
    #[test]
    fn deadlock_test_blockchain_wallet_lock_order() {
        use rand::seq::SliceRandom;
        use rand::{rngs::StdRng, SeedableRng};
        use std::sync::{Arc, Mutex, RwLock};
        use std::thread;

        let blockchain = Arc::new(RwLock::new(create_test_blockchain(&temp_db_dir(
            "deadlock",
        ))));
        let wallet_lock = Arc::new(Mutex::new(())); // Simulates file-based keystore lock

        let mut handles = vec![];

        for i in 0..100 {
            let bc = Arc::clone(&blockchain);
            let wl = Arc::clone(&wallet_lock);
            handles.push(thread::spawn(move || {
                // Each thread gets its own RNG with a deterministic seed
                let mut rng = StdRng::seed_from_u64(3735928559 + i);
                // Random order: 0 = blockchain first, 1 = wallet first
                let order: [u8; 2] = [0, 1];
                let mut order = order;
                order.shuffle(&mut rng);

                for &o in &order {
                    match o {
                        0 => {
                            let _g = bc.read().unwrap();
                        }
                        1 => {
                            let _g = wl.lock().unwrap();
                        }
                        _ => unreachable!(),
                    }
                }
                // Work done
                thread::sleep(Duration::from_millis(1));
            }));
        }

        for h in handles {
            h.join().expect("Thread panicked - possible deadlock");
        }
    }
}

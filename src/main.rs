use serde::{Deserialize, Serialize, Deserializer};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use eframe::egui;
use std::io::{Read, Write};
use rand::Rng;
use std::fs;
use std::path::PathBuf;
use serde_json;
use uuid::Uuid;
use std::net::SocketAddr;
use rusty_leveldb::{DB, Options, LdbIterator};
use std::io::{BufReader, BufWriter};

// Структура для конфигурации
#[derive(Deserialize, Serialize)]
struct Config {
    wallet: WalletConfig,
}

#[derive(Deserialize, Serialize)]
struct WalletConfig {
    name: String,
    password: String,
    port: u16,
    ip: String,
}

// Структура для network.json
#[derive(Deserialize, Serialize)]
struct NetworkConfig {
    peers: Vec<String>,
}

// Структура блока
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Block {
    index: u64,
    timestamp: u64,
    transactions: Vec<Transaction>,
    previous_hash: String,
    hash: String,
    nonce: u64,
}

// Структура транзакции
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
struct Transaction {
    id: String,
    sender: String,
    receiver: String,
    amount: u64,
}

// Вспомогательная структура для десериализации Blockchain
#[derive(Deserialize)]
struct BlockchainDeserialize {
    chain: Vec<Block>,
    balances: HashMap<String, u64>,
    difficulty: u32,
    pending_transactions: Vec<Transaction>,
}

// Структура блокчейна
#[derive(Clone, Serialize)]
struct Blockchain {
    chain: Vec<Block>,
    balances: HashMap<String, u64>,
    difficulty: u32,
    pending_transactions: Vec<Transaction>,
    #[serde(skip)]
    db: Arc<Mutex<DB>>,
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
        } = BlockchainDeserialize::deserialize(deserializer)?;

        Ok(Blockchain {
            chain,
            balances,
            difficulty,
            pending_transactions,
            db: Arc::new(Mutex::new(DB::open(
                PathBuf::from("temp_blockchain_db"),
                Options::default(),
            ).map_err(serde::de::Error::custom)?)),
        })
    }
}

// Структура для команды майнинга
#[derive(Clone)]
struct MiningTask {
    blockchain: Arc<Mutex<Blockchain>>,
    transaction: Transaction,
    mining_status: Arc<Mutex<MiningStatus>>,
    progress_tx: mpsc::Sender<String>,
    status_tx: mpsc::Sender<String>,
}

// Структура узла
struct Node {
    blockchain: Arc<Mutex<Blockchain>>,
    peers: Arc<Mutex<Vec<String>>>,
    address: String,
    sync_rx: mpsc::Receiver<Blockchain>,
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
    fn new(address: String, mining_rx: mpsc::Receiver<MiningTask>, sync_tx: mpsc::Sender<Blockchain>, port: u16) -> Self {
        let blockchain = Arc::new(Mutex::new(Blockchain::new(port)));
        let peers = Arc::new(Mutex::new(vec![]));
        let (sync_tx_local, sync_rx) = mpsc::channel();
        let mut node = Node {
            blockchain: Arc::clone(&blockchain),
            peers,
            address,
            sync_rx,
        };
        // Load peers from network.json before spawning threads
        node.discover_peers();
        let blockchain_clone = Arc::clone(&blockchain);
        let peers_clone = Arc::clone(&node.peers);
        let address_clone = node.address.clone();
        let sync_tx_clone = sync_tx.clone();
        // Mining thread
        thread::spawn(move || {
            println!("Фоновый поток майнинга запущен в потоке {:?}", thread::current().id());
            let mut mining_count = 0;
            let mut total_duration = 0.0;
            let mut successful_mining = 0;
            loop {
                println!("Ожидание задачи майнинга...");
                match mining_rx.recv() {
                    Ok(task) => {
                        mining_count += 1;
                        println!("Получена задача майнинга {} в потоке {:?}", mining_count, thread::current().id());
                        let start_time = SystemTime::now();
                        let mut blockchain = task.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                        let block = blockchain.mine_block(task.progress_tx.clone());
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        total_duration += duration;
                        if let Some(block) = block {
                            successful_mining += 1;
                            let _ = task.status_tx.send(format!(
                                "Транзакция отправлена и замайнена за {} секунд",
                                duration
                            ));
                            let _ = task.progress_tx.send(format!("Блок успешно замайнен за {} секунд", duration));
                            println!(
                                "Майнинг {} успешен, среднее время: {} секунд, блок: {:?}",
                                mining_count,
                                total_duration / mining_count as f64,
                                block
                            );
                            // Broadcast new block
                            drop(blockchain); // Release blockchain lock before broadcasting
                            let peers = peers_clone.lock().expect("Не удалось захватить Mutex для peers").clone();
                            for peer in peers {
                                if peer == address_clone {
                                    continue; // Skip self
                                }
                                let addr: SocketAddr = match peer.parse() {
                                    Ok(addr) => addr,
                                    Err(e) => {
                                        println!("Некорректный адрес пира {}: {}", peer, e);
                                        continue;
                                    }
                                };
                                if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                                    let blockchain = blockchain_clone.lock().expect("Не удалось захватить Mutex для blockchain");
                                    let serialized_blockchain = serde_json::to_string(&*blockchain).expect("Ошибка сериализации блокчейна");
                                    let mut writer = BufWriter::new(stream);
                                    let message = format!("NEW_BLOCK:{}", serialized_blockchain);
                                    let length = message.len() as u32;
                                    let mut data = length.to_be_bytes().to_vec();
                                    data.extend_from_slice(message.as_bytes());
                                    if let Err(e) = writer.write_all(&data) {
                                        println!("Ошибка отправки блока узлу {}: {}", peer, e);
                                    } else {
                                        writer.flush().ok();
                                        println!("Блок отправлен узлу {}", peer);
                                    }
                                } else {
                                    println!("Узел {} недоступен для отправки блока", peer);
                                }
                            }
                        } else {
                            let _ = task.status_tx.send(format!(
                                "Ошибка майнинга транзакции за {} секунд",
                                duration
                            ));
                            println!(
                                "Майнинг {} не удался за {} секунд, всего успешно: {}, среднее время: {} секунд",
                                mining_count,
                                duration,
                                successful_mining,
                                total_duration / mining_count as f64
                            );
                        }
                        if let Ok(mut mining_status) = task.mining_status.lock() {
                            *mining_status = MiningStatus::Idle;
                            println!("Статус майнинга сброшен на Idle");
                        }
                    }
                    Err(e) => {
                        println!("Ошибка получения задачи майнинга: {}", e);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });
        // Synchronization thread
        let blockchain_clone_sync = Arc::clone(&blockchain);
        let peers_clone_sync = Arc::clone(&node.peers);
        let sync_tx_clone_sync = sync_tx.clone();
        thread::spawn(move || {
            loop {
                let mut blockchain = blockchain_clone_sync.lock().expect("Не удалось захватить Mutex для blockchain");
                let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                let current_chain_length = blockchain.chain.len();
                let current_last_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                let existing_db = blockchain.db.clone();
                let current_pending = blockchain.pending_transactions.clone();
                drop(blockchain);

                let peers = peers_clone_sync.lock().expect("Не удалось захватить Mutex для peers").clone();
                println!("Список пиров для синхронизации: {:?}", peers);

                for peer in peers.iter() {
                    let addr: SocketAddr = match peer.parse() {
                        Ok(addr) => addr,
                        Err(e) => {
                            println!("Некорректный адрес пира {}: {}", peer, e);
                            continue;
                        }
                    };
                    if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut writer = BufWriter::new(stream);

                        let message = "GET_BLOCKCHAIN";
                        let length = message.len() as u32;
                        let mut data = length.to_be_bytes().to_vec();
                        data.extend_from_slice(message.as_bytes());
                        if writer.write_all(&data).is_ok() {
                            writer.flush().ok();

                            let mut length_buf = [0; 4];
                            if reader.read_exact(&mut length_buf).is_ok() {
                                let length = u32::from_be_bytes(length_buf) as usize;
                                let mut buffer = vec![0; length];
                                let mut total_read = 0;
                                while total_read < length {
                                    let read = reader.read(&mut buffer[total_read..]).unwrap_or(0);
                                    if read == 0 {
                                        break;
                                    }
                                    total_read += read;
                                }
                                let response = String::from_utf8_lossy(&buffer[..total_read]).to_string();
                                println!("Получен ответ от узла {}: {}", peer, response);
                                match serde_json::from_str::<BlockchainDeserialize>(&response) {
                                    Ok(received_blockchain) => {
                                        let mut blockchain = blockchain_clone_sync.lock().expect("Не удалось захватить Mutex для blockchain");
                                        let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                        let received_last_timestamp = received_blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                                        if (received_blockchain.chain.len() > blockchain.chain.len() && received_last_timestamp > current_last_timestamp) ||
                                            (received_blockchain.chain.len() == blockchain.chain.len() && current_hash != received_hash && received_last_timestamp > current_last_timestamp) {
                                            let new_blockchain = Blockchain {
                                                chain: received_blockchain.chain.clone(),
                                                balances: received_blockchain.balances.clone(),
                                                difficulty: received_blockchain.difficulty,
                                                pending_transactions: vec![],
                                                db: existing_db.clone(),
                                            };
                                            if new_blockchain.validate_chain() {
                                                let mut valid_balances = true;
                                                for (address, balance) in &new_blockchain.balances {
                                                    if let Some(current_balance) = blockchain.balances.get(address) {
                                                        if *balance > *current_balance {
                                                            let mut total_received = 0;
                                                            for block in &new_blockchain.chain {
                                                                for tx in &block.transactions {
                                                                    if tx.receiver == *address {
                                                                        total_received += tx.amount;
                                                                    }
                                                                }
                                                            }
                                                            if total_received < *balance {
                                                                valid_balances = false;
                                                                println!("Недопустимый баланс для {}: получено {}, но указано {}",
                                                                         address, total_received, balance);
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                                if valid_balances {
                                                    let mut merged_transactions = current_pending.clone();
                                                    for tx in received_blockchain.pending_transactions {
                                                        if !merged_transactions.iter().any(|t| t.id == tx.id) {
                                                            if blockchain.add_transaction(tx.clone()) {
                                                                merged_transactions.push(tx);
                                                            } else {
                                                                println!("Транзакция {} от узла {} отклонена", tx.id, peer);
                                                            }
                                                        }
                                                    }
                                                    blockchain.chain = new_blockchain.chain;
                                                    blockchain.balances = new_blockchain.balances;
                                                    blockchain.difficulty = new_blockchain.difficulty;
                                                    blockchain.pending_transactions = merged_transactions;

                                                    let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                                    for tx in &blockchain.pending_transactions {
                                                        let key = tx.id.as_bytes();
                                                        let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                                                        if let Err(e) = db.put(key, &value) {
                                                            println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                                                        }
                                                    }
                                                    drop(db);
                                                    blockchain.save_state();
                                                    println!("Блокчейн обновлён с узла {}", peer);
                                                    let _ = sync_tx_clone_sync.send(Blockchain {
                                                        chain: blockchain.chain.clone(),
                                                        balances: blockchain.balances.clone(),
                                                        difficulty: blockchain.difficulty,
                                                        pending_transactions: blockchain.pending_transactions.clone(),
                                                        db: existing_db.clone(),
                                                    });
                                                } else {
                                                    println!("Полученный блокчейн с узла {} отклонён из-за некорректных балансов", peer);
                                                }
                                            } else {
                                                println!("Полученный блокчейн с узла {} не прошёл валидацию", peer);
                                            }
                                        } else {
                                            let mut merged_transactions = current_pending.clone();
                                            for tx in received_blockchain.pending_transactions {
                                                if !merged_transactions.iter().any(|t| t.id == tx.id) {
                                                    if blockchain.add_transaction(tx.clone()) {
                                                        merged_transactions.push(tx);
                                                    } else {
                                                        println!("Транзакция {} от узла {} отклонена", tx.id, peer);
                                                    }
                                                }
                                            }
                                            blockchain.pending_transactions = merged_transactions;
                                            blockchain.save_state();
                                            println!("Транзакции синхронизированы с узла {} без обновления цепочки", peer);
                                        }
                                    }
                                    Err(e) => println!("Ошибка десериализации блокчейна от узла {}: {}", peer, e),
                                }
                            }
                        }
                    } else {
                        println!("Узел {} недоступен", peer);
                    }
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        node
    }

    fn create_genesis_block(&mut self) {
        let start_time = SystemTime::now();
        let genesis_block = Block {
            index: 0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: vec![],
            previous_hash: "0".to_string(),
            hash: String::new(),
            nonce: 0,
        };
        let hash = self.calculate_hash(&genesis_block);
        let mut genesis_block = genesis_block;
        genesis_block.hash = hash;
        self.chain.push(genesis_block);
        self.balances.insert("wallet1".to_string(), 1000);
        self.balances.insert("wallet2".to_string(), 1000);
        self.balances.insert("wallet3".to_string(), 0);
        self.balances.insert("wallet4".to_string(), 0);
        self.balances.insert("wallet5".to_string(), 0);
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Создание генезис-блока завершено за {} секунд", duration);
    }

    fn calculate_hash(&self, block: &Block) -> String {
        let start_time = SystemTime::now();
        let input = format!(
            "{}{}{}{}{}",
            block.index,
            block.timestamp,
            serde_json::to_string(&block.transactions).unwrap(),
            block.previous_hash,
            block.nonce
        );
        let mut hasher = Sha256::new();
        hasher.update(input);
        let hash = format!("{:x}", hasher.finalize());
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Вычисление хэша завершено за {} секунд: {}", duration, hash);
        hash
    }

    fn add_transaction(&mut self, transaction: Transaction) -> bool {
        let start_time = SystemTime::now();
        println!("Начало добавления транзакции: {:?}", transaction);
        if transaction.sender.is_empty() || transaction.receiver.is_empty() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Ошибка: Пустой адрес отправителя или получателя (sender: {}, receiver: {}), проверка заняла {} секунд",
                     transaction.sender, transaction.receiver, duration);
            return false;
        }
        if self.pending_transactions.contains(&transaction) {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Транзакция уже существует в pending_transactions (ID: {}), проверка заняла {} секунд",
                     transaction.id, duration);
            return false;
        }
        let key = transaction.id.as_bytes();
        let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
        if db.get(key).is_some() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Транзакция с ID {} уже существует в LevelDB, проверка заняла {} секунд",
                     transaction.id, duration);
            return false;
        }
        if let Some(sender_balance) = self.balances.get(&transaction.sender) {
            println!("Баланс отправителя {}: {}", transaction.sender, sender_balance);
            if *sender_balance >= transaction.amount {
                let value = match serde_json::to_vec(&transaction) {
                    Ok(value) => value,
                    Err(e) => {
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Ошибка сериализации транзакции {}: {}, проверка заняла {} секунд",
                                 transaction.id, e, duration);
                        return false;
                    }
                };
                if let Err(e) = db.put(key, &value) {
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    println!("Ошибка сохранения транзакции {} в LevelDB: {}, проверка заняла {} секунд",
                             transaction.id, e, duration);
                    return false;
                }
                drop(db);
                self.pending_transactions.push(transaction);
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!("Транзакция добавлена и сохранена в LevelDB за {} секунд: {:?}",
                         duration, self.pending_transactions);
                return true;
            } else {
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Ошибка: Недостаточно средств (баланс: {}, требуется: {}), проверка заняла {} секунд",
                    sender_balance, transaction.amount, duration
                );
                return false;
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!(
            "Ошибка: Адрес отправителя {} не найден в balances, проверка заняла {} секунд",
            transaction.sender, duration
        );
        false
    }

    fn mine_block(&mut self, progress_tx: mpsc::Sender<String>) -> Option<Block> {
        let total_start_time = SystemTime::now();
        println!("Начало майнинга в потоке {:?}", thread::current().id());
        if self.pending_transactions.is_empty() {
            println!("Нет транзакций для майнинга");
            let _ = progress_tx.send("Нет транзакций для майнинга".to_string());
            return None;
        }

        let previous_block = self.chain.last().unwrap().clone();
        let transactions = self.pending_transactions.clone();
        let difficulty = self.difficulty;

        println!("Транзакции для майнинга: {:?}", transactions);
        let block = self.mine_block_inner(previous_block, transactions, difficulty, progress_tx.clone());

        if let Some(mut block) = block {
            let balance_start_time = SystemTime::now();
            println!("Начало обновления балансов для блока: {:?}", block);
            for tx in &block.transactions {
                let sender_balance = self.balances.get(&tx.sender).cloned().unwrap_or(0);
                if sender_balance < tx.amount {
                    println!("Ошибка: Недостаточно средств у {} для транзакции {} (баланс: {}, требуется: {})",
                             tx.sender, tx.id, sender_balance, tx.amount);
                    return None;
                }
                let sender_final = sender_balance - tx.amount;
                *self.balances.entry(tx.sender.clone()).or_insert(0) = sender_final;

                let receiver_balance = self.balances.get(&tx.receiver).cloned().unwrap_or(0);
                let receiver_final = receiver_balance + tx.amount;
                *self.balances.entry(tx.receiver.clone()).or_insert(0) = receiver_final;

                println!(
                    "Обновлён баланс: {} -> {}, {} -> {}",
                    tx.sender, sender_final, tx.receiver, receiver_final
                );
            }
            let balance_duration = SystemTime::now()
                .duration_since(balance_start_time)
                .unwrap()
                .as_secs_f64();
            println!("Обновление балансов завершено за {} секунд", balance_duration);

            // Добавляем блок в цепочку перед сохранением
            self.chain.push(block.clone());

            let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
            for tx in &self.pending_transactions {
                let key = tx.id.as_bytes();
                println!("Удаление ключа: {:?}", key);
                if key.is_empty() {
                    println!("Ошибка: пустой ключ для транзакции {}", tx.id);
                    continue;
                }
                if let Err(e) = db.delete(key) {
                    println!("Ошибка удаления транзакции {} из LevelDB: {}", tx.id, e);
                }
            }
            // Сохранение состояния после майнинга
            if let Err(e) = db.put(b"chain", &serde_json::to_vec(&self.chain).unwrap()) {
                println!("Ошибка сохранения цепочки блоков в LevelDB: {}", e);
            }
            if let Err(e) = db.put(b"balances", &serde_json::to_vec(&self.balances).unwrap()) {
                println!("Ошибка сохранения балансов в LevelDB: {}", e);
            }
            if let Err(e) = db.put(b"difficulty", &serde_json::to_vec(&self.difficulty).unwrap()) {
                println!("Ошибка сохранения сложности в LevelDB: {}", e);
            }
            drop(db);
            self.pending_transactions.clear();
            let total_duration = SystemTime::now()
                .duration_since(total_start_time)
                .unwrap()
                .as_secs_f64();
            println!("Майнинг завершен за {} секунд, блок добавлен: {:?}", total_duration, block);
            let _ = progress_tx.send(format!("Майнинг завершен за {} секунд", total_duration));
            Some(block)
        } else {
            let total_duration = SystemTime::now()
                .duration_since(total_start_time)
                .unwrap()
                .as_secs_f64();
            println!("Майнинг не удался за {} секунд", total_duration);
            let _ = progress_tx.send(format!("Майнинг не удался за {} секунд", total_duration));
            None
        }
    }

    fn mine_block_inner(
        &self,
        previous_block: Block,
        transactions: Vec<Transaction>,
        difficulty: u32,
        progress_tx: mpsc::Sender<String>,
    ) -> Option<Block> {
        let start_time = SystemTime::now();
        println!("Начало mine_block_inner в потоке {:?}", thread::current().id());
        let mut block = Block {
            index: previous_block.index + 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions,
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
        };

        let max_iterations = 1000;
        let timeout = Duration::from_secs(5);
        let mut iteration_count = 0;
        let mut total_hash_time = 0.0;

        loop {
            if iteration_count >= max_iterations {
                let total_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!("Достигнуто максимальное количество итераций: {} за {} секунд", max_iterations, total_duration);
                let _ = progress_tx.send(format!("Достигнуто максимальное количество итераций: {} за {} секунд", max_iterations, total_duration));
                return None;
            }
            if SystemTime::now().duration_since(start_time).unwrap() > timeout {
                let total_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!("Майнинг прерван: превышен таймаут {} секунд, всего итераций: {}", timeout.as_secs(), iteration_count);
                let _ = progress_tx.send(format!("Майнинг прерван: превышен таймаут {} секунд, всего итераций: {}", timeout.as_secs(), iteration_count));
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
            println!(
                "Итерация {}, nonce: {}, хэш: {}, время вычисления хэша: {} секунд",
                iteration_count, block.nonce, hash, hash_duration
            );
            if hash.starts_with(&"0".repeat(difficulty as usize)) {
                block.hash = hash;
                let total_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                let avg_hash_time = if iteration_count > 0 { total_hash_time / iteration_count as f64 } else { 0.0 };
                println!(
                    "Подходящий хэш найден после {} итераций за {} секунд, среднее время хэширования: {} секунд",
                    iteration_count, total_duration, avg_hash_time
                );
                let _ = progress_tx.send(format!(
                    "Подходящий хэш найден после {} итераций за {} секунд",
                    iteration_count, total_duration
                ));
                return Some(block);
            }
            block.nonce += 1;
        }
    }

    fn validate_chain(&self) -> bool {
        let start_time = SystemTime::now();
        for i in 1..self.chain.len() {
            let current_block = &self.chain[i];
            let previous_block = &self.chain[i - 1];

            if current_block.previous_hash != previous_block.hash {
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Недействительная цепочка: неверный previous_hash в блоке {}, проверка заняла {} секунд",
                    i, duration
                );
                return false;
            }

            let calculated_hash = self.calculate_hash(current_block);
            if current_block.hash != calculated_hash {
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Недействительная цепочка: неверный хэш в блоке {}, проверка заняла {} секунд",
                    i, duration
                );
                return false;
            }

            if !current_block.hash.starts_with(&"0".repeat(self.difficulty as usize)) {
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Недействительная цепочка: неверная сложность в блоке {}, проверка заняла {} секунд",
                    i, duration
                );
                return false;
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Цепочка валидна, проверка заняла {} секунд", duration);
        true
    }

    fn save_state(&self) {
        let start_time = SystemTime::now();
        let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
        if let Err(e) = db.put(b"chain", &serde_json::to_vec(&self.chain).unwrap()) {
            println!("Ошибка сохранения цепочки блоков в LevelDB: {}", e);
        }
        if let Err(e) = db.put(b"balances", &serde_json::to_vec(&self.balances).unwrap()) {
            println!("Ошибка сохранения балансов в LevelDB: {}", e);
        }
        if let Err(e) = db.put(b"difficulty", &serde_json::to_vec(&self.difficulty).unwrap()) {
            println!("Ошибка сохранения сложности в LevelDB: {}", e);
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Сохранение состояния блокчейна завершено за {} секунд", duration);
    }
}

impl Node {
    fn new(address: String, mining_rx: mpsc::Receiver<MiningTask>, sync_tx: mpsc::Sender<Blockchain>, port: u16) -> Self {
        let blockchain = Arc::new(Mutex::new(Blockchain::new(port)));
        let peers = Arc::new(Mutex::new(vec![]));
        let (sync_tx_local, sync_rx) = mpsc::channel();
        let node = Node {
            blockchain: Arc::clone(&blockchain),
            peers,
            address,
            sync_rx,
        };
        let blockchain_clone = Arc::clone(&blockchain);
        let peers_clone = Arc::clone(&node.peers);
        let address_clone = node.address.clone();
        let sync_tx_clone = sync_tx.clone();
        // Mining thread
        thread::spawn(move || {
            println!("Фоновый поток майнинга запущен в потоке {:?}", thread::current().id());
            let mut mining_count = 0;
            let mut total_duration = 0.0;
            let mut successful_mining = 0;
            loop {
                println!("Ожидание задачи майнинга...");
                match mining_rx.recv() {
                    Ok(task) => {
                        mining_count += 1;
                        println!("Получена задача майнинга {} в потоке {:?}", mining_count, thread::current().id());
                        let start_time = SystemTime::now();
                        let mut blockchain = task.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                        let block = blockchain.mine_block(task.progress_tx.clone());
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        total_duration += duration;
                        if let Some(block) = block {
                            successful_mining += 1;
                            let _ = task.status_tx.send(format!(
                                "Транзакция отправлена и замайнена за {} секунд",
                                duration
                            ));
                            let _ = task.progress_tx.send(format!("Блок успешно замайнен за {} секунд", duration));
                            println!(
                                "Майнинг {} успешен, среднее время: {} секунд, блок: {:?}",
                                mining_count,
                                total_duration / mining_count as f64,
                                block
                            );
                            // Broadcast new block
                            drop(blockchain); // Release blockchain lock before broadcasting
                            let peers = peers_clone.lock().expect("Не удалось захватить Mutex для peers").clone();
                            for peer in peers {
                                if peer == address_clone {
                                    continue; // Skip self
                                }
                                let addr: SocketAddr = match peer.parse() {
                                    Ok(addr) => addr,
                                    Err(e) => {
                                        println!("Некорректный адрес пира {}: {}", peer, e);
                                        continue;
                                    }
                                };
                                if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                                    let blockchain = blockchain_clone.lock().expect("Не удалось захватить Mutex для blockchain");
                                    let serialized_blockchain = serde_json::to_string(&*blockchain).expect("Ошибка сериализации блокчейна");
                                    let mut writer = BufWriter::new(stream);
                                    let message = format!("NEW_BLOCK:{}", serialized_blockchain);
                                    let length = message.len() as u32;
                                    let mut data = length.to_be_bytes().to_vec();
                                    data.extend_from_slice(message.as_bytes());
                                    if let Err(e) = writer.write_all(&data) {
                                        println!("Ошибка отправки блока узлу {}: {}", peer, e);
                                    } else {
                                        writer.flush().ok();
                                        println!("Блок отправлен узлу {}", peer);
                                    }
                                } else {
                                    println!("Узел {} недоступен для отправки блока", peer);
                                }
                            }
                        } else {
                            let _ = task.status_tx.send(format!(
                                "Ошибка майнинга транзакции за {} секунд",
                                duration
                            ));
                            println!(
                                "Майнинг {} не удался за {} секунд, всего успешно: {}, среднее время: {} секунд",
                                mining_count,
                                duration,
                                successful_mining,
                                total_duration / mining_count as f64
                            );
                        }
                        if let Ok(mut mining_status) = task.mining_status.lock() {
                            *mining_status = MiningStatus::Idle;
                            println!("Статус майнинга сброшен на Idle");
                        }
                    }
                    Err(e) => {
                        println!("Ошибка получения задачи майнинга: {}", e);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });
        // Synchronization thread
        let blockchain_clone_sync = Arc::clone(&blockchain);
        let peers_clone_sync = Arc::clone(&node.peers);
        let sync_tx_clone_sync = sync_tx.clone();
        thread::spawn(move || {
            loop {
                let mut blockchain = blockchain_clone_sync.lock().expect("Не удалось захватить Mutex для blockchain");
                let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                let current_chain_length = blockchain.chain.len();
                let current_last_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                let existing_db = blockchain.db.clone();
                let current_pending = blockchain.pending_transactions.clone();
                drop(blockchain);

                let peers = peers_clone_sync.lock().expect("Не удалось захватить Mutex для peers").clone();
                println!("Список пиров для синхронизации: {:?}", peers);

                for peer in peers.iter() {
                    let addr: SocketAddr = match peer.parse() {
                        Ok(addr) => addr,
                        Err(e) => {
                            println!("Некорректный адрес пира {}: {}", peer, e);
                            continue;
                        }
                    };
                    if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut writer = BufWriter::new(stream);

                        let message = "GET_BLOCKCHAIN";
                        let length = message.len() as u32;
                        let mut data = length.to_be_bytes().to_vec();
                        data.extend_from_slice(message.as_bytes());
                        if writer.write_all(&data).is_ok() {
                            writer.flush().ok();

                            let mut length_buf = [0; 4];
                            if reader.read_exact(&mut length_buf).is_ok() {
                                let length = u32::from_be_bytes(length_buf) as usize;
                                let mut buffer = vec![0; length];
                                let mut total_read = 0;
                                while total_read < length {
                                    let read = reader.read(&mut buffer[total_read..]).unwrap_or(0);
                                    if read == 0 {
                                        break;
                                    }
                                    total_read += read;
                                }
                                let response = String::from_utf8_lossy(&buffer[..total_read]).to_string();
                                println!("Получен ответ от узла {}: {}", peer, response);
                                match serde_json::from_str::<BlockchainDeserialize>(&response) {
                                    Ok(received_blockchain) => {
                                        let mut blockchain = blockchain_clone_sync.lock().expect("Не удалось захватить Mutex для blockchain");
                                        let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                        let received_last_timestamp = received_blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                                        if (received_blockchain.chain.len() > blockchain.chain.len() && received_last_timestamp > current_last_timestamp) ||
                                            (received_blockchain.chain.len() == blockchain.chain.len() && current_hash != received_hash && received_last_timestamp > current_last_timestamp) {
                                            let new_blockchain = Blockchain {
                                                chain: received_blockchain.chain.clone(),
                                                balances: received_blockchain.balances.clone(),
                                                difficulty: received_blockchain.difficulty,
                                                pending_transactions: vec![],
                                                db: existing_db.clone(),
                                            };
                                            if new_blockchain.validate_chain() {
                                                let mut valid_balances = true;
                                                for (address, balance) in &new_blockchain.balances {
                                                    if let Some(current_balance) = blockchain.balances.get(address) {
                                                        if *balance > *current_balance {
                                                            let mut total_received = 0;
                                                            for block in &new_blockchain.chain {
                                                                for tx in &block.transactions {
                                                                    if tx.receiver == *address {
                                                                        total_received += tx.amount;
                                                                    }
                                                                }
                                                            }
                                                            if total_received < *balance {
                                                                valid_balances = false;
                                                                println!("Недопустимый баланс для {}: получено {}, но указано {}",
                                                                         address, total_received, balance);
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                                if valid_balances {
                                                    let mut merged_transactions = current_pending.clone();
                                                    for tx in received_blockchain.pending_transactions {
                                                        if !merged_transactions.iter().any(|t| t.id == tx.id) {
                                                            if blockchain.add_transaction(tx.clone()) {
                                                                merged_transactions.push(tx);
                                                            } else {
                                                                println!("Транзакция {} от узла {} отклонена", tx.id, peer);
                                                            }
                                                        }
                                                    }
                                                    blockchain.chain = new_blockchain.chain;
                                                    blockchain.balances = new_blockchain.balances;
                                                    blockchain.difficulty = new_blockchain.difficulty;
                                                    blockchain.pending_transactions = merged_transactions;

                                                    let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                                    for tx in &blockchain.pending_transactions {
                                                        let key = tx.id.as_bytes();
                                                        let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                                                        if let Err(e) = db.put(key, &value) {
                                                            println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                                                        }
                                                    }
                                                    drop(db);
                                                    blockchain.save_state();
                                                    println!("Блокчейн обновлён с узла {}", peer);
                                                    let _ = sync_tx_clone_sync.send(Blockchain {
                                                        chain: blockchain.chain.clone(),
                                                        balances: blockchain.balances.clone(),
                                                        difficulty: blockchain.difficulty,
                                                        pending_transactions: blockchain.pending_transactions.clone(),
                                                        db: existing_db.clone(),
                                                    });
                                                } else {
                                                    println!("Полученный блокчейн с узла {} отклонён из-за некорректных балансов", peer);
                                                }
                                            } else {
                                                println!("Полученный блокчейн с узла {} не прошёл валидацию", peer);
                                            }
                                        } else {
                                            let mut merged_transactions = current_pending.clone();
                                            for tx in received_blockchain.pending_transactions {
                                                if !merged_transactions.iter().any(|t| t.id == tx.id) {
                                                    if blockchain.add_transaction(tx.clone()) {
                                                        merged_transactions.push(tx);
                                                    } else {
                                                        println!("Транзакция {} от узла {} отклонена", tx.id, peer);
                                                    }
                                                }
                                            }
                                            blockchain.pending_transactions = merged_transactions;
                                            blockchain.save_state();
                                            println!("Транзакции синхронизированы с узла {} без обновления цепочки", peer);
                                        }
                                    }
                                    Err(e) => println!("Ошибка десериализации блокчейна от узла {}: {}", peer, e),
                                }
                            }
                        }
                    } else {
                        println!("Узел {} недоступен", peer);
                    }
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        node
    }

    fn discover_peers(&mut self) {
        let start_time = SystemTime::now();
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let network_path = exe_dir.join("network.json");
        let network_config: NetworkConfig = match fs::read_to_string(&network_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|err| {
                eprintln!("Ошибка парсинга network.json: {}. Используются значения по умолчанию.", err);
                NetworkConfig {
                    peers: vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()],
                }
            }),
            Err(err) => {
                eprintln!("Ошибка чтения network.json: {}. Используются значения по умолчанию.", err);
                NetworkConfig {
                    peers: vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()],
                }
            }
        };
        let mut peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        *peers = network_config.peers;
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Обнаружение пиров завершено за {} секунд: {:?}", duration, *peers);
    }


    fn add_peer(&mut self, address: String) -> bool {
        let start_time = SystemTime::now();
        let mut peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        if peers.contains(&address) {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Пир {} уже существует, добавление заняло {} секунд", address, duration);
            return false;
        }
        peers.push(address.clone());
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let network_path = exe_dir.join("network.json");
        let network_config = NetworkConfig {
            peers: peers.clone(),
        };
        let network_content = serde_json::to_string_pretty(&network_config).expect("Ошибка сериализации network.json");
        fs::write(&network_path, network_content).expect("Ошибка записи в network.json");
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Пир {} добавлен в network.json за {} секунд", address, duration);
        true
    }

    fn find_wallet_by_ip(&self, ip: &str, port: u16) -> Option<String> {
        let start_time = SystemTime::now();
        let addr = format!("{}:{}", ip, port);
        let peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        if peers.contains(&addr) {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Кошелёк для {}:{} уже в списке пиров, поиск занял {} секунд", ip, port, duration);
            return Some(format!("wallet{}", port % 5 + 1));
        }
        drop(peers);

        if let Ok(stream) = TcpStream::connect_timeout(&addr.parse::<SocketAddr>().unwrap(), Duration::from_secs(1)) {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = BufWriter::new(stream);

            let message = "GET_BLOCKCHAIN";
            let length = message.len() as u32;
            let mut data = length.to_be_bytes().to_vec();
            data.extend_from_slice(message.as_bytes());
            if writer.write_all(&data).is_ok() {
                writer.flush().ok();

                let mut length_buf = [0; 4];
                if reader.read_exact(&mut length_buf).is_ok() {
                    let length = u32::from_be_bytes(length_buf) as usize;
                    let mut buffer = vec![0; length];
                    let mut total_read = 0;
                    while total_read < length {
                        let read = reader.read(&mut buffer[total_read..]).unwrap_or(0);
                        if read == 0 {
                            break;
                        }
                        total_read += read;
                    }
                    let response = String::from_utf8_lossy(&buffer[..total_read]).to_string();
                    match serde_json::from_str::<BlockchainDeserialize>(&response) {
                        Ok(_) => {
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            println!("Кошелёк для {}:{} найден, поиск занял {} секунд", ip, port, duration);
                            return Some(format!("wallet{}", port % 5 + 1));
                        }
                        Err(e) => println!("Ошибка десериализации блокчейна от {}:{}: {}", ip, port, e),
                    }
                }
            }
        } else {
            println!("Не удалось подключиться к {}:{}", ip, port);
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Кошелёк для {}:{} не найден, поиск занял {} секунд", ip, port, duration);
        None
    }

    fn start_server(&mut self, port: u16, sync_tx: mpsc::Sender<Blockchain>) {
        let start_time = SystemTime::now();
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).expect("Не удалось запустить сервер");
        println!("Сервер запущен на порту {}", port);

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let blockchain = Arc::clone(&self.blockchain);
                    let sync_tx = sync_tx.clone();
                    thread::spawn(move || {
                        let mut reader = BufReader::new(&stream);
                        let mut writer = BufWriter::new(&stream);
                        let mut length_buf = [0; 4];
                        if reader.read_exact(&mut length_buf).is_ok() {
                            let length = u32::from_be_bytes(length_buf) as usize;
                            let mut buffer = vec![0; length];
                            let mut total_read = 0;
                            while total_read < length {
                                let read = reader.read(&mut buffer[total_read..]).unwrap_or(0);
                                if read == 0 {
                                    break;
                                }
                                total_read += read;
                            }
                            let message = String::from_utf8_lossy(&buffer[..total_read]).to_string();
                            println!("Получен запрос: {}", message);
                            if message == "GET_BLOCKCHAIN" {
                                let blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                let serialized_blockchain = serde_json::to_string(&*blockchain).expect("Ошибка сериализации блокчейна");
                                let length = serialized_blockchain.len() as u32;
                                let mut data = length.to_be_bytes().to_vec();
                                data.extend_from_slice(serialized_blockchain.as_bytes());
                                if writer.write_all(&data).is_ok() {
                                    writer.flush().ok();
                                    println!("Отправлен блокчейн клиенту");
                                }
                            } else if message.starts_with("NEW_BLOCK:") {
                                let serialized_blockchain = message.strip_prefix("NEW_BLOCK:").unwrap_or("");
                                match serde_json::from_str::<BlockchainDeserialize>(serialized_blockchain) {
                                    Ok(received_blockchain) => {
                                        let mut blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                        let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                        let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                        let current_last_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                                        let received_last_timestamp = received_blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                                        if (received_blockchain.chain.len() > blockchain.chain.len() && received_last_timestamp > current_last_timestamp) ||
                                            (received_blockchain.chain.len() == blockchain.chain.len() && current_hash != received_hash && received_last_timestamp > current_last_timestamp) {
                                            let new_blockchain = Blockchain {
                                                chain: received_blockchain.chain.clone(),
                                                balances: received_blockchain.balances.clone(),
                                                difficulty: received_blockchain.difficulty,
                                                pending_transactions: vec![],
                                                db: blockchain.db.clone(),
                                            };
                                            if new_blockchain.validate_chain() {
                                                let mut valid_balances = true;
                                                for (address, balance) in &new_blockchain.balances {
                                                    if let Some(current_balance) = blockchain.balances.get(address) {
                                                        if *balance > *current_balance {
                                                            let mut total_received = 0;
                                                            for block in &new_blockchain.chain {
                                                                for tx in &block.transactions {
                                                                    if tx.receiver == *address {
                                                                        total_received += tx.amount;
                                                                    }
                                                                }
                                                            }
                                                            if total_received < *balance {
                                                                valid_balances = false;
                                                                println!("Недопустимый баланс для {}: получено {}, но указано {}",
                                                                         address, total_received, balance);
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                                if valid_balances {
                                                    let mut merged_transactions = blockchain.pending_transactions.clone();
                                                    for tx in received_blockchain.pending_transactions {
                                                        if !merged_transactions.iter().any(|t| t.id == tx.id) {
                                                            if blockchain.add_transaction(tx.clone()) {
                                                                merged_transactions.push(tx);
                                                            }
                                                        }
                                                    }
                                                    blockchain.chain = new_blockchain.chain;
                                                    blockchain.balances = new_blockchain.balances;
                                                    blockchain.difficulty = new_blockchain.difficulty;
                                                    blockchain.pending_transactions = merged_transactions;
                                                    blockchain.save_state();
                                                    let _ = sync_tx.send(Blockchain {
                                                        chain: blockchain.chain.clone(),
                                                        balances: blockchain.balances.clone(),
                                                        difficulty: blockchain.difficulty,
                                                        pending_transactions: blockchain.pending_transactions.clone(),
                                                        db: blockchain.db.clone(),
                                                    });
                                                    println!("Блокчейн обновлён через NEW_BLOCK");
                                                } else {
                                                    println!("Полученный блокчейн через NEW_BLOCK отклонён из-за некорректных балансов");
                                                }
                                            } else {
                                                println!("Полученный блокчейн через NEW_BLOCK не прошёл валидацию");
                                            }
                                        }
                                    }
                                    Err(e) => println!("Ошибка десериализации блока: {}", e),
                                }
                            }
                        }
                    });
                }
                Err(e) => println!("Ошибка подключения клиента: {}", e),
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Сервер завершил работу за {} секунд", duration);
    }

    fn sync_blockchain(&mut self, sync_tx: mpsc::Sender<Blockchain>) {
        let start_time = SystemTime::now();
        self.discover_peers();
        let peers = self.peers.lock().expect("Не удалось захватить Mutex для peers").clone();
        println!("Список пиров для синхронизации: {:?}", peers);

        let blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
        let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
        let current_chain_length = blockchain.chain.len();
        let current_last_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
        let existing_db = blockchain.db.clone();
        let current_pending = blockchain.pending_transactions.clone();
        drop(blockchain);

        for peer in peers.iter() {
            let addr: SocketAddr = match peer.parse() {
                Ok(addr) => addr,
                Err(e) => {
                    println!("Некорректный адрес пира {}: {}", peer, e);
                    continue;
                }
            };
            if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = BufWriter::new(stream);

                let message = "GET_BLOCKCHAIN";
                let length = message.len() as u32;
                let mut data = length.to_be_bytes().to_vec();
                data.extend_from_slice(message.as_bytes());
                if writer.write_all(&data).is_ok() {
                    writer.flush().ok();

                    let mut length_buf = [0; 4];
                    if reader.read_exact(&mut length_buf).is_ok() {
                        let length = u32::from_be_bytes(length_buf) as usize;
                        let mut buffer = vec![0; length];
                        let mut total_read = 0;
                        while total_read < length {
                            let read = reader.read(&mut buffer[total_read..]).unwrap_or(0);
                            if read == 0 {
                                break;
                            }
                            total_read += read;
                        }
                        let response = String::from_utf8_lossy(&buffer[..total_read]).to_string();
                        println!("Получен ответ от узла {}: {}", peer, response);
                        match serde_json::from_str::<BlockchainDeserialize>(&response) {
                            Ok(received_blockchain) => {
                                let mut blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                let received_last_timestamp = received_blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                                if (received_blockchain.chain.len() > blockchain.chain.len() && received_last_timestamp > current_last_timestamp) ||
                                    (received_blockchain.chain.len() == blockchain.chain.len() && current_hash != received_hash && received_last_timestamp > current_last_timestamp) {
                                    let new_blockchain = Blockchain {
                                        chain: received_blockchain.chain.clone(),
                                        balances: received_blockchain.balances.clone(),
                                        difficulty: received_blockchain.difficulty,
                                        pending_transactions: vec![],
                                        db: existing_db.clone(),
                                    };
                                    if new_blockchain.validate_chain() {
                                        let mut valid_balances = true;
                                        for (address, balance) in &new_blockchain.balances {
                                            if let Some(current_balance) = blockchain.balances.get(address) {
                                                if *balance > *current_balance {
                                                    let mut total_received = 0;
                                                    for block in &new_blockchain.chain {
                                                        for tx in &block.transactions {
                                                            if tx.receiver == *address {
                                                                total_received += tx.amount;
                                                            }
                                                        }
                                                    }
                                                    if total_received < *balance {
                                                        valid_balances = false;
                                                        println!("Недопустимый баланс для {}: получено {}, но указано {}",
                                                                 address, total_received, balance);
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        if valid_balances {
                                            let mut merged_transactions = current_pending.clone();
                                            for tx in received_blockchain.pending_transactions {
                                                if !merged_transactions.iter().any(|t| t.id == tx.id) {
                                                    if blockchain.add_transaction(tx.clone()) {
                                                        merged_transactions.push(tx);
                                                    } else {
                                                        println!("Транзакция {} от узла {} отклонена", tx.id, peer);
                                                    }
                                                }
                                            }
                                            blockchain.chain = new_blockchain.chain;
                                            blockchain.balances = new_blockchain.balances;
                                            blockchain.difficulty = new_blockchain.difficulty;
                                            blockchain.pending_transactions = merged_transactions;

                                            let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                            for tx in &blockchain.pending_transactions {
                                                let key = tx.id.as_bytes();
                                                let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                                                if let Err(e) = db.put(key, &value) {
                                                    println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                                                }
                                            }
                                            drop(db);
                                            blockchain.save_state();
                                            println!("Блокчейн обновлён с узла {}", peer);
                                            let _ = sync_tx.send(Blockchain {
                                                chain: blockchain.chain.clone(),
                                                balances: blockchain.balances.clone(),
                                                difficulty: blockchain.difficulty,
                                                pending_transactions: blockchain.pending_transactions.clone(),
                                                db: existing_db.clone(),
                                            });
                                        } else {
                                            println!("Полученный блокчейн с узла {} отклонён из-за некорректных балансов", peer);
                                        }
                                    } else {
                                        println!("Полученный блокчейн с узла {} не прошёл валидацию", peer);
                                    }
                                } else {
                                    let mut merged_transactions = current_pending.clone();
                                    for tx in received_blockchain.pending_transactions {
                                        if !merged_transactions.iter().any(|t| t.id == tx.id) {
                                            if blockchain.add_transaction(tx.clone()) {
                                                merged_transactions.push(tx);
                                            } else {
                                                println!("Транзакция {} от узла {} отклонена", tx.id, peer);
                                            }
                                        }
                                    }
                                    blockchain.pending_transactions = merged_transactions;
                                    blockchain.save_state();
                                    println!("Транзакции синхронизированы с узла {} без обновления цепочки", peer);
                                }
                            }
                            Err(e) => println!("Ошибка десериализации блокчейна от узла {}: {}", peer, e),
                        }
                    }
                }
            } else {
                println!("Узел {} недоступен", peer);
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Синхронизация блокчейна завершена за {} секунд", duration);
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
            let mut blockchain = self.node.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
            let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
            let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
            let current_last_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
            let received_last_timestamp = received_blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
            if (received_blockchain.chain.len() > blockchain.chain.len() && received_last_timestamp > current_last_timestamp) ||
                (received_blockchain.chain.len() == blockchain.chain.len() && current_hash != received_hash && received_last_timestamp > current_last_timestamp) {
                if received_blockchain.validate_chain() {
                    let mut valid_balances = true;
                    for (address, balance) in &received_blockchain.balances {
                        if let Some(current_balance) = blockchain.balances.get(address) {
                            if *balance > *current_balance {
                                let mut total_received = 0;
                                for block in &received_blockchain.chain {
                                    for tx in &block.transactions {
                                        if tx.receiver == *address {
                                            total_received += tx.amount;
                                        }
                                    }
                                }
                                if total_received < *balance {
                                    valid_balances = false;
                                    println!("Недопустимый баланс для {}: получено {}, но указано {}",
                                             address, total_received, balance);
                                    break;
                                }
                            }
                        }
                    }
                    if valid_balances {
                        let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                        for tx in &received_blockchain.pending_transactions {
                            let key = tx.id.as_bytes();
                            let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                            if let Err(e) = db.put(key, &value) {
                                println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                            }
                        }
                        drop(db);
                        *blockchain = received_blockchain;
                        blockchain.save_state();
                        println!("UI: Блокчейн обновлён через канал синхронизации");
                        ctx.request_repaint();
                    } else {
                        println!("Полученный блокчейн через канал синхронизации отклонён из-за некорректных балансов");
                    }
                } else {
                    println!("Полученный блокчейн через канал синхронизации не прошёл валидацию");
                }
            }
        }

        if let Some(ref status_rx) = self.status_rx {
            while let Ok(status) = status_rx.try_recv() {
                self.status = status;
                println!("Статус обновлён в UI: {}", self.status);
                ctx.request_repaint();
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.is_authenticated {
                ui.heading("Аутентификация");
                ui.text_edit_singleline(&mut self.password);
                if ui.button("Войти").clicked() {
                    let start_time = SystemTime::now();
                    println!("Кнопка 'Войти' нажата, пароль: {}", self.password);
                    if self.password == "password" {
                        self.is_authenticated = true;
                        self.status = "Успешная аутентификация".to_string();
                        self.node.discover_peers();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Аутентификация успешна за {} секунд", duration);
                    } else {
                        self.status = "Неверный пароль".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Аутентификация не удалась за {} секунд: неверный пароль", duration);
                    }
                    ctx.request_repaint();
                }
            } else {
                ui.heading("Кошелёк");
                ui.label(format!("Адрес: {}", self.wallet_address));
                let balance = {
                    let blockchain = self.node.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                    *blockchain.balances.get(&self.wallet_address).unwrap_or(&0)
                };
                ui.label(format!("Баланс: {}", balance));

                ui.heading("Перевод");
                ui.text_edit_singleline(&mut self.receiver_address);
                ui.text_edit_singleline(&mut self.amount);

                if let Some(ref progress_rx) = self.progress_rx {
                    while let Ok(progress) = progress_rx.try_recv() {
                        let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                        *mining_progress = Some(progress);
                        println!("Прогресс майнинга обновлён в UI: {:?}", *mining_progress);
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
                    println!(
                        "Кнопка 'Отправить' нажата, получатель: {}, сумма: {}",
                        self.receiver_address, self.amount
                    );
                    if self.receiver_address.trim().is_empty() {
                        self.status = "Адрес получателя не может быть пустым".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Ошибка: пустой адрес получателя, проверка заняла {} секунд", duration);
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
                            println!("Ошибка: сумма равна нулю, проверка заняла {} секунд", duration);
                            ctx.request_repaint();
                            return;
                        }
                        let transaction = Transaction {
                            id: Uuid::new_v4().to_string(),
                            sender: self.wallet_address.clone(),
                            receiver: self.receiver_address.trim().to_string(),
                            amount,
                        };
                        let blockchain = Arc::clone(&self.node.blockchain);
                        let mining_status = Arc::clone(&self.mining_status);
                        let mining_progress = Arc::clone(&self.mining_progress);

                        {
                            let mut blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                            println!("Транзакция для добавления: {:?}", transaction);
                            if !blockchain.add_transaction(transaction.clone()) {
                                self.status = "Недостаточно средств или неверный адрес".to_string();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                println!(
                                    "Ошибка: недостаточно средств или неверный адрес, проверка заняла {} секунд",
                                    duration
                                );
                                ctx.request_repaint();
                                return;
                            }
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            println!("Транзакция успешно добавлена за {} секунд", duration);
                        }

                        self.status = "Запуск майнинга...".to_string();
                        println!("Подготовка к отправке задачи майнинга в потоке {:?}", thread::current().id());
                        let (progress_tx, progress_rx) = mpsc::channel();
                        let (status_tx, status_rx) = mpsc::channel();
                        {
                            let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                            *mining_progress = None;
                            self.progress_rx = Some(progress_rx);
                            self.status_rx = Some(status_rx);
                            println!("Каналы прогресса и статуса созданы");
                        }
                        if let Ok(mut mining_status) = self.mining_status.lock() {
                            *mining_status = MiningStatus::Mining;
                            println!("Статус майнинга установлен: Mining");
                        } else {
                            self.status = "Ошибка: Не удалось установить статус майнинга".to_string();
                            println!("Ошибка: Не удалось установить статус майнинга");
                            self.progress_rx = None;
                            self.status_rx = None;
                            ctx.request_repaint();
                            return;
                        }
                        println!("Попытка отправки задачи майнинга");
                        if let Err(e) = self.mining_tx.send(MiningTask {
                            blockchain,
                            transaction,
                            mining_status,
                            progress_tx,
                            status_tx,
                        }) {
                            self.status = format!("Ошибка отправки задачи майнинга: {}", e);
                            println!("Ошибка отправки задачи майнинга: {}", e);
                            if let Ok(mut mining_status) = self.mining_status.lock() {
                                *mining_status = MiningStatus::Idle;
                                println!("Статус майнинга сброшен на Idle");
                            }
                            self.progress_rx = None;
                            self.status_rx = None;
                            ctx.request_repaint();
                            return;
                        }
                        println!("Задача майнинга успешно отправлена");
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Запуск майнинга завершён за {} секунд", duration);
                        ctx.request_repaint();
                    } else {
                        self.status = "Неверный формат суммы".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Ошибка: неверный формат суммы, проверка заняла {} секунд", duration);
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
                    println!("Кнопка 'Найти кошелёк' нажата, IP: {}, Порт: {}", self.ip, self.port);
                    let ip = self.ip.trim();
                    if ip.is_empty() {
                        self.status = "IP-адрес не может быть пустым".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Ошибка: пустой IP-адрес, проверка заняла {} секунд", duration);
                    } else if let Ok(port_num) = self.port.trim().parse::<u16>() {
                        let address = format!("{}:{}", ip, port_num);
                        if let Some(wallet) = self.node.find_wallet_by_ip(ip, port_num) {
                            if self.node.add_peer(address.clone()) {
                                self.status = format!("Найден кошелёк: {} для {}:{} и добавлен в network.json", wallet, ip, port_num);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                println!("Кошелёк найден: {} для {}:{} и добавлен в network.json, поиск занял {} секунд", wallet, ip, port_num, duration);
                            } else {
                                self.status = format!("Найден кошелёк: {} для {}:{}, уже существует в network.json", wallet, ip, port_num);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                println!("Кошелёк найден: {} для {}:{}, уже существует в network.json, поиск занял {} секунд", wallet, ip, port_num, duration);
                            }
                        } else {
                            self.status = format!("Кошелёк не найден для {}:{}", ip, port_num);
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            println!("Кошелёк не найден для {}:{}, поиск занял {} секунд", ip, port_num, duration);
                        }
                    } else {
                        self.status = "Неверный формат порта".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Ошибка: неверный формат порта {}, проверка заняла {} секунд", self.port, duration);
                    }
                    ctx.request_repaint();
                }
            }
        });
    }
}

fn main() {
    let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
    let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
    let config_path = exe_dir.join("config.json");

    let config_content = fs::read_to_string(&config_path).unwrap_or_else(|err| {
        eprintln!("Ошибка чтения {}: {}. Используются значения по умолчанию.", config_path.display(), err);
        r#"{"wallet": {"name": "wallet1", "password": "password", "port": 8081, "ip": "127.0.0.1"}}"#.to_string()
    });
    let config: Config = serde_json::from_str(&config_content).expect("Ошибка парсинга конфигурации");

    let network_path = exe_dir.join("network.json");
    let network_config: NetworkConfig = match fs::read_to_string(&network_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|err| {
            eprintln!("Ошибка парсинга network.json: {}. Используются значения по умолчанию.", err);
            NetworkConfig {
                peers: vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()],
            }
        }),
        Err(err) => {
            eprintln!("Ошибка чтения network.json: {}. Используются значения по умолчанию.", err);
            NetworkConfig {
                peers: vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()],
            }
        }
    };
    // Записываем network_config обратно в файл только если он был создан с значениями по умолчанию
    if network_config.peers == vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()] {
        let network_content = serde_json::to_string_pretty(&network_config).expect("Ошибка сериализации network.json");
        fs::write(&network_path, network_content).expect("Ошибка записи в network.json");
    }

    let (mining_tx, mining_rx) = mpsc::channel();
    let (sync_tx, sync_rx) = mpsc::channel();
    println!("Каналы майнинга и синхронизации созданы");

    let mut node = Node::new(format!("{}:{}", config.wallet.ip, config.wallet.port), mining_rx, sync_tx.clone(), config.wallet.port);
    node.start_server(config.wallet.port, sync_tx.clone());
    node.discover_peers();

    let app = WalletApp {
        node,
        wallet_address: config.wallet.name,
        password: config.wallet.password,
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
    };

    eframe::run_native(
        "Blockchain Wallet",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Box::new(app)),
    )
        .expect("Ошибка запуска приложения");
}
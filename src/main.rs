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
    db: Arc<Mutex<DB>>, // Используем Arc<Mutex<DB>>
}

impl<'de> Deserialize<'de> for Blockchain {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Десериализуем промежуточную структуру
        let BlockchainDeserialize {
            chain,
            balances,
            difficulty,
            pending_transactions,
        } = BlockchainDeserialize::deserialize(deserializer)?;

        // Инициализируем LevelDB
        let exe_path = std::env::current_exe().map_err(serde::de::Error::custom)?;
        let exe_dir = exe_path
            .parent()
            .ok_or_else(|| serde::de::Error::custom("Не удалось получить директорию исполняемого файла"))?;
        let db_path = exe_dir.join("blockchain_db");
        let db = DB::open(db_path, Options::default()).map_err(serde::de::Error::custom)?;
        let db = Arc::new(Mutex::new(db));

        // Загружаем сохранённые транзакции из LevelDB
        let mut stored_transactions = vec![];
        let mut db_locked = db.lock().map_err(serde::de::Error::custom)?;
        let mut iterator = db_locked
            .new_iter()
            .map_err(serde::de::Error::custom)?;
        while let Some((key, value)) = iterator.next() {
            if let Ok(transaction) = serde_json::from_slice::<Transaction>(&value) {
                stored_transactions.push(transaction);
            } else {
                println!("Ошибка десериализации транзакции для ключа {:?}", key);
            }
        }
        drop(db_locked);

        // Объединяем десериализованные и сохранённые транзакции
        let mut final_pending_transactions = pending_transactions;
        for tx in stored_transactions {
            if !final_pending_transactions.contains(&tx) {
                final_pending_transactions.push(tx);
            }
        }

        Ok(Blockchain {
            chain,
            balances,
            difficulty,
            pending_transactions: final_pending_transactions,
            db,
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
    fn new() -> Self {
        let start_time = SystemTime::now();
        // Инициализация LevelDB
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let db_path = exe_dir.join("blockchain_db");
        let db = DB::open(db_path, Options::default()).expect("Не удалось открыть LevelDB");
        let db = Arc::new(Mutex::new(db));

        let mut blockchain = Blockchain {
            chain: vec![],
            balances: HashMap::new(),
            difficulty: 1,
            pending_transactions: vec![],
            db,
        };

        // Загружаем сохранённые транзакции из LevelDB
        let mut pending_transactions = vec![];
        let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
        let mut iterator = db.new_iter().expect("Не удалось создать итератор LevelDB");
        while let Some((key, value)) = iterator.next() {
            if let Ok(transaction) = serde_json::from_slice::<Transaction>(&value) {
                pending_transactions.push(transaction);
            } else {
                println!("Ошибка десериализации транзакции для ключа {:?}", key);
            }
        }
        drop(db);
        blockchain.pending_transactions = pending_transactions;

        blockchain.create_genesis_block();
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Создание блокчейна завершено за {} секунд", duration);
        blockchain
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
        println!("Вычисление хэша завершено за {} секунд", hash);
        hash
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

        let block = self.mine_block_inner(previous_block, transactions, difficulty, progress_tx.clone());

        if let Some(mut block) = block {
            let balance_start_time = SystemTime::now();
            for tx in &block.transactions {
                *self.balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
                *self.balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
            }
            let balance_duration = SystemTime::now()
                .duration_since(balance_start_time)
                .unwrap()
                .as_secs_f64();
            println!("Обновление балансов завершено за {} секунд", balance_duration);

            // Очищаем pending_transactions и удаляем их из LevelDB
            let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
            for tx in &self.pending_transactions {
                let key = tx.id.as_bytes();
                if let Err(e) = db.delete(key) {
                    println!("Ошибка удаления транзакции {} из LevelDB: {}", tx.id, e);
                }
            }
            drop(db);
            self.pending_transactions.clear();
            self.chain.push(block.clone());
            let total_duration = SystemTime::now()
                .duration_since(total_start_time)
                .unwrap()
                .as_secs_f64();
            println!("Майнинг завершен за {} секунд", total_duration);
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

        let max_iterations = 50;
        let timeout = Duration::from_secs_f32(0.5);
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
            if iteration_count % 10 == 0 {
                let progress_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                let avg_hash_time = if iteration_count > 0 { total_hash_time / iteration_count as f64 } else { 0.0 };
                println!(
                    "Прогресс майнинга: {} итераций выполнено за {} секунд, среднее время хэширования: {} секунд",
                    iteration_count, progress_duration, avg_hash_time
                );
                let _ = progress_tx.send(format!("Прогресс майнинга: {} итераций за {} секунд", iteration_count, progress_duration));
            }
        }
    }

    fn add_transaction(&mut self, transaction: Transaction) -> bool {
        let start_time = SystemTime::now();
        println!("Начало добавления транзакции: {:?}", transaction);
        if transaction.sender.is_empty() || transaction.receiver.is_empty() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Ошибка: Пустой адрес отправителя или получателя, проверка заняла {} секунд", duration);
            return false;
        }
        if self.pending_transactions.contains(&transaction) {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Транзакция уже существует в pending_transactions, проверка заняла {} секунд", duration);
            return false;
        }
        if let Some(sender_balance) = self.balances.get(&transaction.sender) {
            println!("Баланс отправителя {}: {}", transaction.sender, sender_balance);
            if *sender_balance >= transaction.amount {
                // Сохраняем транзакцию в LevelDB
                let key = transaction.id.as_bytes();
                let value = serde_json::to_vec(&transaction).expect("Ошибка сериализации транзакции");
                let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                if let Err(e) = db.put(key, &value) {
                    println!("Ошибка сохранения транзакции {} в LevelDB: {}", transaction.id, e);
                    return false;
                }
                drop(db);
                self.pending_transactions.push(transaction);
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!("Транзакция добавлена и сохранена в LevelDB за {} секунд: {:?}", duration, self.pending_transactions);
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
            "Ошибка: Адрес отправителя {} не найден, проверка заняла {} секунд",
            transaction.sender, duration
        );
        false
    }
}

impl Node {
    fn new(address: String, mining_rx: mpsc::Receiver<MiningTask>, sync_tx: mpsc::Sender<Blockchain>) -> Self {
        let blockchain = Arc::new(Mutex::new(Blockchain::new()));
        let peers = Arc::new(Mutex::new(vec![]));
        let node = Node {
            blockchain: blockchain.clone(),
            peers: peers.clone(),
            address: address.clone(),
            sync_rx: mpsc::channel().1,
        };
        thread::spawn(move || {
            println!("Фоновый поток майнинга запущен в потоке {:?}", thread::current().id());
            let mut mining_count = 0;
            let mut total_duration = 0.0;
            let mut successful_mining = 0;
            while let Ok(task) = mining_rx.recv() {
                mining_count += 1;
                println!("Получена задача майнинга {} в потоке {:?}", mining_count, thread::current().id());
                let progress_tx_clone = task.progress_tx.clone();
                let status_tx_clone = task.status_tx.clone();
                let start_time = SystemTime::now();
                let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    println!("Попытка захвата Mutex для blockchain в задаче {}", mining_count);
                    let lock_start_time = SystemTime::now();
                    let mut blockchain = task.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                    let lock_duration = SystemTime::now().duration_since(lock_start_time).unwrap().as_secs_f64();
                    println!("Захват Mutex для blockchain в задаче {} занял {} секунд", mining_count, lock_duration);
                    println!("Проверка транзакции в задаче {}: {:?}", mining_count, task.transaction);
                    if blockchain.add_transaction(task.transaction.clone()) {
                        println!("Транзакция в задаче {} успешно добавлена, начало майнинга", mining_count);
                        blockchain.mine_block(progress_tx_clone)
                    } else {
                        println!("Транзакция в задаче {} отклонена, попытка майнить существующие транзакции", mining_count);
                        if !blockchain.pending_transactions.is_empty() {
                            blockchain.mine_block(progress_tx_clone)
                        } else {
                            let _ = task.progress_tx.send(format!("Ошибка: Нет транзакций для майнинга в задаче {}", mining_count));
                            println!("Ошибка: Нет транзакций для майнинга в задаче {}", mining_count);
                            None
                        }
                    }
                })) {
                    Ok(result) => result,
                    Err(panic) => {
                        let err_msg = match panic.downcast_ref::<&str>() {
                            Some(s) => s.to_string(),
                            None => format!("Неизвестная паника: {:?}", panic),
                        };
                        println!("Паника в потоке майнинга {}: {}", mining_count, err_msg);
                        let _ = task.progress_tx.send(format!("Паника в потоке майнинга {}: {}", mining_count, err_msg));
                        None
                    }
                };
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                total_duration += duration;
                println!("Майнинг {} завершен за {} секунд с результатом: {:?}", mining_count, duration, result);
                let mut attempts = 0;
                let max_attempts = 5;
                let mut status_updated = false;
                while attempts < max_attempts {
                    if let Ok(mut mining_status) = task.mining_status.try_lock() {
                        *mining_status = match result {
                            Some(block) => {
                                successful_mining += 1;
                                println!("Майнинг {} успешен, блок добавлен", mining_count);
                                let _ = status_tx_clone.send(format!("Транзакция отправлена, блок добавлен: {:?}", block));
                                // Рассылаем обновлённый блокчейн другим узлам
                                let mut node_temp = Node {
                                    blockchain: task.blockchain.clone(),
                                    peers: peers.clone(),
                                    address: address.clone(),
                                    sync_rx: mpsc::channel().1,
                                };
                                node_temp.sync_blockchain(sync_tx.clone());
                                MiningStatus::Completed(Some(block))
                            }
                            None => {
                                println!("Майнинг {} не удался: нет транзакций или превышен лимит итераций/таймаут", mining_count);
                                let _ = status_tx_clone.send("Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут".to_string());
                                MiningStatus::Failed("Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут".to_string())
                            }
                        };
                        status_updated = true;
                        println!("Статус майнинга {} обновлён: {:?}", mining_count, *mining_status);
                        break;
                    } else {
                        attempts += 1;
                        println!("Попытка {} обновить статус майнинга {} не удалась", attempts, mining_count);
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
                if !status_updated {
                    println!("Не удалось обновить статус майнинга {} после {} попыток", mining_count, max_attempts);
                    let _ = task.progress_tx.send(format!("Ошибка: Не удалось обновить статус майнинга {} после {} попыток", mining_count, max_attempts));
                    let _ = status_tx_clone.send(format!("Ошибка: Не удалось обновить статус майнинга после {} попыток", max_attempts));
                }
                if mining_count > 0 {
                    let avg_duration = total_duration / mining_count as f64;
                    println!("Среднее время майнинга после {} задач: {} секунд", mining_count, avg_duration);
                    println!("Успешных майнингов: {}, Неуспешных: {}", successful_mining, mining_count - successful_mining);
                }
            }
            println!("Фоновый поток майнинга завершен");
        });
        node
    }

    fn discover_peers(&mut self) {
        let start_time = SystemTime::now();
        let mut peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        peers.clear();
        // Читаем network.json
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let network_path = exe_dir.join("network.json");
        let network_config: NetworkConfig = match fs::read_to_string(&network_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| NetworkConfig { peers: vec![] }),
            Err(_) => NetworkConfig { peers: vec![] },
        };
        // Исключаем собственный адрес из списка пиров
        let own_port = self.address.split(':').last().unwrap_or("0").parse::<u16>().unwrap_or(0);
        for peer in network_config.peers {
            let peer_port = peer.split(':').last().unwrap_or("0").parse::<u16>().unwrap_or(0);
            if peer_port != own_port {
                peers.push(peer);
            }
        }
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
            println!("Пир {} уже существует, добавление не требуется, заняло {} секунд", address, duration);
            return false;
        }
        peers.push(address.clone());
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let network_path = exe_dir.join("network.json");
        let mut network_config = match fs::read_to_string(&network_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| NetworkConfig { peers: vec![] }),
            Err(_) => NetworkConfig { peers: vec![] },
        };
        if !network_config.peers.contains(&address) {
            network_config.peers.push(address.clone());
            let content = serde_json::to_string_pretty(&network_config).expect("Ошибка сериализации network.json");
            fs::write(&network_path, content).expect("Ошибка записи в network.json");
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Пир {} добавлен за {} секунд", address, duration);
        true
    }

    fn find_wallet_by_ip(&self, ip: &str, port: u16) -> Option<String> {
        let start_time = SystemTime::now();
        let ip = ip.trim();
        if ip.is_empty() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Ошибка: Пустой IP-адрес, проверка заняла {} секунд", duration);
            return None;
        }
        let address = format!("{}:{}", ip, port);
        if address.parse::<std::net::SocketAddr>().is_err() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Некорректный формат адреса {}:{}", ip, port);
            return None;
        }
        let peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        println!("Список пиров: {:?}", *peers);
        if peers.contains(&address) {
            let wallet = match address.as_str() {
                "127.0.0.1:8081" => "wallet1".to_string(),
                "127.0.0.1:8082" => "wallet2".to_string(),
                _ => format!("wallet{}", rand::thread_rng().gen_range(1..3)),
            };
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Найден кошелёк: {} для адреса {}:{}, поиск занял {} секунд", wallet, ip, port, duration);
            Some(wallet)
        } else {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Адрес {}:{} не найден в списке пиров, поиск занял {} секунд", ip, port, duration);
            None
        }
    }

    fn start_server(&self, port: u16, sync_tx: mpsc::Sender<Blockchain>) {
        let start_time = SystemTime::now();
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap_or_else(|e| {
            panic!("Ошибка привязки к порту {}: {}", port, e);
        });
        let blockchain = Arc::clone(&self.blockchain);

        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buffer = [0; 4096];
                        match stream.read(&mut buffer) {
                            Ok(n) => {
                                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                                println!("Получен запрос: {}", request);
                                if request.contains("GET_BLOCKCHAIN") {
                                    let blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                    let response = serde_json::to_string(&*blockchain).unwrap();
                                    let _ = stream.write_all(response.as_bytes());
                                    println!("Отправлен блокчейн клиенту");
                                } else if request.starts_with("UPDATE_BLOCKCHAIN:") {
                                    let blockchain_data = request.strip_prefix("UPDATE_BLOCKCHAIN:").unwrap_or("");
                                    match serde_json::from_str::<Blockchain>(blockchain_data) {
                                        Ok(received_blockchain) => {
                                            let mut blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                            let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                            let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                            if received_blockchain.chain.len() > blockchain.chain.len() || current_hash != received_hash {
                                                // Клонируем pending_transactions и balances для сохранения в LevelDB и обновления блокчейна
                                                let pending_transactions = received_blockchain.pending_transactions.clone();
                                                let balances = received_blockchain.balances.clone();
                                                let chain = received_blockchain.chain.clone();
                                                let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                                for tx in &pending_transactions {
                                                    let key = tx.id.as_bytes();
                                                    let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                                                    if let Err(e) = db.put(key, &value) {
                                                        println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                                                    }
                                                }
                                                drop(db);
                                                // Сохраняем существующую БД
                                                let existing_db = blockchain.db.clone();
                                                *blockchain = Blockchain {
                                                    chain,
                                                    balances,
                                                    difficulty: received_blockchain.difficulty,
                                                    pending_transactions,
                                                    db: existing_db,
                                                };
                                                println!("Блокчейн обновлён через UPDATE_BLOCKCHAIN");
                                                let _ = sync_tx.send(received_blockchain);
                                            } else {
                                                println!("Полученный блокчейн не новее текущего");
                                            }
                                        }
                                        Err(e) => println!("Ошибка десериализации блокчейна: {}", e),
                                    }
                                }
                            }
                            Err(e) => println!("Ошибка чтения запроса: {}", e),
                        }
                    }
                    Err(e) => println!("Ошибка обработки входящего соединения: {}", e),
                }
            }
        });
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Сервер запущен на порту {} за {} секунд", port, duration);
    }

    fn sync_blockchain(&mut self, sync_tx: mpsc::Sender<Blockchain>) {
        let start_time = SystemTime::now();
        self.discover_peers();
        let peers: Vec<String> = self.peers.lock().expect("Не удалось захватить Mutex для peers")
            .iter()
            .cloned()
            .collect();
        println!("Список пиров для синхронизации: {:?}", peers);

        let blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
        let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
        let current_chain_length = blockchain.chain.len();
        let existing_db = blockchain.db.clone();
        drop(blockchain);

        for peer in peers.iter() {
            let addr: SocketAddr = match peer.parse() {
                Ok(addr) => addr,
                Err(e) => {
                    println!("Некорректный адрес пира {}: {}", peer, e);
                    continue;
                }
            };
            match TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                Ok(mut stream) => {
                    let blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                    let response = serde_json::to_string(&*blockchain).unwrap();
                    match stream.write_all(format!("UPDATE_BLOCKCHAIN:{}", response).as_bytes()) {
                        Ok(_) => println!("Блокчейн отправлен узлу {}", peer),
                        Err(e) => println!("Ошибка отправки блокчейна узлу {}: {}", peer, e),
                    }
                }
                Err(e) => println!("Узел {} недоступен: {}", peer, e),
            }
            match TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                Ok(mut stream) => {
                    match stream.write_all(b"GET_BLOCKCHAIN") {
                        Ok(_) => {
                            let mut buffer = [0; 4096];
                            match stream.read(&mut buffer) {
                                Ok(n) => {
                                    let response = String::from_utf8_lossy(&buffer[..n]).to_string();
                                    println!("Получен ответ от узла {}: {}", peer, response);
                                    match serde_json::from_str::<Blockchain>(&response) {
                                        Ok(received_blockchain) => {
                                            let mut blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                            let received_hash = received_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                            if received_blockchain.chain.len() > blockchain.chain.len() || current_hash != received_hash {
                                                // Клонируем поля для сохранения в LevelDB и обновления блокчейна
                                                let pending_transactions = received_blockchain.pending_transactions.clone();
                                                let chain = received_blockchain.chain.clone();
                                                let balances = received_blockchain.balances.clone();
                                                let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                                for tx in &pending_transactions {
                                                    let key = tx.id.as_bytes();
                                                    let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                                                    if let Err(e) = db.put(key, &value) {
                                                        println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                                                    }
                                                }
                                                drop(db);
                                                // Сохраняем существующую БД
                                                *blockchain = Blockchain {
                                                    chain,
                                                    balances,
                                                    difficulty: received_blockchain.difficulty,
                                                    pending_transactions,
                                                    db: existing_db.clone(),
                                                };
                                                println!("Блокчейн обновлён с узла {}", peer);
                                                let _ = sync_tx.send(received_blockchain);
                                            } else {
                                                println!("Полученный блокчейн с узла {} не новее текущего", peer);
                                            }
                                        }
                                        Err(e) => println!("Ошибка десериализации блокчейна от узла {}: {}", peer, e),
                                    }
                                }
                                Err(e) => println!("Ошибка чтения ответа от узла {}: {}", peer, e),
                            }
                        }
                        Err(e) => println!("Ошибка отправки GET_BLOCKCHAIN узлу {}: {}", peer, e),
                    }
                }
                Err(e) => println!("Узел {} недоступен: {}", peer, e),
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
            if received_blockchain.chain.len() > blockchain.chain.len() || current_hash != received_hash {
                let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                for tx in &received_blockchain.pending_transactions {
                    let key = tx.id.as_bytes();
                    let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                    if let Err(e) = db.put(key, &value) {
                        println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                    }
                }
                drop(db);
                // Сохраняем существующую БД
                let existing_db = blockchain.db.clone();
                *blockchain = Blockchain {
                    chain: received_blockchain.chain,
                    balances: received_blockchain.balances,
                    difficulty: received_blockchain.difficulty,
                    pending_transactions: received_blockchain.pending_transactions,
                    db: existing_db,
                };
                println!("UI: Блокчейн обновлён через канал синхронизации");
                ctx.request_repaint();
            }
        }

        if let Some(ref status_rx) = self.status_rx {
            while let Ok(status) = status_rx.try_recv() {
                self.status = status;
                if self.status.starts_with("Транзакция отправлена") {
                    if let Ok(mut mining_status) = self.mining_status.lock() {
                        *mining_status = MiningStatus::Idle;
                        println!("Статус майнинга сброшен на Idle");
                    }
                    self.progress_rx = None;
                }
                println!("Получено обновление статуса: {}", self.status);
                ctx.request_repaint();
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(&self.status);
            if !self.is_authenticated {
                ui.heading("Аутентификация");
                ui.text_edit_singleline(&mut self.wallet_address);
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
        r#"{"wallet": {"name": "wallet1", "password": "password", "port": 8081}}"#.to_string()
    });
    let config: Config = serde_json::from_str(&config_content).expect("Ошибка парсинга конфигурации");

    let network_path = exe_dir.join("network.json");
    let network_config = NetworkConfig {
        peers: vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string()],
    };
    let network_content = serde_json::to_string_pretty(&network_config).expect("Ошибка сериализации network.json");
    fs::write(&network_path, network_content).expect("Ошибка записи в network.json");

    let (mining_tx, mining_rx) = mpsc::channel();
    let (sync_tx, sync_rx) = mpsc::channel();
    println!("Каналы майнинга и синхронизации созданы");

    let mut node = Node::new(format!("127.0.0.1:{}", config.wallet.port), mining_rx, sync_tx.clone());
    node.start_server(config.wallet.port, sync_tx.clone());
    node.discover_peers();

    let mut node_clone = Node {
        blockchain: Arc::clone(&node.blockchain),
        peers: Arc::clone(&node.peers),
        address: node.address.clone(),
        sync_rx: mpsc::channel().1,
    };
    thread::spawn(move || {
        loop {
            node_clone.sync_blockchain(sync_tx.clone());
            std::thread::sleep(Duration::from_secs(1));
        }
    });

    let app = WalletApp {
        node: Node {
            blockchain: node.blockchain,
            peers: node.peers,
            address: node.address,
            sync_rx,
        },
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
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
mod wallet;

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
#[derive(Deserialize, Serialize)]
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

        let port = std::env::var("PORT").unwrap_or("8081".to_string()).parse::<u16>().unwrap_or(8081);
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let db_path = exe_dir.join(format!("blockchain_db_{}", port));

        Ok(Blockchain {
            chain,
            balances,
            difficulty,
            pending_transactions,
            db: Arc::new(Mutex::new(DB::open(
                db_path,
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
    new_wallet_password: String,
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
        let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
        let mut iterator = db.new_iter().expect("Не удалось создать итератор LevelDB");
        println!("Содержимое базы данных:");
        while let Some((key, value)) = iterator.next() {
            let key_str = std::str::from_utf8(&key).unwrap_or("невалидный ключ");
            println!("Ключ: {}", key_str);
            if key == b"chain" {
                println!("Значение (chain): {:?}", serde_json::from_slice::<Vec<Block>>(&value));
            } else if key == b"balances" {
                println!("Значение (balances): {:?}", serde_json::from_slice::<HashMap<String, u64>>(&value));
            } else if key == b"difficulty" {
                println!("Значение (difficulty): {:?}", serde_json::from_slice::<u32>(&value));
            } else {
                println!("Значение (transaction): {:?}", serde_json::from_slice::<Transaction>(&value));
            }
        }
    }

    fn new(port: u16) -> Self {
        let start_time = SystemTime::now();
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let db_path = exe_dir.join(format!("blockchain_db_{}", port));
        println!("Проверка базы данных по пути: {}", db_path.display());
        if !db_path.exists() {
            println!("База данных не существует, создаётся новая");
        }
        let lock_file = db_path.join("LOCK");
        if lock_file.exists() {
            println!("Файл LOCK существует, ожидание освобождения базы данных");
            let max_attempts = 5;
            let mut attempts = 0;
            while lock_file.exists() && attempts < max_attempts {
                std::thread::sleep(Duration::from_millis(1000));
                attempts += 1;
            }
            if lock_file.exists() {
                println!("Файл LOCK всё ещё существует, попытка удаления");
                if let Err(e) = fs::remove_file(&lock_file) {
                    panic!("Не удалось удалить файл LOCK: {}", e);
                }
            }
        }

        let db = DB::open(db_path, Options::default()).expect("Не удалось открыть LevelDB");
        let db = Arc::new(Mutex::new(db));

        let mut blockchain = Blockchain {
            chain: vec![],
            balances: HashMap::new(),
            difficulty: 1,
            pending_transactions: vec![],
            db: db.clone(),
        };
        blockchain.debug_db();

        let mut chain_opt: Option<Vec<Block>> = None;
        let mut balances_opt: Option<HashMap<String, u64>> = None;
        let mut difficulty_opt: Option<u32> = None;
        let mut pending = vec![];

        {
            let mut db_guard = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");

            chain_opt = db_guard.get(b"chain").and_then(|v| serde_json::from_slice::<Vec<Block>>(&v).ok());
            println!("chain_opt: {:?}", chain_opt);

            balances_opt = db_guard.get(b"balances").and_then(|v| serde_json::from_slice::<HashMap<String, u64>>(&v).ok());
            println!("balances_opt: {:?}", balances_opt);

            difficulty_opt = db_guard.get(b"difficulty").and_then(|v| serde_json::from_slice::<u32>(&v).ok());
            println!("difficulty_opt: {:?}", difficulty_opt);

            let mut iterator = db_guard.new_iter().expect("Не удалось создать итератор LevelDB");
            println!("Загрузка транзакций из LevelDB...");
            while let Some((key, value)) = iterator.next() {
                let key_str = std::str::from_utf8(&key).unwrap_or("невалидный ключ");
                println!("Найден ключ: {}", key_str);
                if key == b"chain" || key == b"balances" || key == b"difficulty" {
                    println!("Пропуск системного ключа: {:?}", key);
                    continue;
                }
                if Uuid::parse_str(key_str).is_ok() {
                    match serde_json::from_slice::<Transaction>(&value) {
                        Ok(transaction) => {
                            println!("Успешно загружена транзакция: {:?}", transaction);
                            if !pending.iter().any(|t: &Transaction| t.id == transaction.id) {
                                pending.push(transaction);
                            }
                        }
                        Err(e) => println!("Ошибка десериализации транзакции для ключа {}: {}", key_str, e),
                    }
                } else {
                    println!("Невалидный UUID ключ: {}", key_str);
                }
            }
            println!("Загружено {} транзакций", pending.len());
        }

        if let Some(chain) = chain_opt {
            blockchain.chain = chain;
        } else {
            blockchain.create_genesis_block();
        }

        if let Some(balances) = balances_opt {
            blockchain.balances = balances;
        } else {
            blockchain.balances = HashMap::new();
            println!("Балансы не найдены в LevelDB, инициализация на основе цепочки блоков");
            // Инициализируем балансы на основе транзакций
            for block in &blockchain.chain {
                for tx in &block.transactions {
                    if tx.sender != "genesis" {
                        *blockchain.balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
                    }
                    *blockchain.balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
                }
            }
            // Сохраняем инициализированные балансы в LevelDB
            let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
            if let Err(e) = db.put(b"balances", &serde_json::to_vec(&blockchain.balances).unwrap()) {
                println!("Ошибка сохранения балансов в LevelDB: {}", e);
            }
            db.flush().expect("Ошибка при фиксации данных в LevelDB");
        }

        if let Some(difficulty) = difficulty_opt {
            blockchain.difficulty = difficulty;
        }

        blockchain.pending_transactions = pending;
        blockchain.save_state();
        blockchain.debug_db();
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Создание блокчейна завершено за {} секунд", duration);
        blockchain
    }

    fn create_genesis_block(&mut self) {
        let start_time = SystemTime::now();
        let genesis_transaction = Transaction {
            id: Uuid::new_v4().to_string(),
            sender: "genesis".to_string(),
            receiver: "initial_wallet_address".to_string(),
            amount: 10000,
        };
        let genesis_block = Block {
            index: 0,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: vec![genesis_transaction],
            previous_hash: "0".to_string(),
            hash: String::new(),
            nonce: 0,
        };
        let hash = self.calculate_hash(&genesis_block);
        let mut genesis_block = genesis_block;
        genesis_block.hash = hash;
        self.chain.push(genesis_block.clone());
        // Обновляем балансы на основе транзакций генезис-блока, только для получателя
        for tx in &genesis_block.transactions {
            if tx.sender != "genesis" {
                let sender_balance = self.balances.get(&tx.sender).cloned().unwrap_or(0);
                if sender_balance < tx.amount {
                    panic!("Недостаточно средств у {} для транзакции {}", tx.sender, tx.id);
                }
                *self.balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
            }
            *self.balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
        }
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

    fn mine_block(&mut self, progress_tx: mpsc::Sender<String>) -> Option<Block> {
        let total_start_time = SystemTime::now();
        println!("Начало майнинга в потоке {:?}", thread::current().id());
        if self.pending_transactions.is_empty() {
            println!("Нет транзакций для майнинга");
            let _ = progress_tx.send("Нет транзакций для майнинга".to_string());
            return None;
        }

        let previous_block = self.chain.last().unwrap().clone();
        let transactions: Vec<Transaction> = self.pending_transactions
            .iter()
            .filter(|tx| !self.chain.iter().any(|block| block.transactions.iter().any(|t| t.id == tx.id)))
            .cloned()
            .collect();
        if transactions.is_empty() {
            println!("Все pending_transactions уже включены в блоки");
            let _ = progress_tx.send("Все pending_transactions уже включены в блоки".to_string());
            return None;
        }
        let difficulty = self.difficulty;

        let block = self.mine_block_inner(previous_block, transactions, difficulty, progress_tx.clone());

        if let Some(mut block) = block {
            let balance_start_time = SystemTime::now();
            for tx in &block.transactions {
                let sender_balance = self.balances.get(&tx.sender).cloned().unwrap_or(0);
                if sender_balance < tx.amount {
                    println!("Ошибка: Недостаточно средств у {} для транзакции {}", tx.sender, tx.id);
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
            if iteration_count % 100 == 0 {
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
        let key = transaction.id.as_bytes();
        let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
        if db.get(key).is_some() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Транзакция с ID {} уже существует в LevelDB, проверка заняла {} секунд", transaction.id, duration);
            return false;
        }
        for existing_tx in &self.pending_transactions {
            if (existing_tx.sender == transaction.sender || existing_tx.receiver == transaction.sender) &&
                (transaction.sender != transaction.receiver) {
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Ошибка: Конфликт транзакции {} с существующей транзакцией {} для кошелька {}, проверка заняла {} секунд",
                    transaction.id, existing_tx.id, transaction.sender, duration
                );
                return false;
            }
        }
        if let Some(sender_balance) = self.balances.get(&transaction.sender) {
            println!("Баланс отправителя {}: {}", transaction.sender, sender_balance);
            if *sender_balance >= transaction.amount {
                self.balances.entry(transaction.receiver.clone()).or_insert(0);
                let value = match serde_json::to_vec(&transaction) {
                    Ok(value) => value,
                    Err(e) => {
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Ошибка сериализации транзакции {}: {}, проверка заняла {} секунд", transaction.id, e, duration);
                        return false;
                    }
                };
                if let Err(e) = db.put(key, &value) {
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    println!("Ошибка сохранения транзакции {} в LevelDB: {}, проверка заняла {} секунд", transaction.id, e, duration);
                    return false;
                }
                drop(db);
                self.pending_transactions.push(transaction);
                self.save_state();
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
                    "Ошибка: Недостаточно средств у {} (баланс: {}, требуется: {}), проверка заняла {} секунд",
                    transaction.sender, sender_balance, transaction.amount, duration
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

    fn validate_chain(&self) -> bool {
        let start_time = SystemTime::now();
        println!("Начало валидации цепочки блоков");
        if self.chain.is_empty() {
            println!("Цепочка пуста, невалидна");
            return false;
        }

        let genesis_block = &self.chain[0];
        println!("Проверка генезис-блока: index={}, previous_hash={}", genesis_block.index, genesis_block.previous_hash);
        if genesis_block.index != 0 || genesis_block.previous_hash != "0" {
            println!("Некорректный генезис-блок: {:?}", genesis_block);
            return false;
        }
        let genesis_hash = self.calculate_hash(genesis_block);
        println!("Вычисленный хэш генезис-блока: {}, сохранённый хэш: {}", genesis_hash, genesis_block.hash);
        if genesis_block.hash != genesis_hash {
            println!("Некорректный хэш генезис-блока: {:?}", genesis_block);
            return false;
        }

        // Инициализируем expected_balances с текущими балансами из self.balances
        let mut expected_balances: HashMap<String, u64> = self.balances.clone();
        println!("Инициализированы expected_balances с текущими балансами: {:?}", expected_balances);

        // Применяем все транзакции из цепочки блоков
        for block in &self.chain {
            println!("Обработка блока {} с {} транзакциями", block.index, block.transactions.len());
            for tx in &block.transactions {
                println!("Обработка транзакции {}: sender={}, receiver={}, amount={}", tx.id, tx.sender, tx.receiver, tx.amount);
                // Пропускаем проверку баланса для отправителя "genesis"
                if tx.sender != "genesis" {
                    let sender_balance = expected_balances.get(&tx.sender).unwrap_or(&0);
                    println!("Текущий баланс отправителя {}: {}", tx.sender, sender_balance);
                    if *sender_balance < tx.amount {
                        println!("Недостаточно средств у {} в блоке {} для транзакции {}: требуется {}, доступно {}", tx.sender, block.index, tx.id, tx.amount, sender_balance);
                        return false;
                    }
                    *expected_balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
                    println!("Баланс отправителя {} уменьшен на {}: новый баланс {}", tx.sender, tx.amount, expected_balances.get(&tx.sender).unwrap_or(&0));
                } else {
                    println!("Отправитель 'genesis', пропуск проверки баланса");
                }
                *expected_balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
                println!("Баланс получателя {} увеличен на {}: новый баланс {}", tx.receiver, tx.amount, expected_balances.get(&tx.receiver).unwrap_or(&0));
                println!("Обновлённые expected_balances после транзакции {}: {:?}", tx.id, expected_balances);
            }
        }

        // Проверяем неподтверждённые транзакции
        let mut temp_balances = expected_balances.clone();
        println!("Проверка неподтверждённых транзакций, начальные temp_balances: {:?}", temp_balances);
        for tx in &self.pending_transactions {
            println!("Обработка неподтверждённой транзакции {}: sender={}, receiver={}, amount={}", tx.id, tx.sender, tx.receiver, tx.amount);
            let sender_balance = temp_balances.get(&tx.sender).unwrap_or(&0);
            println!("Текущий баланс отправителя {} в temp_balances: {}", tx.sender, sender_balance);
            if *sender_balance < tx.amount {
                println!("Недостаточно средств у {} в pending_transactions для транзакции {}: требуется {}, доступно {}", tx.sender, tx.id, tx.amount, sender_balance);
                return false;
            }
            *temp_balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
            *temp_balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
            println!("Баланс отправителя {} уменьшен на {}: новый баланс {}", tx.sender, tx.amount, temp_balances.get(&tx.sender).unwrap_or(&0));
            println!("Баланс получателя {} увеличен на {}: новый баланс {}", tx.receiver, tx.amount, temp_balances.get(&tx.receiver).unwrap_or(&0));
            println!("Обновлённые temp_balances после pending транзакции {}: {:?}", tx.id, temp_balances);
        }

        // Проверяем структуру цепочки
        for i in 1..self.chain.len() {
            let current_block = &self.chain[i];
            let previous_block = &self.chain[i - 1];
            println!("Проверка блока {}: index={}, previous_hash={}", i, current_block.index, current_block.previous_hash);
            if current_block.index != previous_block.index + 1 {
                println!("Некорректный индекс блока {}: ожидалось {}, получено {}", i, previous_block.index + 1, current_block.index);
                return false;
            }
            if current_block.previous_hash != previous_block.hash {
                println!("Некорректный previous_hash в блоке {}: ожидалось {}, получено {}", i, previous_block.hash, current_block.previous_hash);
                return false;
            }
            let current_hash = self.calculate_hash(current_block);
            println!("Вычисленный хэш блока {}: {}, сохранённый хэш: {}", i, current_hash, current_block.hash);
            if current_block.hash != current_hash {
                println!("Некорректный хэш в блоке {}: {:?}", i, current_block);
                return false;
            }
        }

        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Валидация цепочки завершена за {} секунд", duration);
        true
    }

    fn save_state(&mut self) {
        let mut db = self.db.lock().expect("Не удалось захватить Mutex для LevelDB");
        for tx in &self.pending_transactions {
            let key = tx.id.as_bytes();
            println!("Сохранение транзакции с ID {} в LevelDB", tx.id);
            match db.get(key) {
                Some(_) => {
                    println!("Транзакция {} уже существует в LevelDB, пропуск", tx.id);
                    continue;
                }
                None => {
                    let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                    db.put(key, &value).expect("Ошибка сохранения транзакции в LevelDB");
                }
            }
        }
        db.put(b"chain", &serde_json::to_vec(&self.chain).unwrap()).expect("Ошибка сохранения цепочки блоков");
        db.put(b"balances", &serde_json::to_vec(&self.balances).unwrap()).expect("Ошибка сохранения балансов");
        db.put(b"difficulty", &serde_json::to_vec(&self.difficulty).unwrap()).expect("Ошибка сохранения сложности");
        db.flush().expect("Ошибка при фиксации данных в LevelDB");
        drop(db);
        self.debug_db();
    }
}

impl Node {
    fn new(address: String, mining_rx: mpsc::Receiver<MiningTask>, sync_tx: mpsc::Sender<Blockchain>, port: u16) -> Self {
        let blockchain = Arc::new(Mutex::new(Blockchain::new(port)));
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
                                println!("Майнинг {} успешен, блок добавлен: {:?}", mining_count, block);
                                let _ = status_tx_clone.send(format!("Транзакция отправлена, блок добавлен: {:?}", block));
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
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
        let network_path = exe_dir.join("network.json");
        let network_config: NetworkConfig = match fs::read_to_string(&network_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| NetworkConfig { peers: vec![] }),
            Err(_) => NetworkConfig { peers: vec![] },
        };
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
        let mut network_config: NetworkConfig = match fs::read_to_string(&network_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|_| NetworkConfig { peers: vec![] }),
            Err(_) => NetworkConfig { peers: vec![] },
        };
        if !network_config.peers.contains(&address) {
            network_config.peers.push(address.clone());
            let network_content = serde_json::to_string_pretty(&network_config).expect("Ошибка сериализации network.json");
            fs::write(&network_path, network_content).expect("Ошибка записи в network.json");
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Пир {} добавлен за {} секунд", address, duration);
        true
    }

    fn find_wallet_by_ip(&self, ip: &str, port: u16) -> Option<String> {
        let addr = format!("{}:{}", ip, port);
        let peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        if peers.contains(&addr) {
            let blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
            for (wallet, _) in blockchain.balances.iter() {
                return Some(wallet.clone());
            }
        }
        None
    }

    fn start_server(&mut self, port: u16, sync_tx: mpsc::Sender<Blockchain>) {
        let start_time = SystemTime::now();
        let blockchain = Arc::clone(&self.blockchain);
        let address = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&address).expect("Не удалось запустить сервер");
        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let blockchain = Arc::clone(&blockchain);
                        let sync_tx = sync_tx.clone();
                        thread::spawn(move || {
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let mut writer = BufWriter::new(stream);
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
                                let request = String::from_utf8_lossy(&buffer[..total_read]).to_string();
                                println!("Получен запрос: {}", request);

                                if request == "GET_BLOCKCHAIN" {
                                    let blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                    let response = serde_json::to_string(&*blockchain).unwrap();
                                    let length = response.len() as u32;
                                    let mut data = length.to_be_bytes().to_vec();
                                    data.extend_from_slice(response.as_bytes());
                                    if writer.write_all(&data).is_ok() {
                                        writer.flush().ok();
                                        println!("Отправлен блокчейн клиенту");
                                    }
                                } else if request.starts_with("UPDATE_BLOCKCHAIN:") {
                                    let blockchain_data = request.strip_prefix("UPDATE_BLOCKCHAIN:").unwrap_or("");
                                    let mut temp_blockchain: BlockchainDeserialize = match serde_json::from_str(blockchain_data) {
                                        Ok(data) => data,
                                        Err(e) => {
                                            println!("Ошибка десериализации данных блокчейна: {}", e);
                                            return;
                                        }
                                    };
                                    let mut blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                    let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                    let current_pending = blockchain.pending_transactions.clone();
                                    let received_hash = temp_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();

                                    if temp_blockchain.chain.len() > blockchain.chain.len() || current_hash != received_hash {
                                        let mut new_blockchain = Blockchain {
                                            chain: temp_blockchain.chain.clone(),
                                            balances: temp_blockchain.balances.clone(),
                                            difficulty: temp_blockchain.difficulty,
                                            pending_transactions: vec![],
                                            db: blockchain.db.clone(),
                                        };
                                        // Объединяем pending_transactions, добавляя только валидные
                                        let mut merged_pending = vec![];
                                        for tx in temp_blockchain.pending_transactions.iter() {
                                            if !merged_pending.iter().any(|t: &Transaction| t.id == tx.id) && new_blockchain.add_transaction(tx.clone()) {
                                                merged_pending.push(tx.clone());
                                                println!("Добавлена транзакция от узла: {:?}", tx);
                                            }
                                        }
                                        for tx in current_pending.iter() {
                                            if !merged_pending.iter().any(|t| t.id == tx.id) && new_blockchain.add_transaction(tx.clone()) {
                                                merged_pending.push(tx.clone());
                                                println!("Сохранена локальная транзакция: {:?}", tx);
                                            }
                                        }
                                        new_blockchain.pending_transactions = merged_pending;
                                        if new_blockchain.validate_chain() {
                                            let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                            for tx in &new_blockchain.pending_transactions {
                                                let key = tx.id.as_bytes();
                                                let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                                                if let Err(e) = db.put(key, &value) {
                                                    println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                                                }
                                            }
                                            drop(db);
                                            *blockchain = new_blockchain;
                                            blockchain.save_state();
                                            println!("Блокчейн обновлён через UPDATE_BLOCKCHAIN");
                                            let _ = sync_tx.send(Blockchain {
                                                chain: temp_blockchain.chain,
                                                balances: temp_blockchain.balances,
                                                difficulty: temp_blockchain.difficulty,
                                                pending_transactions: blockchain.pending_transactions.clone(),
                                                db: blockchain.db.clone(),
                                            });
                                        } else {
                                            println!("Полученный блокчейн не прошёл валидацию");
                                        }
                                    } else {
                                        // Обновляем только pending_transactions, добавляя только валидные
                                        let mut new_pending = blockchain.pending_transactions.clone();
                                        for tx in temp_blockchain.pending_transactions.iter() {
                                            if !new_pending.iter().any(|t| t.id == tx.id) && blockchain.add_transaction(tx.clone()) {
                                                new_pending.push(tx.clone());
                                                println!("Добавлена транзакция от узла: {:?}", tx);
                                            }
                                        }
                                        for tx in current_pending.iter() {
                                            if !new_pending.iter().any(|t| t.id == tx.id) && blockchain.add_transaction(tx.clone()) {
                                                new_pending.push(tx.clone());
                                                println!("Сохранена локальная транзакция: {:?}", tx);
                                            }
                                        }
                                        blockchain.pending_transactions = new_pending;
                                        blockchain.save_state();
                                        println!("Обновлены pending_transactions через UPDATE_BLOCKCHAIN");
                                    }
                                }
                            }
                        });
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
        // Обнаруживаем пиры перед синхронизацией
        self.discover_peers();
        let peers: Vec<String> = self.peers.lock().expect("Не удалось захватить Mutex для peers")
            .iter()
            .cloned()
            .collect();
        println!("Список пиров для синхронизации: {:?}", peers);

        // Получаем текущее состояние блокчейна
        let blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
        let current_hash = blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
        let current_chain_length = blockchain.chain.len();
        let current_timestamp = blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
        let current_pending = blockchain.pending_transactions.clone();
        let current_block_count = blockchain.chain.iter().filter(|b| !b.transactions.is_empty()).count();
        let current_balances = blockchain.balances.clone();
        let wallet_address = self.address.clone();
        let existing_db = blockchain.db.clone();
        println!("Текущая длина chain: {}, содержимое: {:?}", current_chain_length, blockchain.chain);
        drop(blockchain); // Освобождаем блокировку

        for peer in peers.iter() {
            let addr: SocketAddr = match peer.parse() {
                Ok(addr) => addr,
                Err(e) => {
                    println!("Некорректный адрес пира {}: {}", peer, e);
                    continue;
                }
            };
            if current_chain_length <= 1 {
                println!("Новый узел, только получение данных, отправка цепочки запрещена");
                continue; // Пропускаем отправку UPDATE_BLOCKCHAIN
            }
            // Отправка UPDATE_BLOCKCHAIN
            if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                let mut writer = BufWriter::new(stream.try_clone().unwrap());
                let blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                let response = serde_json::to_string(&*blockchain).unwrap();
                let message = format!("UPDATE_BLOCKCHAIN:{}", response);
                let length = message.len() as u32;
                let mut data = length.to_be_bytes().to_vec();
                data.extend_from_slice(message.as_bytes());
                if writer.write_all(&data).is_ok() {
                    writer.flush().ok();
                    println!("Блокчейн отправлен узлу {}, длина сообщения: {} байт", peer, data.len());
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
                        println!("Получен ответ от узла {}: длина {} байт", peer, response.len());
                        match serde_json::from_str::<BlockchainDeserialize>(&response) {
                            Ok(received_blockchain) => {
                                if received_blockchain.chain.is_empty() || received_blockchain.chain.len() <= 1 {
                                    println!("Получена пустая или минимальная цепочка (длина {}) от узла {}, игнорируем", received_blockchain.chain.len(), peer);
                                    continue;
                                }
                                let temp_blockchain = Blockchain {
                                    chain: received_blockchain.chain.clone(),
                                    balances: received_blockchain.balances.clone(),
                                    difficulty: received_blockchain.difficulty,
                                    pending_transactions: received_blockchain.pending_transactions.clone(),
                                    db: existing_db.clone(),
                                };
                                println!("Полученная цепочка от узла {}: длина {}, содержимое: {:?}", peer, temp_blockchain.chain.len(), temp_blockchain.chain);
                                let received_hash = temp_blockchain.chain.last().map(|b| b.hash.clone()).unwrap_or_default();
                                let received_timestamp = temp_blockchain.chain.last().map(|b| b.timestamp).unwrap_or(0);
                                let received_block_count = temp_blockchain.chain.iter().filter(|b| !b.transactions.is_empty()).count();
                                if temp_blockchain.chain.len() > current_chain_length && received_block_count > current_block_count && temp_blockchain.validate_chain() {
                                    let mut new_blockchain = Blockchain {
                                        chain: temp_blockchain.chain.clone(),
                                        balances: temp_blockchain.balances.clone(),
                                        difficulty: temp_blockchain.difficulty,
                                        pending_transactions: vec![],
                                        db: existing_db.clone(),
                                    };
                                    let mut merged_pending = vec![];
                                    let mut added_transactions = 0;

                                    for tx in temp_blockchain.pending_transactions.iter() {
                                        if !new_blockchain.chain.iter().any(|block| block.transactions.iter().any(|t| t.id == tx.id)) &&
                                            !merged_pending.iter().any(|t: &Transaction| t.id == tx.id) &&
                                            new_blockchain.add_transaction(tx.clone()) {
                                            merged_pending.push(tx.clone());
                                            added_transactions += 1;
                                            println!("Добавлена транзакция от узла {}: {:?}", peer, tx);
                                        }
                                    }

                                    for tx in current_pending.iter() {
                                        if !new_blockchain.chain.iter().any(|block| block.transactions.iter().any(|t| t.id == tx.id)) &&
                                            !merged_pending.iter().any(|t: &Transaction| t.id == tx.id) &&
                                            new_blockchain.add_transaction(tx.clone()) {
                                            merged_pending.push(tx.clone());
                                            added_transactions += 1;
                                            println!("Сохранена локальная транзакция: {:?}", tx);
                                        }
                                    }

                                    new_blockchain.pending_transactions = merged_pending;
                                    println!("Обновлено {} pending_transactions с узла {}", added_transactions, peer);

                                    if new_blockchain.validate_chain() {
                                        let mut blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                        let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                        let chain_data = serde_json::to_vec(&new_blockchain.chain).expect("Ошибка сериализации chain");
                                        println!("Сохраняемый chain: длина {}, размер {} байт", new_blockchain.chain.len(), chain_data.len());
                                        for tx in &new_blockchain.pending_transactions {
                                            let key = tx.id.as_bytes();
                                            let value = serde_json::to_vec(tx).expect("Ошибка сериализации транзакции");
                                            if let Err(e) = db.put(key, &value) {
                                                println!("Ошибка сохранения транзакции {} в LevelDB: {}", tx.id, e);
                                            }
                                        }
                                        if let Err(e) = db.put(b"chain", &chain_data) {
                                            println!("Ошибка сохранения chain в LevelDB: {}", e);
                                        }
                                        if let Err(e) = db.put(b"balances", &serde_json::to_vec(&new_blockchain.balances).unwrap()) {
                                            println!("Ошибка сохранения balances в LevelDB: {}", e);
                                        }
                                        if let Err(e) = db.put(b"difficulty", &serde_json::to_vec(&new_blockchain.difficulty).unwrap()) {
                                            println!("Ошибка сохранения difficulty в LevelDB: {}", e);
                                        }
                                        db.flush().expect("Ошибка при фиксации данных в LevelDB");
                                        drop(db);
                                        *blockchain = new_blockchain;
                                        blockchain.save_state();
                                        println!("Блокчейн обновлён с узла {}, новая длина chain: {}", peer, blockchain.chain.len());
                                        let _ = sync_tx.send(Blockchain {
                                            chain: blockchain.chain.clone(),
                                            balances: blockchain.balances.clone(),
                                            difficulty: blockchain.difficulty,
                                            pending_transactions: blockchain.pending_transactions.clone(),
                                            db: existing_db.clone(),
                                        });
                                    } else {
                                        println!("Полученный блокчейн с узла {} не прошёл валидацию после объединения", peer);
                                    }
                                } else {
                                    let mut blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                    let mut new_pending = blockchain.pending_transactions.clone();
                                    let mut added_transactions = 0;
                                    for tx in temp_blockchain.pending_transactions.iter() {
                                        if !blockchain.chain.iter().any(|block| block.transactions.iter().any(|t| t.id == tx.id)) &&
                                            !new_pending.iter().any(|t: &Transaction| t.id == tx.id) &&
                                            blockchain.add_transaction(tx.clone()) {
                                            new_pending.push(tx.clone());
                                            added_transactions += 1;
                                            println!("Добавлена транзакция от узла {}: {:?}", peer, tx);
                                        }
                                    }
                                    for tx in current_pending.iter() {
                                        if !blockchain.chain.iter().any(|block| block.transactions.iter().any(|t| t.id == tx.id)) &&
                                            !new_pending.iter().any(|t: &Transaction| t.id == tx.id) &&
                                            blockchain.add_transaction(tx.clone()) {
                                            new_pending.push(tx.clone());
                                            added_transactions += 1;
                                            println!("Сохранена локальная транзакция: {:?}", tx);
                                        }
                                    }
                                    blockchain.pending_transactions = new_pending;
                                    println!("Обновлено {} pending_transactions с узла {}", added_transactions, peer);
                                    blockchain.save_state();
                                }
                            }
                            Err(e) => println!("Ошибка десериализации блокчейна от узла {}: {}", peer, e),
                        }
                    } else {
                        println!("Узел {} недоступен", peer);
                    }
                }
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
            if received_blockchain.chain.is_empty() || received_blockchain.chain.len() <= 1 {
                println!("Получена пустая или минимальная цепочка через sync_rx, игнорируем");
                continue;
            }
            let current_balance = blockchain.balances.get(&self.wallet_address).cloned().unwrap_or(0);
            let received_balance = received_blockchain.balances.get(&self.wallet_address).cloned().unwrap_or(0);
            if received_blockchain.chain.len() > blockchain.chain.len() && received_blockchain.validate_chain() {
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
                if current_balance != received_balance {
                    println!("Баланс кошелька {} изменился с {} на {}, запрашивается перерисовка", self.wallet_address, current_balance, received_balance);
                    ctx.request_repaint();
                } else {
                    println!("Баланс кошелька {} не изменился ({}), перерисовка не требуется", self.wallet_address, current_balance);
                }
            } else {
                println!("Полученный блокчейн через канал синхронизации не длиннее или не прошёл валидацию");
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
            ui.label(format!(
                "Количество транзакций в базе данных: {}",
                self.node
                    .blockchain
                    .lock()
                    .expect("Не удалось захватить Mutex для blockchain")
                    .pending_transactions
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
                        println!("Кнопка 'Войти' нажата, адрес: {}, пароль: {}", self.wallet_address, self.password);
                        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
                        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
                        let config_path = exe_dir.join("config.json");
                        match wallet::Wallet::load(&self.password, &config_path) {
                            Ok(wallet) => {
                                if base64::encode(wallet.public_key.to_bytes()) == self.wallet_address {
                                    self.is_authenticated = true;
                                    self.status = "Успешная аутентификация".to_string();
                                    self.node.discover_peers();
                                    let duration = SystemTime::now()
                                        .duration_since(start_time)
                                        .unwrap()
                                        .as_secs_f64();
                                    println!("Аутентификация успешна за {} секунд", duration);
                                    // Баланс не устанавливается при входе
                                } else {
                                    self.status = "Неверный адрес кошелька".to_string();
                                    let duration = SystemTime::now()
                                        .duration_since(start_time)
                                        .unwrap()
                                        .as_secs_f64();
                                    println!("Аутентификация не удалась за {} секунд: неверный адрес кошелька", duration);
                                }
                            }
                            Err(e) => {
                                self.status = format!("Ошибка аутентификации: {}", e);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                println!("Аутентификация не удалась за {} секунд: {}", duration, e);
                            }
                        }
                        ctx.request_repaint();
                    }
                    if ui.button("Регистрация").clicked() {
                        let start_time = SystemTime::now();
                        println!("Кнопка 'Регистрация' нажата, пароль: {}", self.new_wallet_password);
                        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
                        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");
                        let config_path = exe_dir.join("config.json");
                        if self.new_wallet_password.is_empty() {
                            self.status = "Пароль для регистрации не может быть пустым".to_string();
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            println!("Ошибка регистрации за {} секунд: пустой пароль", duration);
                        } else {
                            match wallet::Wallet::new(&self.new_wallet_password, &config_path) {
                                Ok(wallet) => {
                                    self.wallet_address = base64::encode(wallet.public_key.to_bytes());
                                    self.password = self.new_wallet_password.clone();
                                    self.is_authenticated = true;
                                    self.status = format!("Кошелёк успешно создан: {}", self.wallet_address);
                                    self.node.discover_peers();
                                    let duration = SystemTime::now()
                                        .duration_since(start_time)
                                        .unwrap()
                                        .as_secs_f64();
                                    println!("Регистрация успешна за {} секунд, адрес: {}", duration, self.wallet_address);
                                    let mut blockchain = self.node.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                                    if !blockchain.balances.contains_key(&self.wallet_address) {
                                        blockchain.balances.entry(self.wallet_address.clone()).or_insert(10000);
                                        blockchain.save_state(); // Уже есть
                                        // Дополнительно сохраняем балансы в LevelDB явно
                                        let mut db = blockchain.db.lock().expect("Не удалось захватить Mutex для LevelDB");
                                        if let Err(e) = db.put(b"balances", &serde_json::to_vec(&blockchain.balances).unwrap()) {
                                            println!("Ошибка сохранения балансов в LevelDB: {}", e);
                                        }
                                        db.flush().expect("Ошибка при фиксации данных в LevelDB");
                                    }
                                }
                                Err(e) => {
                                    self.status = format!("Ошибка регистрации: {}", e);
                                    let duration = SystemTime::now()
                                        .duration_since(start_time)
                                        .unwrap()
                                        .as_secs_f64();
                                    println!("Ошибка регистрации за {} секунд: {}", duration, e);
                                }
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
        r#"{"wallet": {"name": "", "password": "", "port": 8081, "ip": "127.0.0.1"}}"#.to_string()
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

    let blockchain = Arc::clone(&node.blockchain);

    let app = WalletApp {
        node: Node {
            blockchain: Arc::clone(&node.blockchain),
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
        new_wallet_password: String::new(),
    };

    ctrlc::set_handler(move || {
        println!("Получен сигнал завершения, сохранение состояния...");
        save_on_exit(blockchain.clone());
        println!("Состояние сохранено, выход...");
        std::process::exit(0);
    }).expect("Ошибка установки обработчика завершения");

    eframe::run_native(
        "Blockchain Wallet",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Box::new(app)),
    ).expect("Ошибка запуска приложения");
}

fn save_on_exit(blockchain: Arc<Mutex<Blockchain>>) {
    let mut blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
    blockchain.save_state();
}
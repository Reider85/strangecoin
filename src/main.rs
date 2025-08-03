use serde::{Deserialize, Serialize};
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
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Transaction {
    sender: String,
    receiver: String,
    amount: u64,
}

// Структура блокчейна
#[derive(Clone, Serialize, Deserialize)]
struct Blockchain {
    chain: Vec<Block>,
    balances: HashMap<String, u64>,
    difficulty: u32,
    pending_transactions: Vec<Transaction>,
}

// Структура для команды майнинга
#[derive(Clone)]
struct MiningTask {
    blockchain: Arc<Mutex<Blockchain>>,
    transaction: Transaction,
    mining_status: Arc<Mutex<MiningStatus>>,
    progress_tx: mpsc::Sender<String>,
}

// Структура узла
struct Node {
    blockchain: Arc<Mutex<Blockchain>>,
    peers: Arc<Mutex<Vec<String>>>,
    address: String,
}

// Структура клиента для GUI
struct WalletApp {
    node: Node,
    wallet_address: String,
    password: String,
    is_authenticated: bool,
    receiver_address: String,
    amount: String,
    status: String,
    mining_status: Arc<Mutex<MiningStatus>>,
    mining_progress: Arc<Mutex<Option<String>>>,
    progress_rx: Option<mpsc::Receiver<String>>,
    mining_tx: mpsc::Sender<MiningTask>,
    mining_thread: Option<JoinHandle<()>>,
    last_repaint: f64,
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
        let mut blockchain = Blockchain {
            chain: vec![],
            balances: HashMap::new(),
            difficulty: 1, // Уменьшено для тестов
            pending_transactions: vec![],
        };
        blockchain.create_genesis_block();
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
            "{}{}{}{}",
            block.index,
            block.timestamp,
            serde_json::to_string(&block.transactions).unwrap(),
            block.previous_hash
        );
        let mut hasher = Sha256::new();
        hasher.update(input);
        let hash = format!("{:x}", hasher.finalize());
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Вычисление хэша завершено за {} секунд", duration);
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

        // Клонируем данные для минимизации удержания Mutex
        let previous_block = self.chain.last().unwrap().clone();
        let transactions = self.pending_transactions.clone();
        let difficulty = self.difficulty;

        // Майнинг в отдельной функции без удержания self
        let block = Self::mine_block_inner(previous_block, transactions, difficulty, progress_tx);

        if let Some(mut block) = block {
            // Обновляем блокчейн
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

            self.pending_transactions.clear();
            self.chain.push(block.clone());
            let total_duration = SystemTime::now()
                .duration_since(total_start_time)
                .unwrap()
                .as_secs_f64();
            println!("Майнинг завершен за {} секунд", total_duration);
            Some(block)
        } else {
            None
        }
    }

    fn mine_block_inner(
        previous_block: Block,
        transactions: Vec<Transaction>,
        difficulty: u32,
        progress_tx: mpsc::Sender<String>,
    ) -> Option<Block> {
        let start_time = SystemTime::now();
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

        let max_iterations = 100; // Уменьшено для тестирования
        let timeout = Duration::from_secs(1); // Уменьшено для тестирования
        let mut iteration_count = 0;

        loop {
            if iteration_count >= max_iterations {
                println!("Достигнуто максимальное количество итераций: {}", max_iterations);
                let _ = progress_tx.send(format!("Достигнуто максимальное количество итераций: {}", max_iterations));
                return None;
            }
            if SystemTime::now().duration_since(start_time).unwrap() > timeout {
                println!("Майнинг прерван: превышен таймаут {} секунд", timeout.as_secs());
                let _ = progress_tx.send(format!("Майнинг прерван: превышен таймаут {} секунд", timeout.as_secs()));
                return None;
            }
            iteration_count += 1;
            let hash_start_time = SystemTime::now();
            let input = format!(
                "{}{}{}{}",
                block.index,
                block.timestamp,
                serde_json::to_string(&block.transactions).unwrap(),
                block.previous_hash
            );
            let mut hasher = Sha256::new();
            hasher.update(input);
            let hash = format!("{:x}", hasher.finalize());
            let hash_duration = SystemTime::now()
                .duration_since(hash_start_time)
                .unwrap()
                .as_secs_f64();
            println!(
                "Итерация {}, nonce: {}, хэш: {}, время вычисления хэша: {} секунд",
                iteration_count, block.nonce, hash, hash_duration
            );
            if hash.starts_with(&"0".repeat(difficulty as usize)) {
                block.hash = hash;
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Подходящий хэш найден после {} итераций за {} секунд",
                    iteration_count, duration
                );
                let _ = progress_tx.send(format!("Подходящий хэш найден после {} итераций", iteration_count));
                return Some(block);
            }
            block.nonce += 1;
            if iteration_count % 50 == 0 {
                let progress_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Прогресс майнинга: {} итераций выполнено за {} секунд",
                    iteration_count, progress_duration
                );
                let _ = progress_tx.send(format!("Прогресс майнинга: {} итераций", iteration_count));
            }
        }
    }

    fn add_transaction(&mut self, transaction: Transaction) -> bool {
        let start_time = SystemTime::now();
        if transaction.sender.is_empty() || transaction.receiver.is_empty() {
            println!("Ошибка: Пустой адрес отправителя или получателя");
            return false;
        }
        if let Some(sender_balance) = self.balances.get(&transaction.sender) {
            if *sender_balance >= transaction.amount {
                self.pending_transactions.push(transaction);
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!("Транзакция добавлена за {} секунд: {:?}", duration, self.pending_transactions);
                return true;
            }
        }
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!(
            "Ошибка: Недостаточно средств или неверный адрес, проверка заняла {} секунд",
            duration
        );
        false
    }
}

impl Node {
    fn new(address: String, mining_rx: mpsc::Receiver<MiningTask>) -> Self {
        let blockchain = Arc::new(Mutex::new(Blockchain::new()));
        let node = Node {
            blockchain: blockchain.clone(),
            peers: Arc::new(Mutex::new(vec![])),
            address,
        };
        // Запускаем фоновый поток для обработки задач майнинга
        thread::spawn(move || {
            println!("Фоновый поток майнинга запущен в потоке {:?}", thread::current().id());
            let mut mining_count = 0;
            let mut total_duration = 0.0;
            let mut successful_mining = 0;
            while let Ok(task) = mining_rx.recv() {
                mining_count += 1;
                println!("Получена задача майнинга {} в потоке {:?}", mining_count, thread::current().id());
                let progress_tx_clone = task.progress_tx.clone(); // Клонируем Sender для использования в mine_block
                let start_time = SystemTime::now();
                // Выполняем майнинг и сохраняем результат до захвата Mutex
                let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut blockchain = task.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                    if blockchain.add_transaction(task.transaction.clone()) {
                        blockchain.mine_block(progress_tx_clone)
                    } else {
                        let _ = task.progress_tx.send("Ошибка: Не удалось добавить транзакцию".to_string());
                        None
                    }
                })) {
                    Ok(result) => result,
                    Err(panic) => {
                        let err_msg = match panic.downcast_ref::<&str>() {
                            Some(s) => s.to_string(),
                            None => format!("Неизвестная паника: {:?}", panic),
                        };
                        println!("Паника в потоке майнинга: {}", err_msg);
                        let _ = task.progress_tx.send(format!("Паника в потоке майнинга: {}", err_msg));
                        None
                    }
                };
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                total_duration += duration;
                println!("Майнинг {} завершен за {} секунд с результатом: {:?}", mining_count, duration, result);
                // Обновляем статус майнинга с несколькими попытками
                let mut attempts = 0;
                let max_attempts = 5;
                let mut status_updated = false;
                while attempts < max_attempts {
                    if let Ok(mut mining_status) = task.mining_status.try_lock() {
                        *mining_status = match result {
                            Some(block) => {
                                successful_mining += 1;
                                println!("Майнинг {} успешен, блок добавлен", mining_count);
                                MiningStatus::Completed(Some(block))
                            }
                            None => {
                                println!("Майнинг {} не удался: нет транзакций или превышен лимит итераций/таймаут", mining_count);
                                MiningStatus::Failed("Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут".to_string())
                            }
                        };
                        status_updated = true;
                        println!("Статус майнинга {} обновлён: {:?}", mining_count, *mining_status);
                        break;
                    } else {
                        attempts += 1;
                        println!("Попытка {} обновить статус майнинга {} не удалась", attempts, mining_count);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                if !status_updated {
                    println!("Не удалось обновить статус майнинга {} после {} попыток", mining_count, max_attempts);
                    let _ = task.progress_tx.send(format!("Ошибка: Не удалось обновить статус майнинга {} после {} попыток", mining_count, max_attempts));
                }
                if mining_count > 0 {
                    println!("Среднее время майнинга после {} задач: {} секунд", mining_count, total_duration / mining_count as f64);
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
        peers.push("127.0.0.1:8081".to_string());
        peers.push("127.0.0.1:8082".to_string());
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Обнаружение пиров завершено за {} секунд: {:?}", duration, *peers);
    }

    fn find_wallet_by_ip(&self, ip: &str) -> Option<String> {
        let start_time = SystemTime::now();
        let ip = ip.trim();
        let ip = if !ip.contains(':') {
            format!("{}:8081", ip)
        } else {
            ip.to_string()
        };
        if ip.parse::<std::net::SocketAddr>().is_err() {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Некорректный формат IP: {}, проверка заняла {} секунд", ip, duration);
            return None;
        }
        let peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        println!("Список пиров: {:?}", *peers);
        if peers.contains(&ip) {
            let wallet = format!("wallet{}", rand::thread_rng().gen_range(1..3));
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("Найден кошелек: {}, поиск занял {} секунд", wallet, duration);
            Some(wallet)
        } else {
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            println!("IP {} не найден в списке пиров, поиск занял {} секунд", ip, duration);
            None
        }
    }

    fn start_server(&self, port: u16) {
        let start_time = SystemTime::now();
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        let blockchain = Arc::clone(&self.blockchain);

        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buffer = [0; 1024];
                stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..]).to_string();

                if request.contains("GET_BLOCKCHAIN") {
                    let blockchain = blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                    let response = serde_json::to_string(&*blockchain).unwrap();
                    stream.write_all(response.as_bytes()).unwrap();
                }
            }
        });
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        println!("Сервер запущен на порту {} за {} секунд", port, duration);
    }

    fn sync_blockchain(&self) {
        let start_time = SystemTime::now();
        let peers = self.peers.lock().expect("Не удалось захватить Mutex для peers");
        for peer in peers.iter() {
            if let Ok(mut stream) = TcpStream::connect(peer) {
                stream.write_all(b"GET_BLOCKCHAIN").unwrap();
                let mut buffer = [0; 1024];
                stream.read(&mut buffer).unwrap();
                let response = String::from_utf8_lossy(&buffer[..]).to_string();
                if let Ok(received_blockchain) = serde_json::from_str::<Blockchain>(&response) {
                    let mut blockchain = self.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                    if received_blockchain.chain.len() > blockchain.chain.len() {
                        *blockchain = received_blockchain;
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
        println!("UI обновляется в потоке {:?}", thread::current().id());
        let now = ctx.input(|i| i.time);
        if now - self.last_repaint > 0.01 {
            ctx.request_repaint();
            self.last_repaint = now;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
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
                ui.heading("Кошелек");
                ui.label(format!("Адрес: {}", self.wallet_address));
                let balance = {
                    let blockchain = self.node.blockchain.lock().expect("Не удалось захватить Mutex для blockchain");
                    *blockchain.balances.get(&self.wallet_address).unwrap_or(&0)
                };
                ui.label(format!("Баланс: {}", balance));

                ui.heading("Перевод");
                ui.text_edit_singleline(&mut self.receiver_address);
                ui.text_edit_singleline(&mut self.amount);

                // Проверяем прогресс майнинга без блокировки
                if let Some(ref progress_rx) = self.progress_rx {
                    while let Ok(progress) = progress_rx.try_recv() {
                        let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                        *mining_progress = Some(progress);
                        println!("Прогресс майнинга обновлен в UI: {:?}", *mining_progress);
                    }
                }

                // Проверяем статус майнинга с помощью try_lock
                let mining_status_result = self.mining_status.try_lock();
                let is_mining = match &mining_status_result {
                    Ok(mining_status) => matches!(**mining_status, MiningStatus::Mining),
                    Err(e) => {
                        println!("Не удалось захватить Mutex для mining_status в UI: {}", e);
                        self.status = format!("Ошибка: Не удалось проверить статус майнинга: {}", e);
                        ctx.request_repaint();
                        false
                    }
                };

                if is_mining {
                    ui.label("Майнинг блока в процессе...");
                    let progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                    if let Some(progress_msg) = &*progress {
                        ui.label(format!("Прогресс: {}", progress_msg));
                    }
                    ui.spinner();
                    ctx.request_repaint();
                } else {
                    match mining_status_result {
                        Ok(mining_status) => match &*mining_status {
                            MiningStatus::Completed(block) => {
                                self.status = format!("Транзакция отправлена, блок добавлен: {:?}", block);
                                if let Ok(mut mining_status) = self.mining_status.try_lock() {
                                    *mining_status = MiningStatus::Idle;
                                    self.progress_rx = None;
                                    println!("Статус майнинга сброшен на Idle");
                                } else {
                                    println!("Не удалось сбросить статус майнинга: Mutex занят");
                                    self.status = "Ошибка: Не удалось сбросить статус майнинга".to_string();
                                }
                                ctx.request_repaint();
                            }
                            MiningStatus::Failed(err) => {
                                self.status = err.clone();
                                if let Ok(mut mining_status) = self.mining_status.try_lock() {
                                    *mining_status = MiningStatus::Idle;
                                    self.progress_rx = None;
                                    println!("Статус майнинга сброшен на Idle");
                                } else {
                                    println!("Не удалось сбросить статус майнинга: Mutex занят");
                                    self.status = "Ошибка: Не удалось сбросить статус майнинга".to_string();
                                }
                                ctx.request_repaint();
                            }
                            MiningStatus::Idle => {
                                // Отключаем кнопку, если майнинг уже идет
                                if is_mining {
                                    ui.add_enabled(false, egui::Button::new("Отправить"));
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
                                            sender: self.wallet_address.clone(),
                                            receiver: self.receiver_address.trim().to_string(),
                                            amount,
                                        };
                                        let blockchain = Arc::clone(&self.node.blockchain);
                                        let mining_status = Arc::clone(&self.mining_status);
                                        let mining_progress = Arc::clone(&self.mining_progress);

                                        // Добавляем транзакцию в blockchain
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
                                        {
                                            let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                                            *mining_progress = None;
                                            self.progress_rx = Some(progress_rx);
                                            println!("Канал прогресса создан");
                                        }
                                        // Пытаемся установить статус майнинга с несколькими попытками
                                        let mut attempts = 0;
                                        let max_attempts = 5;
                                        let mut mining_status_set = false;
                                        while attempts < max_attempts {
                                            match self.mining_status.try_lock() {
                                                Ok(mut mining_status) => {
                                                    *mining_status = MiningStatus::Mining;
                                                    println!("Статус майнинга установлен: Mining");
                                                    mining_status_set = true;
                                                    break;
                                                }
                                                Err(e) => {
                                                    attempts += 1;
                                                    println!("Попытка {} не удалась: {}", attempts, e);
                                                    if attempts == max_attempts {
                                                        self.status = format!("Ошибка: Не удалось установить статус майнинга после {} попыток: {}", max_attempts, e);
                                                        println!("Не удалось установить статус майнинга после {} попыток: {}", max_attempts, e);
                                                        self.progress_rx = None;
                                                        ctx.request_repaint();
                                                        return;
                                                    }
                                                    // Небольшая задержка перед следующей попыткой
                                                    std::thread::sleep(Duration::from_millis(100));
                                                }
                                            }
                                        }
                                        if !mining_status_set {
                                            return;
                                        }
                                        println!("Отправка задачи майнинга");
                                        if let Err(e) = self.mining_tx.send(MiningTask {
                                            blockchain,
                                            transaction,
                                            mining_status,
                                            progress_tx,
                                        }) {
                                            self.status = format!("Ошибка отправки задачи майнинга: {}", e);
                                            println!("Ошибка отправки задачи майнинга: {}", e);
                                            if let Ok(mut mining_status) = self.mining_status.try_lock() {
                                                *mining_status = MiningStatus::Idle;
                                                println!("Статус майнинга сброшен на Idle");
                                            }
                                            self.progress_rx = None;
                                            ctx.request_repaint();
                                            return;
                                        }
                                        println!("Задача майнинга отправлена");
                                        let duration = SystemTime::now()
                                            .duration_since(start_time)
                                            .unwrap()
                                            .as_secs_f64();
                                        println!("Запуск майнинга завершен за {} секунд", duration);
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
                            }
                            MiningStatus::Mining => {} // Уже обработано выше
                        },
                        Err(e) => {
                            self.status = format!("Ошибка: Не удалось проверить статус майнинга: {}", e);
                            println!("Не удалось проверить статус майнинга: {}", e);
                            ctx.request_repaint();
                        }
                    }
                }

                ui.heading("Поиск кошелька по IP");
                let mut ip = String::new();
                ui.text_edit_singleline(&mut ip);
                if ui.button("Найти кошелек").clicked() {
                    let start_time = SystemTime::now();
                    println!("Кнопка 'Найти кошелек' нажата, IP: {}", ip);
                    let ip = ip.trim();
                    if ip.is_empty() {
                        self.status = "IP-адрес не может быть пустым".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Ошибка: пустой IP-адрес, проверка заняла {} секунд", duration);
                    } else if let Some(wallet) = self.node.find_wallet_by_ip(ip) {
                        self.status = format!("Найден кошелек: {}", wallet);
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Кошелек найден: {}, поиск занял {} секунд", wallet, duration);
                    } else {
                        self.status = format!("Кошелек не найден для IP: {}", ip);
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        println!("Кошелек не найден для IP: {}, поиск занял {} секунд", ip, duration);
                    }
                    ctx.request_repaint();
                }

                ui.label(&self.status);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    // Модуль тестов оставлен пустым, так как test_two_clients удален
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

    let (mining_tx, mining_rx) = mpsc::channel();
    let mut node = Node::new(format!("127.0.0.1:{}", config.wallet.port), mining_rx);
    node.start_server(config.wallet.port);
    node.discover_peers();

    let app = WalletApp {
        node,
        wallet_address: config.wallet.name,
        password: config.wallet.password,
        is_authenticated: false,
        receiver_address: String::new(),
        amount: String::new(),
        status: String::new(),
        mining_status: Arc::new(Mutex::new(MiningStatus::Idle)),
        mining_progress: Arc::new(Mutex::new(None)),
        progress_rx: None,
        mining_tx,
        mining_thread: None,
        last_repaint: 0.0,
    };

    eframe::run_native(
        "Blockchain Wallet",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Box::new(app)),
    )
        .expect("Ошибка запуска приложения");
}
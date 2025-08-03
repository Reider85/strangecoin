use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream, IpAddr};
use std::sync::{Arc, Mutex};
use std::thread;
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
            difficulty: 1,
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

    fn mine_block(&mut self) -> Option<Block> {
        let total_start_time = SystemTime::now();
        println!("Начало майнинга...");
        if self.pending_transactions.is_empty() {
            println!("Нет транзакций для майнинга");
            return None;
        }

        let previous_block = self.chain.last().unwrap();
        let mut block = Block {
            index: previous_block.index + 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: self.pending_transactions.clone(),
            previous_hash: previous_block.hash.clone(),
            hash: String::new(),
            nonce: 0,
        };

        let start_time = SystemTime::now();
        let mut iteration_count = 0;
        let max_iterations = 100_000; // Ограничение на количество итераций
        loop {
            if iteration_count >= max_iterations {
                println!("Достигнуто максимальное количество итераций: {}", max_iterations);
                return None;
            }
            iteration_count += 1;
            let hash_start_time = SystemTime::now();
            let hash = self.calculate_hash(&block);
            let hash_duration = SystemTime::now()
                .duration_since(hash_start_time)
                .unwrap()
                .as_secs_f64();
            println!(
                "Итерация {}, nonce: {}, хэш: {}, время вычисления хэша: {} секунд",
                iteration_count, block.nonce, hash, hash_duration
            );
            if hash.starts_with(&"0".repeat(self.difficulty as usize)) {
                block.hash = hash;
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Подходящий хэш найден после {} итераций за {} секунд",
                    iteration_count, duration
                );
                break;
            }
            block.nonce += 1;
            if iteration_count % 100 == 0 {
                let progress_duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                println!(
                    "Прогресс майнинга: {} итераций выполнено за {} секунд",
                    iteration_count, progress_duration
                );
            }
        }

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
        println!("Майнинг завершен за {} секунд, итераций: {}", total_duration, iteration_count);
        Some(block)
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
    fn new(address: String) -> Self {
        Node {
            blockchain: Arc::new(Mutex::new(Blockchain::new())),
            peers: Arc::new(Mutex::new(vec![])),
            address,
        }
    }

    fn discover_peers(&mut self) {
        let start_time = SystemTime::now();
        let mut peers = self.peers.lock().unwrap();
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
        let peers = self.peers.lock().unwrap();
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
                    let blockchain = blockchain.lock().unwrap();
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
        let peers = self.peers.lock().unwrap();
        for peer in peers.iter() {
            if let Ok(mut stream) = TcpStream::connect(peer) {
                stream.write_all(b"GET_BLOCKCHAIN").unwrap();
                let mut buffer = [0; 1024];
                stream.read(&mut buffer).unwrap();
                let response = String::from_utf8_lossy(&buffer[..]).to_string();
                if let Ok(received_blockchain) = serde_json::from_str::<Blockchain>(&response) {
                    let mut blockchain = self.blockchain.lock().unwrap();
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
        let now = ctx.input(|i| i.time);
        if now - self.last_repaint > 0.05 {
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
                let blockchain = self.node.blockchain.lock().unwrap();
                let balance = blockchain.balances.get(&self.wallet_address).unwrap_or(&0);
                ui.label(format!("Баланс: {}", balance));
                drop(blockchain);

                ui.heading("Перевод");
                ui.text_edit_singleline(&mut self.receiver_address);
                ui.text_edit_singleline(&mut self.amount);

                {
                    let mut mining_status = self.mining_status.lock().unwrap();
                    match &*mining_status {
                        MiningStatus::Mining => {
                            ui.label("Майнинг блока в процессе...");
                            ui.spinner();
                            ctx.request_repaint();
                        }
                        MiningStatus::Completed(block) => {
                            self.status = format!("Транзакция отправлена, блок добавлен: {:?}", block);
                            *mining_status = MiningStatus::Idle;
                            ctx.request_repaint();
                        }
                        MiningStatus::Failed(err) => {
                            self.status = err.clone();
                            *mining_status = MiningStatus::Idle;
                            ctx.request_repaint();
                        }
                        MiningStatus::Idle => {
                            if ui.button("Отправить").clicked() {
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

                                    {
                                        let mut blockchain = blockchain.lock().unwrap();
                                        if !blockchain.add_transaction(transaction) {
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
                                    println!("Запуск майнинга в отдельном потоке");
                                    {
                                        let mut mining_status = mining_status.lock().unwrap();
                                        *mining_status = MiningStatus::Mining;
                                        println!("Статус майнинга установлен: Mining");
                                    }
                                    let blockchain = Arc::clone(&self.node.blockchain);
                                    let mining_status = Arc::clone(&self.mining_status);
                                    thread::spawn(move || {
                                        println!("Поток майнинга начат");
                                        let mut blockchain = blockchain.lock().unwrap();
                                        let result = blockchain.mine_block();
                                        let mut mining_status = mining_status.lock().unwrap();
                                        *mining_status = match result {
                                            Some(block) => {
                                                println!("Майнинг успешен, блок добавлен");
                                                MiningStatus::Completed(Some(block))
                                            }
                                            None => {
                                                println!("Майнинг не удался: нет транзакций или превышен лимит итераций");
                                                MiningStatus::Failed("Майнинг не удался: нет транзакций или превышен лимит итераций".to_string())
                                            }
                                        };
                                        println!("Статус майнинга обновлён: {:?}", *mining_status);
                                    });
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
    use super::*;

    #[test]
    fn test_two_clients() {
        let exe_path = std::env::current_exe().expect("Не удалось определить путь к исполняемому файлу");
        let exe_dir = exe_path.parent().expect("Не удалось получить директорию исполняемого файла");

        let config_path1 = exe_dir.join("config.json");
        let config_content1 = fs::read_to_string(&config_path1).unwrap_or_else(|err| {
            eprintln!("Ошибка чтения {}: {}. Используются значения по умолчанию.", config_path1.display(), err);
            r#"{"wallet": {"name": "wallet1", "password": "password", "port": 8081}}"#.to_string()
        });
        let config1: Config = serde_json::from_str(&config_content1).expect("Ошибка парсинга конфигурации");

        let node1 = Node::new(format!("127.0.0.1:{}", config1.wallet.port));
        node1.start_server(config1.wallet.port);
        let app1 = WalletApp {
            node: node1.clone(),
            wallet_address: config1.wallet.name,
            password: config1.wallet.password,
            is_authenticated: false,
            receiver_address: String::new(),
            amount: String::new(),
            status: String::new(),
            mining_status: Arc::new(Mutex::new(MiningStatus::Idle)),
            last_repaint: 0.0,
        };
        thread::spawn(move || {
            eframe::run_native(
                "Client 1",
                eframe::NativeOptions::default(),
                Box::new(|_cc| Box::new(app1)),
            )
                .unwrap();
        });

        let config_path2 = exe_dir.join("config2.json");
        let config_content2 = fs::read_to_string(&config_path2).unwrap_or_else(|err| {
            eprintln!("Ошибка чтения {}: {}. Используются значения по умолчанию.", config_path2.display(), err);
            r#"{"wallet": {"name": "wallet2", "password": "password", "port": 8082}}"#.to_string()
        });
        let config2: Config = serde_json::from_str(&config_content2).expect("Ошибка парсинга конфигурации");

        let node2 = Node::new(format!("127.0.0.1:{}", config2.wallet.port));
        node2.start_server(config2.wallet.port);
        let app2 = WalletApp {
            node: node2.clone(),
            wallet_address: config2.wallet.name,
            password: config2.wallet.password,
            is_authenticated: false,
            receiver_address: String::new(),
            amount: String::new(),
            status: String::new(),
            mining_status: Arc::new(Mutex::new(MiningStatus::Idle)),
            last_repaint: 0.0,
        };
        thread::spawn(move || {
            eframe::run_native(
                "Client 2",
                eframe::NativeOptions::default(),
                Box::new(|_cc| Box::new(app2)),
            )
                .unwrap();
        });

        thread::sleep(Duration::from_secs(5));

        node1.discover_peers();
        node2.discover_peers();
        node1.sync_blockchain();
        node2.sync_blockchain();
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

    let mut node = Node::new(format!("127.0.0.1:{}", config.wallet.port));
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
        last_repaint: 0.0,
    };

    eframe::run_native(
        "Blockchain Wallet",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Box::new(app)),
    )
        .unwrap();
}
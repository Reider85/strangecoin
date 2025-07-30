use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream, IpAddr};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
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
    #[allow(dead_code)]
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
            difficulty: 1, // Уменьшено для ускорения майнинга
            pending_transactions: vec![],
        };
        blockchain.create_genesis_block();
        blockchain
    }

    fn create_genesis_block(&mut self) {
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
    }

    fn calculate_hash(&self, block: &Block) -> String {
        let input = format!(
            "{}{}{}{}",
            block.index,
            block.timestamp,
            serde_json::to_string(&block.transactions).unwrap(),
            block.previous_hash
        );
        let mut hasher = Sha256::new();
        hasher.update(input);
        format!("{:x}", hasher.finalize())
    }

    fn mine_block(&mut self) -> Option<Block> {
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

        loop {
            let hash = self.calculate_hash(&block);
            if hash.starts_with(&"0".repeat(self.difficulty as usize)) {
                block.hash = hash;
                break;
            }
            block.nonce += 1;
        }

        for tx in &block.transactions {
            *self.balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
            *self.balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
        }

        self.pending_transactions.clear();
        self.chain.push(block.clone());
        println!("Майнинг завершен: {:?}", block);
        Some(block)
    }

    fn add_transaction(&mut self, transaction: Transaction) -> bool {
        if let Some(sender_balance) = self.balances.get(&transaction.sender) {
            if *sender_balance >= transaction.amount {
                self.pending_transactions.push(transaction);
                println!("Транзакция добавлена: {:?}", self.pending_transactions);
                return true;
            }
        }
        println!("Ошибка: Недостаточно средств или неверный адрес");
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
        let mut peers = self.peers.lock().unwrap();
        peers.push("127.0.0.1:8081".to_string());
        peers.push("127.0.0.1:8082".to_string());
        println!("Обнаружены пиры: {:?}", *peers);
    }

    fn find_wallet_by_ip(&self, ip: &str) -> Option<String> {
        let ip = ip.trim();
        if ip.parse::<IpAddr>().is_err() {
            println!("Некорректный формат IP: {}", ip);
            return None;
        }
        let peers = self.peers.lock().unwrap();
        println!("Список пиров: {:?}", *peers);
        if peers.contains(&ip.to_string()) {
            let wallet = format!("wallet{}", rand::thread_rng().gen_range(1..3));
            println!("Найден кошелек: {}", wallet);
            Some(wallet)
        } else {
            println!("IP {} не найден в списке пиров", ip);
            None
        }
    }

    fn start_server(&self, port: u16) {
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
    }

    #[allow(dead_code)]
    fn sync_blockchain(&self) {
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
    }
}

impl eframe::App for WalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.is_authenticated {
                ui.heading("Аутентификация");
                ui.text_edit_singleline(&mut self.wallet_address);
                ui.text_edit_singleline(&mut self.password);
                if ui.button("Войти").clicked() {
                    if self.password == "password" {
                        self.is_authenticated = true;
                        self.status = "Успешная аутентификация".to_string();
                    } else {
                        self.status = "Неверный пароль".to_string();
                    }
                    ctx.request_repaint();
                }
            } else {
                ui.heading("Кошелек");
                ui.label(format!("Адрес: {}", self.wallet_address));
                let blockchain = self.node.blockchain.lock().unwrap();
                let balance = blockchain.balances.get(&self.wallet_address).unwrap_or(&0);
                ui.label(format!("Баланс: {}", balance));

                ui.heading("Перевод");
                ui.text_edit_singleline(&mut self.receiver_address);
                ui.text_edit_singleline(&mut self.amount);

                // Проверяем статус майнинга
                {
                    let mut mining_status = self.mining_status.lock().unwrap();
                    match &*mining_status {
                        MiningStatus::Mining => {
                            ui.label("Майнинг блока в процессе...");
                            ui.spinner();
                            ctx.request_repaint(); // Обновляем UI во время майнинга
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
                                if let Ok(amount) = self.amount.parse::<u64>() {
                                    let transaction = Transaction {
                                        sender: self.wallet_address.clone(),
                                        receiver: self.receiver_address.clone(),
                                        amount,
                                    };
                                    let blockchain = Arc::clone(&self.node.blockchain);
                                    let mining_status = Arc::clone(&self.mining_status);

                                    // Проверяем возможность добавления транзакции
                                    {
                                        let mut blockchain = blockchain.lock().unwrap();
                                        if !blockchain.add_transaction(transaction) {
                                            self.status = "Недостаточно средств или неверный адрес".to_string();
                                            ctx.request_repaint();
                                            return;
                                        }
                                    }

                                    // Запускаем майнинг в отдельном потоке
                                    self.status = "Запуск майнинга...".to_string();
                                    {
                                        let mut mining_status = mining_status.lock().unwrap();
                                        *mining_status = MiningStatus::Mining;
                                    }
                                    thread::spawn(move || {
                                        let mut blockchain = blockchain.lock().unwrap();
                                        let result = blockchain.mine_block();
                                        let mut mining_status = mining_status.lock().unwrap();
                                        *mining_status = match result {
                                            Some(block) => MiningStatus::Completed(Some(block)),
                                            None => MiningStatus::Failed("Нет транзакций для майнинга".to_string()),
                                        };
                                    });
                                    ctx.request_repaint();
                                } else {
                                    self.status = "Неверный формат суммы".to_string();
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
                    let ip = ip.trim();
                    if ip.parse::<IpAddr>().is_ok() {
                        if let Some(wallet) = self.node.find_wallet_by_ip(ip) {
                            self.status = format!("Найден кошелек: {}", wallet);
                        } else {
                            self.status = format!("Кошелек не найден для IP: {}", ip);
                        }
                    } else {
                        self.status = "Некорректный формат IP-адреса".to_string();
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
        };
        thread::spawn(move || {
            eframe::run_native(
                "Client 2",
                eframe::NativeOptions::default(),
                Box::new(|_cc| Box::new(app2)),
            )
                .unwrap();
        });

        thread::sleep(std::time::Duration::from_secs(5));

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
    };

    eframe::run_native(
        "Blockchain Wallet",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Box::new(app)),
    )
        .unwrap();
}
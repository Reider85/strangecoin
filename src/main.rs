use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use eframe::{egui, App};
use std::io::{Read, Write};
use rand::Rng;

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
}

impl Blockchain {
    fn new() -> Self {
        let mut blockchain = Blockchain {
            chain: vec![],
            balances: HashMap::new(),
            difficulty: 2,
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
        // Начальный баланс для тестов
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
        if self.pending_transactions.is_empty() {
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

        // Proof of Work
        loop {
            let hash = self.calculate_hash(&block);
            if hash.starts_with(&"0".repeat(self.difficulty as usize)) {
                block.hash = hash;
                break;
            }
            block.nonce += 1;
        }

        // Обновление балансов
        for tx in &block.transactions {
            *self.balances.entry(tx.sender.clone()).or_insert(0) -= tx.amount;
            *self.balances.entry(tx.receiver.clone()).or_insert(0) += tx.amount;
        }

        self.pending_transactions.clear();
        self.chain.push(block.clone());
        Some(block)
    }

    fn add_transaction(&mut self, transaction: Transaction) -> bool {
        if let Some(sender_balance) = self.balances.get(&transaction.sender) {
            if *sender_balance >= transaction.amount {
                self.pending_transactions.push(transaction);
                return true;
            }
        }
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

    // Поиск узлов в сети
    fn discover_peers(&mut self) {
        let mut peers = self.peers.lock().unwrap();
        // Имитация поиска узлов (в реальной сети можно использовать UDP-бродкаст)
        peers.push("127.0.0.1:8081".to_string());
        peers.push("127.0.0.1:8082".to_string());
    }

    // Поиск кошелька по IP
    fn find_wallet_by_ip(&self, ip: &str) -> Option<String> {
        let peers = self.peers.lock().unwrap();
        if peers.contains(&ip.to_string()) {
            // Для простоты возвращаем случайный адрес
            Some(format!("wallet{}", rand::thread_rng().gen_range(1..3)))
        } else {
            None
        }
    }

    // Запуск сервера для обработки входящих соединений
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

    // Синхронизация с другими узлами
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
                    // Простая проверка пароля
                    if self.password == "password" {
                        self.is_authenticated = true;
                        self.status = "Успешная аутентификация".to_string();
                    } else {
                        self.status = "Неверный пароль".to_string();
                    }
                }
            } else {
                ui.heading("Кошелек");
                ui.label(format!("Адрес: {}", self.wallet_address));
                let balance = self
                    .node
                    .blockchain
                    .lock()
                    .unwrap()
                    .balances
                    .get(&self.wallet_address)
                    .unwrap_or(&0);
                ui.label(format!("Баланс: {}", balance));

                ui.heading("Перевод");
                ui.text_edit_singleline(&mut self.receiver_address);
                ui.text_edit_singleline(&mut self.amount);
                if ui.button("Отправить").clicked() {
                    if let Ok(amount) = self.amount.parse::<u64>() {
                        let transaction = Transaction {
                            sender: self.wallet_address.clone(),
                            receiver: self.receiver_address.clone(),
                            amount,
                        };
                        let mut blockchain = self.node.blockchain.lock().unwrap();
                        if blockchain.add_transaction(transaction) {
                            if let Some(block) = blockchain.mine_block() {
                                self.status = format!("Транзакция отправлена, блок добавлен: {:?}", block);
                            } else {
                                self.status = "Ошибка при майнинге блока".to_string();
                            }
                        } else {
                            self.status = "Недостаточно средств или неверный адрес".to_string();
                        }
                    } else {
                        self.status = "Неверный формат суммы".to_string();
                    }
                }

                ui.heading("Поиск кошелька по IP");
                let mut ip = String::new();
                ui.text_edit_singleline(&mut ip);
                if ui.button("Найти кошелек").clicked() {
                    if let Some(wallet) = self.node.find_wallet_by_ip(&ip) {
                        self.status = format!("Найден кошелек: {}", wallet);
                    } else {
                        self.status = "Кошелек не найден".to_string();
                    }
                }

                ui.label(&self.status);
            }
        });
    }
}

// Тест для запуска двух клиентов
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_two_clients() {
        // Клиент 1
        let node1 = Node::new("127.0.0.1:8081".to_string());
        node1.start_server(8081);
        let app1 = WalletApp {
            node: node1.clone(),
            wallet_address: "wallet1".to_string(),
            password: "password".to_string(),
            is_authenticated: false,
            receiver_address: String::new(),
            amount: String::new(),
            status: String::new(),
        };
        thread::spawn(move || {
            eframe::run_native(
                "Client 1",
                eframe::NativeOptions::default(),
                Box::new(|_cc| Box::new(app1)),
            )
                .unwrap();
        });

        // Клиент 2
        let node2 = Node::new("127.0.0.1:8082".to_string());
        node2.start_server(8082);
        let app2 = WalletApp {
            node: node2.clone(),
            wallet_address: "wallet2".to_string(),
            password: "password".to_string(),
            is_authenticated: false,
            receiver_address: String::new(),
            amount: String::new(),
            status: String::new(),
        };
        thread::spawn(move || {
            eframe::run_native(
                "Client 2",
                eframe::NativeOptions::default(),
                Box::new(|_cc| Box::new(app2)),
            )
                .unwrap();
        });

        // Даем время клиентам запуститься
        thread::sleep(std::time::Duration::from_secs(5));

        // Проверка синхронизации
        node1.discover_peers();
        node2.discover_peers();
        node1.sync_blockchain();
        node2.sync_blockchain();
    }
}

fn main() {
    let node = Node::new("127.0.0.1:8081".to_string());
    node.start_server(8081);
    node.discover_peers();

    let app = WalletApp {
        node,
        wallet_address: "wallet1".to_string(),
        password: String::new(),
        is_authenticated: false,
        receiver_address: String::new(),
        amount: String::new(),
        status: String::new(),
    };

    eframe::run_native(
        "Blockchain Wallet",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Box::new(app)),
    )
        .unwrap();
}
#[cfg(feature = "gui")]
use crate::error::StrangecoinError;
#[cfg(feature = "gui")]
use eframe::egui;
use std::fs;
use std::io::Write;
use std::io::{BufReader, BufWriter};
use std::net::SocketAddr;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, error, info, warn};

use crate::network::sync_engine::{Inbox, Incoming};

pub mod address;
pub mod api;
pub mod blockchain;
pub mod cli;
pub mod config;
pub mod consensus;
pub mod economics;
pub mod error;
pub mod events;
pub mod governance;
#[cfg(feature = "gui")]
pub mod gui;
pub mod mempool;
pub mod network;
pub mod storage;
pub mod wallet;

pub use blockchain::{Blockchain, BlockchainDeserialize, BlockchainFacade, ConsensusManager};
pub use strangecoin_core::serialize;
pub use strangecoin_core::types::{Block, ChainSnapshot, Transaction};
pub use strangecoin_core::AccountState;

/// Sync task tick period: how often the shutdown flag is re-checked.
pub const SYNC_TICK: Duration = Duration::from_millis(100);
/// Number of `SYNC_TICK`s between two `sync_blockchain` rounds (~1s).
pub const SYNC_TICKS_PER_SYNC: u32 = 10;
/// Budget granted to in-flight work after a shutdown signal (P17).
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
/// Height reserved for the opt-in primary-issuance grant block
/// (created only when `allow_grant_blocks` is explicitly enabled).
pub const GRANT_BLOCK_INDEX: u64 = 1;

#[derive(Clone)]
pub struct MiningTask {
    pub blockchain: Arc<BlockchainFacade>,
    pub transaction: Transaction,
    pub mining_status: Arc<Mutex<MiningStatus>>,
    pub progress_tx: mpsc::Sender<String>,
    pub status_tx: mpsc::Sender<String>,
    pub rate_limiter: Arc<crate::network::RateLimiter>,
    pub shutdown: Arc<AtomicBool>,
    pub event_bus: Arc<events::EventBus>,
    /// SyncEngine inbox of the node this task was created for (ADR-0010):
    /// the post-mining sync round enqueues candidates instead of adopting.
    pub inbox: Option<Inbox>,
}

pub struct Node {
    pub blockchain: Arc<BlockchainFacade>,
    pub peers: Arc<Mutex<Vec<String>>>,
    pub address: String,
    pub sync_rx: mpsc::Receiver<ChainSnapshot>,
    pub rate_limiter: Arc<crate::network::RateLimiter>,
    pub shutdown: Arc<AtomicBool>,
    pub listener: Arc<Mutex<Option<TcpListener>>>,
    pub sync_thread_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub event_bus: Arc<events::EventBus>,
    pub network_id: u32,
    /// SyncEngine inbox, spawned by `start_server` (ADR-0010). `None` until
    /// the server starts; the sync task and mining inherit it from here.
    pub inbox: Option<Inbox>,
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

impl Node {
    fn new(
        address: String,
        mining_rx: mpsc::Receiver<MiningTask>,
        port: u16,
        event_bus: Arc<events::EventBus>,
        network_id: u32,
    ) -> Self {
        let blockchain = Arc::new(BlockchainFacade::with_event_bus(
            port,
            event_bus.clone(),
        ));
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
            event_bus: event_bus.clone(),
            network_id,
            // Spawned by `start_server`.
            inbox: None,
        };
        thread::spawn(move || {
            info!("Фоновый поток майнинга запущен");
            let mut mining_count = 0;
            let mut total_duration = 0.0;
            let mut successful_mining = 0;
            while let Ok(task) = mining_rx.recv() {
                mining_count += 1;
                info!(mining_count, "Получена задача майнинга");
                task.event_bus.publish(events::NodeEvent::MiningStarted);
                let progress_tx_clone = task.progress_tx.clone();
                let status_tx_clone = task.status_tx.clone();
                let start_time = SystemTime::now();
                let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    debug!(mining_count, ?task.transaction, "Проверка транзакции");
                    match task.blockchain.apply_tx(task.transaction.clone()) {
                        Ok(_) => {
                            info!(
                                mining_count,
                                "Транзакция успешно добавлена, начало майнинга"
                            );
                            task.event_bus.publish(events::NodeEvent::TxAccepted {
                                txid: hex::encode(strangecoin_core::serialize::txid(
                                    &task.transaction,
                                )),
                            });
                            task.blockchain
                                .mine_block(progress_tx_clone, &task.shutdown)
                        }
                        Err(e) => {
                            warn!(mining_count, error = %e, "Транзакция отклонена, попытка майнить существующие транзакции");
                            task.event_bus.publish(events::NodeEvent::TxRejected {
                                txid: hex::encode(strangecoin_core::serialize::txid(
                                    &task.transaction,
                                )),
                                reason: format!("{}", e),
                            });
                            if !task.blockchain.mempool_is_empty() {
                                task.blockchain
                                    .mine_block(progress_tx_clone, &task.shutdown)
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
                task.event_bus.publish(events::NodeEvent::MiningFinished);
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
                                    event_bus: task.event_bus.clone(),
                                    network_id: crate::consensus::CHAIN_ID_REGTEST,
                                    inbox: task.inbox.clone(),
                                };
                                node_temp.sync_blockchain();
                                task.event_bus.publish(events::NodeEvent::BlockApplied {
                                    height: block.index,
                                    hash: block.hash.clone(),
                                });
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
            .next_back()
            .unwrap_or("0")
            .parse::<u16>()
            .unwrap_or(0);
        let empty_peers: Vec<serde_json::Value> = vec![];
        let peer_list = network_config["peers"].as_array().unwrap_or(&empty_peers);
        for peer in peer_list {
            if let Some(peer_str) = peer.as_str() {
                let peer_port = peer_str
                    .split(':')
                    .next_back()
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
            return self.blockchain.first_account();
        }
        None
    }

    pub fn start_server(&mut self, port: u16, sync_tx: mpsc::Sender<ChainSnapshot>) {
        let start_time = SystemTime::now();
        // ADR-0010: the engine spawned here is the only adopter of incoming
        // data; the handlers below only enqueue into its inbox.
        if self.inbox.is_none() {
            self.inbox = Some(crate::network::sync_engine::spawn(
                Arc::clone(&self.blockchain),
                Arc::clone(&self.event_bus),
                sync_tx.clone(),
                Arc::clone(&self.shutdown),
                Arc::clone(&self.rate_limiter),
            ));
        }
        let inbox = self
            .inbox
            .clone()
            .expect("SyncEngine inbox must exist after spawn");
        let blockchain = Arc::clone(&self.blockchain);
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let shutdown = Arc::clone(&self.shutdown);
        let network_id = self.network_id;
        let address = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&address).expect("Не удалось запустить сервер");
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
                        let inbox = inbox.clone();
                        let rate_limiter = Arc::clone(&rate_limiter);
                        let peer_addr = stream.peer_addr().ok();
                        let rate_limiter_clone = Arc::clone(&rate_limiter);
                        let shutdown = Arc::clone(&shutdown);
                        let _ = stream.set_read_timeout(Some(
                            crate::network::sync::SYNC_IO_TIMEOUT,
                        ));
                        
                        thread::spawn(move || {
                            if let Some(addr) = peer_addr {
                                if let Err(e) = rate_limiter_clone.check(addr) {
                                    warn!(peer = %addr, error = %e, "Rate limit exceeded, closing connection");
                                    return;
                                }
                            }
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let mut writer = BufWriter::new(stream);
                            
                            // First message must be HELLO handshake
                            let hello_bytes =
                                match crate::network::protocol::read_length_prefixed(&mut reader) {
                                    Ok(bytes) => bytes,
                                    Err(e) => {
                                        warn!(error = %e, "Failed to read HELLO handshake");
                                        return;
                                    }
                                };
                            let peer_network_id = match crate::network::protocol::parse_hello(&hello_bytes) {
                                Ok(id) => id,
                                Err(e) => {
                                    warn!(error = %e, "Invalid HELLO message from peer");
                                    return;
                                }
                            };
                            
                            if peer_network_id != network_id {
                                warn!(
                                    peer = %peer_addr.map_or_else(|| "unknown".to_string(), |addr| addr.to_string()),
                                    expected = network_id,
                                    got = peer_network_id,
                                    "Peer rejected: foreign network_id"
                                );
                                if let Some(addr) = peer_addr {
                                    rate_limiter_clone.ban(addr);
                                }
                                return;
                            }
                            
                            debug!(peer_network_id, "HELLO handshake successful");

                            let blockchain = Arc::clone(&blockchain);
                            use crate::network::protocol as proto;

                            // S1-P16: one connection serves many requests —
                            // a headers-first client iterates GET_HEADERS /
                            // GET_BLOCKS on a single session. The read
                            // timeout bounds idle handler threads; a client
                            // that just closes surfaces as EOF here.
                            loop {
                                if shutdown.load(Ordering::Relaxed) {
                                    break;
                                }
                                let request_bytes =
                                    match proto::read_length_prefixed(&mut reader) {
                                        Ok(bytes) => bytes,
                                        Err(e) => {
                                            debug!(error = %e, "Соединение закрыто после HELLO");
                                            break;
                                        }
                                    };

                                // Binary requests (S1-P16) — checked before
                                // the UTF-8 text path; text messages start
                                // with ASCII letters, tags with 0x01..=0x04.
                                match request_bytes.first() {
                                    Some(&proto::MSG_GET_HEADERS) => {
                                        let payload = match proto::parse_get_headers(&request_bytes)
                                        {
                                            Ok(from_height) => {
                                                let headers = blockchain.headers_from_height(
                                                    from_height,
                                                    proto::MAX_HEADERS_BATCH,
                                                );
                                                proto::encode_headers(&headers).map_err(|e| {
                                                    warn!(error = %e, "Не удалось закодировать заголовки");
                                                })
                                            }
                                            Err(e) => {
                                                warn!(error = %e, "Некорректный GET_HEADERS");
                                                Err(())
                                            }
                                        };
                                        match payload {
                                            Ok(payload) => {
                                                if let Err(e) =
                                                    proto::write_length_prefixed(&mut writer, &payload)
                                                {
                                                    debug!(error = %e, "Не удалось отправить заголовки");
                                                    break;
                                                }
                                            }
                                            Err(()) => break,
                                        }
                                        continue;
                                    }
                                    Some(&proto::MSG_GET_BLOCKS) => {
                                        let payload = match proto::parse_get_blocks(&request_bytes)
                                        {
                                            Ok(hashes) => {
                                                let blocks = blockchain.blocks_by_hashes(
                                                    &hashes,
                                                    proto::MAX_BLOCKS_BATCH,
                                                );
                                                proto::encode_blocks(&blocks).map_err(|e| {
                                                    warn!(error = %e, "Не удалось закодировать блоки");
                                                })
                                            }
                                            Err(e) => {
                                                warn!(error = %e, "Некорректный GET_BLOCKS");
                                                Err(())
                                            }
                                        };
                                        match payload {
                                            Ok(payload) => {
                                                if let Err(e) =
                                                    proto::write_length_prefixed(&mut writer, &payload)
                                                {
                                                    debug!(error = %e, "Не удалось отправить блоки");
                                                    break;
                                                }
                                            }
                                            Err(()) => break,
                                        }
                                        continue;
                                    }
                                    _ => {}
                                }

                                let request = String::from_utf8_lossy(&request_bytes).to_string();
                                debug!(request = %request, "Получен запрос");

                                if request == "GET_BLOCKCHAIN" {
                                    let response = blockchain.to_wire_json();
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
                                    let temp_blockchain: BlockchainDeserialize =
                                        match serde_json::from_str(blockchain_data) {
                                            Ok(data) => data,
                                            Err(e) => {
                                                error!(error = %e, "Ошибка десериализации данных блокчейна");
                                                return;
                                            }
                                        };
                                    // ADR-0010: parse and enqueue only — the
                                    // SyncEngine validates and adopts.
                                    inbox.push_inbound(Incoming::CandidateChain {
                                        chain: temp_blockchain.chain,
                                        balances: Some(temp_blockchain.balances),
                                        mempool_txs: temp_blockchain.mempool_txs,
                                        pending_transactions: temp_blockchain
                                            .pending_transactions,
                                        difficulty: temp_blockchain.difficulty,
                                        from: peer_addr,
                                    });
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
    pub fn sync_blockchain(&mut self) {
        let start_time = SystemTime::now();
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let shutdown = Arc::clone(&self.shutdown);
        let inbox = self.inbox.clone();
        self.discover_peers();
        let peers: Vec<String> = self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers")
            .iter()
            .cloned()
            .collect();
        info!(peers = ?peers, "Список пиров для синхронизации");

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
            if let Err(e) = rate_limiter.check(addr) {
                warn!(peer = %peer, error = %e, "Rate limit exceeded for outgoing request, skipping peer");
                continue;
            }
            let current_chain_length = self.blockchain.chain_len();
            debug!(current_chain_length, "Текущая длина chain");
            if current_chain_length <= 1 {
                info!("Новый узел, только получение данных, отправка цепочки запрещена");
            } else {
                // Отправка UPDATE_BLOCKCHAIN with HELLO handshake
                if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                    let mut writer = BufWriter::new(stream.try_clone().unwrap());

                    // Send HELLO handshake first
                    let hello_data = crate::network::protocol::encode_hello(self.network_id);
                    if writer.write_all(&hello_data).is_err() {
                        warn!(peer = %peer, "Failed to send HELLO handshake");
                        continue;
                    }
                    writer.flush().ok();

                    // Send UPDATE_BLOCKCHAIN
                    let response = self.blockchain.to_wire_json();
                    let message = format!("UPDATE_BLOCKCHAIN:{}", response);
                    let length = message.len() as u32;
                    let mut data = length.to_be_bytes().to_vec();
                    data.extend_from_slice(message.as_bytes());
                    if writer.write_all(&data).is_ok() {
                        writer.flush().ok();
                        info!(peer = %peer, message_len = data.len(), "Блокчейн отправлен узлу");
                    }
                }
            }

            // S1-P16: headers-first sync. Errors mean the peer does not
            // serve the binary protocol (pre-P16 peers never answer) — then
            // we fall back to GET_BLOCKCHAIN below. An enqueued candidate
            // skips the fallback; "nothing better" does not (mempool gossip
            // lives there). ADR-0010: the candidate is adopted by the engine.
            let mut candidate_via_headers = false;
            let local_chain = self.blockchain.chain_snapshot();
            match crate::network::sync::sync_headers_first(
                addr,
                self.network_id,
                &local_chain,
                &shutdown,
            ) {
                Ok(outcome) => {
                    if let Some(candidate) = outcome.candidate {
                        candidate_via_headers = true;
                        info!(
                            peer = %peer,
                            headers = outcome.headers_ingested,
                            blocks = outcome.blocks_downloaded,
                            "Кандидат (headers-first) поставлен в очередь"
                        );
                        match &inbox {
                            Some(inbox) => {
                                inbox.push(Incoming::CandidateChain {
                                    chain: candidate,
                                    balances: None,
                                    mempool_txs: Vec::new(),
                                    pending_transactions: Vec::new(),
                                    difficulty: self.blockchain.difficulty(),
                                    from: Some(addr),
                                });
                            }
                            None => warn!(
                                peer = %peer,
                                "SyncEngine inbox отсутствует: кандидат headers-first отброшен"
                            ),
                        }
                    } else {
                        debug!(
                            peer = %peer,
                            headers = outcome.headers_ingested,
                            "Лучшей ветки у пира нет"
                        );
                    }
                }
                Err(e) => {
                    debug!(
                        peer = %peer,
                        error = %e,
                        "Headers-first недоступен, переходим к GET_BLOCKCHAIN"
                    );
                }
            }
            if candidate_via_headers {
                continue;
            }

            // Запрос GET_BLOCKCHAIN with HELLO handshake
            if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = BufWriter::new(stream);

                // Send HELLO handshake first
                let hello_data = crate::network::protocol::encode_hello(self.network_id);
                if writer.write_all(&hello_data).is_err() {
                    warn!(peer = %peer, "Failed to send HELLO handshake");
                    continue;
                }
                writer.flush().ok();

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
                        Ok(received) => {
                            if received.chain.len() <= 1 {
                                info!(peer = %peer, chain_len = received.chain.len(), "Получена пустая или минимальная цепочка, игнорируем");
                                continue;
                            }
                            info!(peer = %peer, chain_len = received.chain.len(), "Полученная цепочка от узла");
                            // ADR-0010: parse and enqueue only — the
                            // SyncEngine validates and adopts.
                            match &inbox {
                                Some(inbox) => {
                                    inbox.push(Incoming::CandidateChain {
                                        chain: received.chain,
                                        balances: Some(received.balances),
                                        mempool_txs: received.mempool_txs,
                                        pending_transactions: received.pending_transactions,
                                        difficulty: received.difficulty,
                                        from: Some(addr),
                                    });
                                }
                                None => warn!(
                                    peer = %peer,
                                    "SyncEngine inbox отсутствует: кандидат отброшен"
                                ),
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

        // Cancel the sync task (tokio cancellation is abort-based, not blocking)
        if let Ok(mut handle) = self.sync_thread_handle.lock() {
            if let Some(h) = handle.take() {
                h.abort();
                info!("Sync task aborted");
            }
        }

        info!("Network node shutdown complete");
    }
}

#[cfg(feature = "gui")]
impl eframe::App for WalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if now - self.last_repaint > 0.01 {
            ctx.request_repaint();
            self.last_repaint = now;
        }

        while let Ok(received_snapshot) = self.node.sync_rx.try_recv() {
            if received_snapshot.chain.len() <= 1 {
                debug!("Получена пустая или минимальная цепочка через sync_rx, игнорируем");
                continue;
            }
            let wallet = self.wallet_address.clone();
            let old_balance = self.node.blockchain.get_balance(&wallet);
            let adopted = self
                .node
                .blockchain
                .adopt_wire(received_snapshot)
                .unwrap_or(false);
            if adopted {
                info!("UI: Блокчейн обновлён через канал синхронизации");
                let new_balance = self.node.blockchain.get_balance(&wallet);
                if old_balance != new_balance {
                    info!(wallet = %wallet, old_balance = old_balance, new_balance = new_balance, "Баланс кошелька изменился, запрашивается перерисовка");
                    ctx.request_repaint();
                } else {
                    debug!(wallet = %wallet, balance = old_balance, "Баланс кошелька не изменился, перерисовка не требуется");
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
                self.node.blockchain.mempool_len()
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
                                        let expected_address = crate::address::encode_address(&wallet.public_key, self.node.network_id)
                        .unwrap_or_else(|_| "invalid_address".to_string());
                                        if expected_address == self.wallet_address {
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
                                self.wallet_address = crate::address::encode_address(&wallet.public_key, self.node.network_id)
                        .unwrap_or_else(|_| "invalid_address".to_string());
                                self.password = password;
                                self.is_authenticated = true;
                                self.status = format!("Кошелёк успешно создан: {}", self.wallet_address);
                                self.node.discover_peers();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                info!(duration_secs = duration, wallet_address = %self.wallet_address, "Регистрация успешна");
                                if !self.node.blockchain.has_account(&self.wallet_address) {
                                    match self.node.blockchain.grant_initial_balance_to_first_wallet(&self.wallet_address) {
                                        Ok(true) => {}
                                        Ok(false) => {
                                            self.node.blockchain.ensure_account(&self.wallet_address);
                                        }
                                        Err(StrangecoinError::GrantBlocksDisabled) => {
                                            warn!("Grant blocks disabled, creating zero-balance entry");
                                            self.node.blockchain.ensure_account(&self.wallet_address);
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Failed to grant initial balance");
                                            self.node.blockchain.ensure_account(&self.wallet_address);
                                        }
                                    }
                                    self.node.blockchain.save_state();
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
                let balance = self.node.blockchain.get_balance(&self.wallet_address);
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
                        let sender_nonce = self.node.blockchain.get_nonce(&self.wallet_address);
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

                        {
                            debug!(?transaction, "Транзакция для добавления");
                            if blockchain.apply_tx(transaction.clone()).is_err() {
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
                            event_bus: Arc::clone(&self.node.event_bus),
                            inbox: self.node.inbox.clone(),
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

/// Sync-точка входа: собственный runtime, блокирует до завершения `run_async()`.
///
/// Существует для утилит и тестов, которым нужен запуск ноды без `#[tokio::main]`.
/// Основной путь — `main.rs`, который вызывает `run_async().await` напрямую.
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("Не удалось создать runtime tokio");
    rt.block_on(run_async());
}

pub async fn run_async() {
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
    let event_bus = Arc::new(events::EventBus::new());
    info!("Каналы майнинга и синхронизации созданы");

    let listen_addr = config.network.listen_addr;
    let port = listen_addr.port();

    // Create shared shutdown signal for graceful shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_ctrlc = Arc::clone(&shutdown);

    let mut node = Node::new(
        listen_addr.to_string(),
        mining_rx,
        port,
        event_bus.clone(),
        config.network_id,
    );
    node.blockchain
        .set_allow_grant_blocks(config.allow_grant_blocks);
    node.start_server(port, sync_tx.clone());
    node.discover_peers();

    // Spawn sync task on the tokio runtime (ADR-0007: polling loop migrated to async)
    let shutdown_sync = Arc::clone(&shutdown);
    let shutdown_sync_node = Arc::clone(&shutdown);
    let node_blockchain = Arc::clone(&node.blockchain);
    let node_peers = Arc::clone(&node.peers);
    let node_address = node.address.clone();
    let node_rate_limiter = node.rate_limiter.clone();
    let node_event_bus = Arc::clone(&node.event_bus);
    // The sync task shares the node's SyncEngine inbox (ADR-0010): pulled
    // candidates are enqueued, not adopted inline.
    let node_inbox = node.inbox.clone();

    let sync_task_handle = tokio::spawn(async move {
        let mut sync_node = Node {
            blockchain: node_blockchain,
            peers: node_peers,
            address: node_address,
            sync_rx: mpsc::channel().1,
            rate_limiter: node_rate_limiter,
            shutdown: shutdown_sync_node,
            listener: Arc::new(Mutex::new(None)),
            sync_thread_handle: Arc::new(Mutex::new(None)),
            event_bus: node_event_bus,
            network_id: config.network_id,
            inbox: node_inbox,
        };
        // Tick at SYNC_TICK for prompt shutdown checks, but only sync every
        // SYNC_TICKS_PER_SYNC ticks to preserve the original ~1s sync period
        // (sync_blockchain performs blocking TCP I/O with a 1s connect timeout).
        let mut ticker = tokio::time::interval(SYNC_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut ticks: u32 = 0;
        loop {
            ticker.tick().await;
            if shutdown_sync.load(Ordering::Relaxed) {
                break;
            }
            ticks += 1;
            if ticks >= SYNC_TICKS_PER_SYNC {
                ticks = 0;
                sync_node.sync_blockchain();
            }
        }
        info!("Sync task stopped");
    });

    // Store sync task handle in node for graceful shutdown
    *node.sync_thread_handle.lock().unwrap() = Some(sync_task_handle);

    #[cfg(feature = "gui")]
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
            event_bus: Arc::clone(&node.event_bus),
            network_id: config.network_id,
            inbox: node.inbox.clone(),
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
    // Headless: keep the mining sender alive (mining thread idles on recv) and
    // retain the sync receiver so adoption events can be drained below.
    #[cfg(not(feature = "gui"))]
    let (_headless_mining_tx, sync_rx) = (mining_tx, sync_rx);

    // Graceful shutdown — async primary handler (ADR-0007).
    // The AtomicBool is the single shutdown signal shared with the legacy threads;
    // they keep polling it exactly as before, so P17 behaviour is unchanged.
    let shutdown_signal = Arc::clone(&shutdown);
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!(error = %e, "Не удалось установить async-обработчик завершения");
            return;
        }
        info!("Shutdown signal received, initiating graceful shutdown...");
        shutdown_signal.store(true, Ordering::Relaxed);

        // Wait out the same 30s budget the ctrlc fallback grants to mining/sync (P17).
        // The legacy threads observe the AtomicBool and unwind on their own; reaching
        // this point means they did not finish in time.
        tokio::time::sleep(SHUTDOWN_TIMEOUT).await;

        error!("Shutdown timeout exceeded, forcing exit");
        info!("Graceful shutdown complete, exiting");
        std::process::exit(0);
    });

    // Fallback handler: `ctrlc` covers console-detach edge cases on Windows that
    // tokio::signal does not. Both handlers write the same AtomicBool, so whichever
    // fires first initiates shutdown and the other is terminated by process::exit.
    ctrlc::set_handler(move || {
        info!("Shutdown signal received (ctrlc fallback), initiating graceful shutdown...");
        shutdown_ctrlc.store(true, Ordering::Relaxed);

        // Wait for mining and sync threads to finish
        let shutdown_start = Instant::now();
        while shutdown_start.elapsed() < SHUTDOWN_TIMEOUT {
            std::thread::sleep(SYNC_TICK);
            // The Drop impls will handle cleanup when node goes out of scope
        }

        error!("Shutdown timeout exceeded, forcing exit");
        info!("Graceful shutdown complete, exiting");
        std::process::exit(0);
    })
    .expect("Ошибка установки обработчика завершения");

    // eframe::run_native is blocking and owns a windowing event loop, so it runs on
    // a dedicated thread from tokio's blocking pool (ADR-0007) rather than occupying
    // a runtime worker.
    #[cfg(feature = "gui")]
    {
        tokio::task::spawn_blocking(move || {
            eframe::run_native(
                "Blockchain Wallet",
                eframe::NativeOptions::default(),
                Box::new(|_cc| Box::new(app)),
            )
            .expect("Ошибка запуска приложения");
        })
        .await
        .expect("GUI task panicked");
    }
    // Headless mode: no GUI event loop; keep the node running until the shared
    // shutdown flag flips (ctrlc / tokio signal handlers above), draining the
    // UI sync channel so adoption events do not accumulate unbounded.
    #[cfg(not(feature = "gui"))]
    {
        info!("Headless mode (gui feature off): running until shutdown signal");
        while !shutdown.load(Ordering::Relaxed) {
            while sync_rx.try_recv().is_ok() {}
            tokio::time::sleep(SYNC_TICK).await;
        }
        info!("Headless mode: shutdown signal received, exiting");
    }
}

pub mod test_support {
    use super::*;
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
        let dir = TestDir::new(tag);
        dir.0.clone()
    }

    pub fn random_port() -> u16 {
        use std::net::TcpListener;
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// LevelDB path that `BlockchainFacade::new(port)` opens (open-path DB,
    /// not the TestDir scheme). Exposed so tests can seed a legacy DB before
    /// triggering the DB-open migrations (BUG-S0-017).
    pub fn blockchain_db_path_for_port(port: u16) -> PathBuf {
        crate::blockchain::state_cache::db_path_for_port(port)
    }

    pub fn create_test_blockchain(db_path: &Path) -> BlockchainFacade {
        fs::create_dir_all(db_path).expect("Failed to create test DB directory");
        let storage = crate::storage::Storage::new(db_path).expect("Failed to open test DB");
        let mut bc = Blockchain {
            chain: vec![],
            balances: crate::blockchain::state_cache::StateCache::new(),
            difficulty: 0,
            mempool: crate::mempool::Mempool::new(),
            storage,
            allow_grant_blocks: true,
            total_work: [0, 0, 0, 0],
            rules: crate::blockchain::consensus_manager::ConsensusManager::new(),
        };
        bc.create_genesis_block();
        BlockchainFacade::from_blockchain(bc)
    }

    pub fn mine_current(blockchain: &BlockchainFacade) {
        let (progress_tx, _progress_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let block = blockchain.mine_block(progress_tx, &shutdown);
        assert!(block.is_some(), "Mining current transaction failed");
    }

    pub fn sync_to_longest(wallets: &[Arc<BlockchainFacade>]) {
        use crate::blockchain::chain_selector::ChainSelector;

        let best = wallets
            .iter()
            .filter_map(|w| w.chain_info().map(|info| (w.clone(), info)))
            .reduce(|(best_w, best_info), (w, info)| {
                if ChainSelector::is_better(&info, &best_info) {
                    (w, info)
                } else {
                    (best_w, best_info)
                }
            });
        let Some((best_w, _)) = best else {
            return;
        };
        let (chain, balances, mempool_txs, difficulty) = best_w.with_inner(|bc| {
            (
                bc.chain.clone(),
                bc.balances.clone(),
                bc.mempool.transactions(),
                bc.difficulty,
            )
        });
        for w in wallets {
            if Arc::ptr_eq(w, &best_w) {
                continue;
            }
            let _ = w.adopt_candidate(
                chain.clone(),
                Some(balances.accounts().clone()),
                mempool_txs.clone(),
                difficulty,
            );
        }
    }

    pub fn assert_balances(wallets: &[Arc<BlockchainFacade>], addrs: &[String], expected: &[u64]) {
        for (i, w) in wallets.iter().enumerate() {
            assert_eq!(
                w.get_balance(&addrs[i]),
                expected[i],
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

    pub fn adopt_from(target: &Arc<BlockchainFacade>, source: &Arc<BlockchainFacade>) -> bool {
        let (chain, balances, mempool_txs, difficulty) = source.with_inner(|bc| {
            (
                bc.chain.clone(),
                bc.balances.clone(),
                bc.mempool.transactions(),
                bc.difficulty,
            )
        });
        target
            .adopt_candidate(
                chain,
                Some(balances.accounts().clone()),
                mempool_txs,
                difficulty,
            )
            .unwrap_or(false)
    }

    pub fn create_node_for_test(
        bc: &Arc<BlockchainFacade>,
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
            // Один bus на node и facade — как в проде: RBF-замена публикует
            // TxRejected из facade, а майнинг-задача — из node.
            event_bus: bc.event_bus(),
            network_id: crate::consensus::CHAIN_ID_REGTEST,
            // Spawned by `start_server` when the test starts a server.
            inbox: None,
        };
        (node, peers)
    }

    pub fn create_sync_node(
        bc: &Arc<BlockchainFacade>,
        peers: &Arc<Mutex<Vec<String>>>,
        address: &str,
        rate_limiter: &Arc<crate::network::RateLimiter>,
        inbox: Option<Inbox>,
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
            event_bus: bc.event_bus(),
            network_id: crate::consensus::CHAIN_ID_REGTEST,
            inbox,
        }
    }

    pub fn sign_transaction(tx: &mut Transaction, secret_key: &SecretKey) {
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
        let addr = crate::address::encode_address(&pk, crate::consensus::current_chain_id())
            .expect("Failed to encode address for current chain");
        (addr, sk)
    }

    pub fn generate_keypairs(n: usize) -> Vec<(String, SecretKey)> {
        (0..n).map(|_| generate_keypair()).collect()
    }

    pub fn create_and_mine_tx(
        bc: &BlockchainFacade,
        sender_addr: &str,
        receiver_addr: &str,
        amount: u64,
        secret_key: &SecretKey,
    ) {
        let sender_nonce = bc.get_nonce(sender_addr);
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
        assert!(bc.apply_tx(tx).is_ok(), "Transaction rejected");
        mine_current(bc);
    }
}

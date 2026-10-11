use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info, warn};

use crate::network::sync::SYNC_IO_TIMEOUT;
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
    pub sync_rx: tokio::sync::mpsc::UnboundedReceiver<ChainSnapshot>,
    pub rate_limiter: Arc<crate::network::RateLimiter>,
    pub shutdown: Arc<AtomicBool>,
    /// Accept-loop task handle; aborted on Drop (ADR-0011).
    pub accept_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Sync-polling task handle; aborted on Drop.
    pub sync_task_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub event_bus: Arc<events::EventBus>,
    pub network_id: u32,
    /// SyncEngine inbox, spawned by `start_server` (ADR-0010). `None` until
    /// the server starts; the sync task and mining inherit it from here.
    pub inbox: Option<Inbox>,
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
        mining_rx: tokio::sync::mpsc::UnboundedReceiver<MiningTask>,
        port: u16,
        event_bus: Arc<events::EventBus>,
        network_id: u32,
    ) -> Self {
        let blockchain = Arc::new(BlockchainFacade::with_event_bus(
            port,
            event_bus.clone(),
            network_id,
        ));
        let peers = Arc::new(Mutex::new(vec![]));
        let rate_limiter = Arc::new(crate::network::RateLimiter::new(10, 100));
        let shutdown = Arc::new(AtomicBool::new(false));
        let node = Node {
            blockchain: blockchain.clone(),
            peers: peers.clone(),
            address: address.clone(),
            sync_rx: tokio::sync::mpsc::unbounded_channel().1,
            rate_limiter: rate_limiter.clone(),
            shutdown: shutdown.clone(),
            accept_task: Arc::new(Mutex::new(None)),
            sync_task_handle: Arc::new(Mutex::new(None)),
            event_bus: event_bus.clone(),
            network_id,
            // Spawned by `start_server`.
            inbox: None,
        };
        // ADR-0011: the mining worker is a tokio task. It is spawned with the
        // same dual-mode rule as `sync_engine::spawn`: on a runtime it becomes
        // a task; without one (plain `#[test]` threads) it runs on a dedicated
        // thread with its own current-thread runtime.
        spawn_mining_worker(
            mining_rx,
            peers.clone(),
            address.clone(),
            network_id,
        );
        node
    }

    pub fn discover_peers(&mut self) {
        let start_time = SystemTime::now();
        // ADR-0011: file I/O deliberately outside the peers lock.
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
        let discovered: Vec<String> = network_config["peers"]
            .as_array()
            .unwrap_or(&empty_peers)
            .iter()
            .filter_map(|peer| peer.as_str())
            .filter(|peer_str| {
                peer_str
                    .split(':')
                    .next_back()
                    .unwrap_or("0")
                    .parse::<u16>()
                    .unwrap_or(0)
                    != own_port
            })
            .map(|peer_str| peer_str.to_string())
            .collect();
        let mut peers = self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers");
        peers.clear();
        peers.extend(discovered);
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        debug!(duration_secs = duration, peers = ?*peers, "Обнаружение пиров завершено");
    }

    pub fn add_peer(&mut self, address: String) -> bool {
        let start_time = SystemTime::now();
        if self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers")
            .contains(&address)
        {
            debug!(peer = %address, "Пир уже существует, добавление не требуется");
            return false;
        }
        // ADR-0011: file I/O deliberately outside the peers lock.
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
        let mut peers = self
            .peers
            .lock()
            .expect("Не удалось захватить Mutex для peers");
        if peers.contains(&address) {
            return false;
        }
        peers.push(address.clone());
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

    /// Start the P2P server (ADR-0011: async accept loop + per-connection
    /// tokio tasks; replaces the legacy thread-per-connection model).
    pub async fn start_server(
        &mut self,
        port: u16,
        sync_tx: UnboundedSender<ChainSnapshot>,
    ) {
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
        let listener = TcpListener::bind(&address)
            .await
            .expect("Не удалось запустить сервер");
        let accept_task = tokio::spawn(async move {
            loop {
                if shutdown.load(Ordering::Relaxed) {
                    info!("Shutdown signal received, stopping server");
                    break;
                }
                // Accept with a tick-bounded timeout so the shutdown flag is
                // observed even when no connection arrives (the legacy
                // listener.incoming() only woke on a connection).
                let accepted =
                    match tokio::time::timeout(SYNC_TICK, listener.accept()).await {
                        Ok(result) => result,
                        Err(_) => continue,
                    };
                match accepted {
                    Ok((stream, peer_addr)) => {
                        let blockchain = Arc::clone(&blockchain);
                        let inbox = inbox.clone();
                        let rate_limiter = Arc::clone(&rate_limiter);
                        let shutdown = Arc::clone(&shutdown);
                        tokio::spawn(handle_connection(
                            stream,
                            peer_addr,
                            blockchain,
                            inbox,
                            rate_limiter,
                            shutdown,
                            network_id,
                        ));
                    }
                    Err(e) => error!(error = %e, "Ошибка обработки входящего соединения"),
                }
            }
        });
        *self.accept_task.lock().unwrap() = Some(accept_task);
        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(port, duration_secs = duration, "Сервер запущен");
    }
    /// One outgoing sync round against all known peers (ADR-0011: async —
    /// gossip push, headers-first and the legacy fallback all await instead
    /// of blocking a thread or runtime worker).
    pub async fn sync_blockchain(&mut self) {
        let start_time = SystemTime::now();
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let shutdown = Arc::clone(&self.shutdown);
        let inbox = self.inbox.clone();
        let network_id = self.network_id;
        let blockchain = Arc::clone(&self.blockchain);
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
            let current_chain_length = blockchain.chain_len();
            debug!(current_chain_length, "Текущая длина chain");
            if current_chain_length <= 1 {
                info!("Новый узел, только получение данных, отправка цепочки запрещена");
            } else {
                // Отправка UPDATE_BLOCKCHAIN with HELLO handshake
                let gossip = tokio::time::timeout(
                    Duration::from_secs(1),
                    tokio::net::TcpStream::connect(&addr),
                )
                .await;
                if let Ok(Ok(mut stream)) = gossip {
                    // Send HELLO handshake first
                    let hello_data = crate::network::protocol::encode_hello(network_id);
                    if stream.write_all(&hello_data).await.is_err() {
                        warn!(peer = %peer, "Failed to send HELLO handshake");
                        continue;
                    }

                    // Send UPDATE_BLOCKCHAIN
                    let response = blockchain.to_wire_json();
                    let message = format!("UPDATE_BLOCKCHAIN:{}", response);
                    let mut data = (message.len() as u32).to_be_bytes().to_vec();
                    data.extend_from_slice(message.as_bytes());
                    if stream.write_all(&data).await.is_ok() {
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
            let local_chain = blockchain.chain_snapshot();
            match crate::network::sync::sync_headers_first(
                addr,
                network_id,
                &local_chain,
                &shutdown,
            )
            .await
            {
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
                                    difficulty: blockchain.difficulty(),
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
            let legacy = tokio::time::timeout(
                Duration::from_secs(1),
                tokio::net::TcpStream::connect(&addr),
            )
            .await;
            if let Ok(Ok(mut stream)) = legacy {
                // Send HELLO handshake first
                let hello_data = crate::network::protocol::encode_hello(network_id);
                if stream.write_all(&hello_data).await.is_err() {
                    warn!(peer = %peer, "Failed to send HELLO handshake");
                    continue;
                }

                let message = "GET_BLOCKCHAIN";
                let mut data = (message.len() as u32).to_be_bytes().to_vec();
                data.extend_from_slice(message.as_bytes());
                if stream.write_all(&data).await.is_ok() {
                    // The legacy path had no read timeout at all; ADR-0011
                    // bounds it with the same SYNC_IO_TIMEOUT as headers-first.
                    let response_bytes = match tokio::time::timeout(
                        SYNC_IO_TIMEOUT,
                        crate::network::protocol::read_length_prefixed_async(&mut stream),
                    )
                    .await
                    {
                        Ok(Ok(bytes)) => bytes,
                        Ok(Err(e)) => {
                            warn!(peer = %peer, error = %e, "Failed to read length-prefixed response");
                            continue;
                        }
                        Err(_) => {
                            warn!(peer = %peer, "Timeout reading GET_BLOCKCHAIN response");
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

        // Abort the accept-loop task (ADR-0011: replaces the stored-listener
        // close; the loop also polls the flag every SYNC_TICK).
        if let Ok(mut handle) = self.accept_task.lock() {
            if let Some(h) = handle.take() {
                h.abort();
                info!("Accept task aborted");
            }
        }

        // Cancel the sync task (tokio cancellation is abort-based, not blocking)
        if let Ok(mut handle) = self.sync_task_handle.lock() {
            if let Some(h) = handle.take() {
                h.abort();
                info!("Sync task aborted");
            }
        }

        info!("Network node shutdown complete");
    }
}

// ---------------------------------------------------- connection handling

/// Serve one inbound peer connection (ADR-0011: runs as a tokio task; the
/// legacy version was a dedicated `std::thread` per TCP stream).
///
/// Read-serving only: headers/blocks/json requests touch the facade through
/// read-only calls; `UPDATE_BLOCKCHAIN:` is parsed and enqueued into the
/// SyncEngine inbox (ADR-0010) — adoption happens in the engine.
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    blockchain: Arc<BlockchainFacade>,
    inbox: Inbox,
    rate_limiter: Arc<crate::network::RateLimiter>,
    shutdown: Arc<AtomicBool>,
    network_id: u32,
) {
    if let Err(e) = rate_limiter.check(peer_addr) {
        warn!(peer = %peer_addr, error = %e, "Rate limit exceeded, closing connection");
        return;
    }

    // First message must be HELLO handshake (bounded by SYNC_IO_TIMEOUT).
    let hello_bytes = match tokio::time::timeout(
        SYNC_IO_TIMEOUT,
        crate::network::protocol::read_length_prefixed_async(&mut stream),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            warn!(error = %e, "Failed to read HELLO handshake");
            return;
        }
        Err(_) => {
            warn!(peer = %peer_addr, "Timeout reading HELLO handshake");
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
            peer = %peer_addr,
            expected = network_id,
            got = peer_network_id,
            "Peer rejected: foreign network_id"
        );
        rate_limiter.ban(peer_addr);
        return;
    }

    debug!(peer_network_id, "HELLO handshake successful");

    use crate::network::protocol as proto;

    // S1-P16: one connection serves many requests — a headers-first client
    // iterates GET_HEADERS / GET_BLOCKS on a single session. The read
    // timeout bounds idle handlers; a client that just closes surfaces as
    // EOF here.
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let request_bytes = match tokio::time::timeout(
            SYNC_IO_TIMEOUT,
            proto::read_length_prefixed_async(&mut stream),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => {
                debug!(error = %e, "Соединение закрыто после HELLO");
                break;
            }
            Err(_) => {
                debug!(peer = %peer_addr, "Timeout ожидания запроса после HELLO");
                break;
            }
        };

        // Binary requests (S1-P16) — checked before the UTF-8 text path;
        // text messages start with ASCII letters, tags with 0x01..=0x04.
        match request_bytes.first() {
            Some(&proto::MSG_GET_HEADERS) => {
                let payload = match proto::parse_get_headers(&request_bytes) {
                    Ok(from_height) => {
                        let headers =
                            blockchain.headers_from_height(from_height, proto::MAX_HEADERS_BATCH);
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
                            proto::write_length_prefixed_async(&mut stream, &payload).await
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
                let payload = match proto::parse_get_blocks(&request_bytes) {
                    Ok(hashes) => {
                        let blocks =
                            blockchain.blocks_by_hashes(&hashes, proto::MAX_BLOCKS_BATCH);
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
                            proto::write_length_prefixed_async(&mut stream, &payload).await
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
            let mut data = (response.len() as u32).to_be_bytes().to_vec();
            data.extend_from_slice(response.as_bytes());
            if stream.write_all(&data).await.is_ok() {
                info!("Отправлен блокчейн клиенту");
            }
        } else if request.starts_with("UPDATE_BLOCKCHAIN:") {
            let blockchain_data = request.strip_prefix("UPDATE_BLOCKCHAIN:").unwrap_or("");
            let temp_blockchain: BlockchainDeserialize = match serde_json::from_str(blockchain_data)
            {
                Ok(data) => data,
                Err(e) => {
                    error!(error = %e, "Ошибка десериализации данных блокчейна");
                    return;
                }
            };
            // ADR-0010: parse and enqueue only — the SyncEngine validates
            // and adopts.
            inbox.push_inbound(Incoming::CandidateChain {
                chain: temp_blockchain.chain,
                balances: Some(temp_blockchain.balances),
                mempool_txs: temp_blockchain.mempool_txs,
                pending_transactions: temp_blockchain.pending_transactions,
                difficulty: temp_blockchain.difficulty,
                from: Some(peer_addr),
            });
        }
    }
}

// ---------------------------------------------------------- mining worker

/// Spawn the mining worker with the same dual-mode rule as
/// `sync_engine::spawn` (ADR-0011): on a tokio runtime the worker is a task;
/// without one (plain `#[test]` threads) it runs on a dedicated thread with
/// its own current-thread runtime.
fn spawn_mining_worker(
    mining_rx: tokio::sync::mpsc::UnboundedReceiver<MiningTask>,
    peers: Arc<Mutex<Vec<String>>>,
    address: String,
    network_id: u32,
) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(mining_worker_loop(mining_rx, peers, address, network_id));
        }
        Err(_) => {
            std::thread::Builder::new()
                .name("mining-worker".into())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("mining worker runtime");
                    rt.block_on(mining_worker_loop(mining_rx, peers, address, network_id));
                })
                .expect("mining worker thread");
        }
    }
}

/// The mining worker loop (ADR-0011). CPU-bound `apply_tx` + `mine_block`
/// (which holds the facade write lock for the whole PoW search) run inside
/// `spawn_blocking` so runtime workers are never blocked; status updates and
/// the post-mine gossip round await between tasks.
async fn mining_worker_loop(
    mut mining_rx: tokio::sync::mpsc::UnboundedReceiver<MiningTask>,
    peers: Arc<Mutex<Vec<String>>>,
    address: String,
    network_id: u32,
) {
    info!("Фоновый воркер майнинга запущен (ADR-0011)");
    let mut mining_count = 0u64;
    let mut total_duration = 0.0;
    let mut successful_mining = 0u64;
    while let Some(task) = mining_rx.recv().await {
        mining_count += 1;
        info!(mining_count, "Получена задача майнинга");
        task.event_bus.publish(events::NodeEvent::MiningStarted);
        let progress_tx_clone = task.progress_tx.clone();
        let start_time = SystemTime::now();

        let MiningTask {
            blockchain,
            transaction,
            mining_status,
            progress_tx,
            status_tx,
            rate_limiter,
            shutdown,
            event_bus,
            inbox,
        } = task;

        // apply_tx + mine_block are lock- and CPU-bound: isolate them on the
        // blocking pool (ADR-0011). catch_unwind preserves the legacy
        // panic-isolation semantics of the mining thread.
        let blockchain_bc = blockchain.clone();
        let transaction_bc = transaction.clone();
        let shutdown_bc = shutdown.clone();
        let event_bus_bc = event_bus.clone();
        let result = tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                debug!(mining_count, ?transaction_bc, "Проверка транзакции");
                match blockchain_bc.apply_tx(transaction_bc.clone()) {
                    Ok(_) => {
                        info!(
                            mining_count,
                            "Транзакция успешно добавлена, начало майнинга"
                        );
                        event_bus_bc.publish(events::NodeEvent::TxAccepted {
                            txid: hex::encode(strangecoin_core::serialize::txid(
                                &transaction_bc,
                            )),
                        });
                        blockchain_bc.mine_block(progress_tx_clone, &shutdown_bc)
                    }
                    Err(e) => {
                        warn!(mining_count, error = %e, "Транзакция отклонена, попытка майнить существующие транзакции");
                        event_bus_bc.publish(events::NodeEvent::TxRejected {
                            txid: hex::encode(strangecoin_core::serialize::txid(
                                &transaction_bc,
                            )),
                            reason: format!("{}", e),
                        });
                        if !blockchain_bc.mempool_is_empty() {
                            blockchain_bc.mine_block(progress_tx_clone, &shutdown_bc)
                        } else {
                            let _ = progress_tx_clone.send(format!(
                                "Ошибка: Нет транзакций для майнинга в задаче {}",
                                mining_count
                            ));
                            warn!(mining_count, "Нет транзакций для майнинга");
                            None
                        }
                    }
                }
            }))
        })
        .await;
        let result = match result {
            Ok(caught) => match caught {
                Ok(result) => result,
                Err(panic) => {
                    let err_msg = match panic.downcast_ref::<&str>() {
                        Some(s) => s.to_string(),
                        None => format!("Неизвестная паника: {:?}", panic),
                    };
                    error!(mining_count, error = %err_msg, "Паника в воркере майнинга");
                    let _ = progress_tx.send(format!(
                        "Паника в воркере майнинга {}: {}",
                        mining_count, err_msg
                    ));
                    None
                }
            },
            Err(join_err) => {
                error!(mining_count, error = %join_err, "spawn_blocking воркера майнинга завершился ошибкой");
                None
            }
        };

        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        total_duration += duration;
        info!(mining_count, duration_secs = duration, result = ?result, "Майнинг завершен");
        event_bus.publish(events::NodeEvent::MiningFinished);

        // Post-mine side effects, lock-free (ADR-0011): gossip round with the
        // node's own network_id — the legacy temp Node hard-coded
        // CHAIN_ID_REGTEST, which sent gossip with the wrong network_id on
        // non-regtest networks (BUG-S0-021). The temp Node carries a fresh
        // shutdown flag so its Drop cannot trip the real node's shutdown.
        if let Some(block) = &result {
            successful_mining += 1;
            info!(mining_count, ?block, "Майнинг успешен, блок добавлен");
            let _ = status_tx.send(format!(
                "Транзакция отправлена, блок добавлен: {:?}",
                block
            ));
            let mut sync_node = Node {
                blockchain: blockchain.clone(),
                peers: peers.clone(),
                address: address.clone(),
                sync_rx: tokio::sync::mpsc::unbounded_channel().1,
                rate_limiter: rate_limiter.clone(),
                shutdown: Arc::new(AtomicBool::new(false)),
                accept_task: Arc::new(Mutex::new(None)),
                sync_task_handle: Arc::new(Mutex::new(None)),
                event_bus: event_bus.clone(),
                network_id,
                inbox: inbox.clone(),
            };
            sync_node.sync_blockchain().await;
            event_bus.publish(events::NodeEvent::BlockApplied {
                height: block.index,
                hash: block.hash.clone(),
            });
        }

        // Update the mining status (short critical section; the GUI polls it
        // via try_lock). Retry with an async sleep — the guard (including
        // the one TryLockError::Poisoned may carry) is dropped before the
        // await so the future stays Send.
        enum StatusUpdate {
            Updated,
            Busy,
        }
        let mut attempts = 0;
        let max_attempts = 5;
        let mut status_updated = false;
        while attempts < max_attempts {
            let outcome = match mining_status.try_lock() {
                Ok(mut mining_status_guard) => {
                    *mining_status_guard = match &result {
                        Some(block) => MiningStatus::Completed(Some(block.clone())),
                        None => {
                            warn!(mining_count, "Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут");
                            let _ = status_tx.send("Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут".to_string());
                            MiningStatus::Failed("Майнинг не удался: нет транзакций или превышен лимит итераций/таймаут".to_string())
                        }
                    };
                    debug!(mining_count, ?mining_status_guard, "Статус майнинга обновлён");
                    StatusUpdate::Updated
                }
                Err(_) => StatusUpdate::Busy,
            };
            match outcome {
                StatusUpdate::Updated => {
                    status_updated = true;
                    break;
                }
                StatusUpdate::Busy => {
                    attempts += 1;
                    debug!(
                        attempts,
                        mining_count, "Попытка обновить статус майнинга не удалась"
                    );
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
        if !status_updated {
            warn!(
                mining_count,
                max_attempts, "Не удалось обновить статус майнинга после попыток"
            );
            let _ = progress_tx.send(format!(
                "Ошибка: Не удалось обновить статус майнинга {} после {} попыток",
                mining_count, max_attempts
            ));
            let _ = status_tx.send(format!(
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
    info!("Фоновый воркер майнинга завершен");
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
            allow_zero_state_root: None,
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

    let (mining_tx, mining_rx) = tokio::sync::mpsc::unbounded_channel();
    let (sync_tx, sync_rx) = tokio::sync::mpsc::unbounded_channel();
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
    node.blockchain
        .set_allow_zero_state_root(config.zero_state_root_allowed());
    node.start_server(port, sync_tx.clone()).await;
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
            sync_rx: tokio::sync::mpsc::unbounded_channel().1,
            rate_limiter: node_rate_limiter,
            shutdown: shutdown_sync_node,
            accept_task: Arc::new(Mutex::new(None)),
            sync_task_handle: Arc::new(Mutex::new(None)),
            event_bus: node_event_bus,
            network_id: config.network_id,
            inbox: node_inbox,
        };
        // Tick at SYNC_TICK for prompt shutdown checks, but only sync every
        // SYNC_TICKS_PER_SYNC ticks to preserve the original ~1s sync period.
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
                sync_node.sync_blockchain().await;
            }
        }
        info!("Sync task stopped");
    });

    // Store sync task handle in node for graceful shutdown
    *node.sync_task_handle.lock().unwrap() = Some(sync_task_handle);

    #[cfg(feature = "gui")]
    let app = gui::WalletApp {
        node: Node {
            blockchain: Arc::clone(&node.blockchain),
            peers: Arc::clone(&node.peers),
            address: node.address.clone(),
            sync_rx,
            rate_limiter: node.rate_limiter.clone(),
            shutdown: Arc::clone(&shutdown),
            accept_task: Arc::new(Mutex::new(None)),
            sync_task_handle: Arc::new(Mutex::new(None)),
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
    // Headless: keep the mining sender alive (the mining worker idles on recv) and
    // retain the sync receiver so adoption events can be drained below.
    #[cfg(not(feature = "gui"))]
    let (_headless_mining_tx, mut sync_rx) = (mining_tx, sync_rx);

    // Graceful shutdown — async primary handler (ADR-0007).
    // The AtomicBool is the single shutdown signal shared with the async tasks
    // and blocking-pool work; they keep polling it exactly as before, so P17
    // behaviour is unchanged.
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

    // GUI event loop (eframe) is blocking and runs on tokio's blocking pool
    // inside gui::run (ADR-0007) rather than occupying a runtime worker.
    #[cfg(feature = "gui")]
    gui::run(app).await;
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

    /// Test blockchain on `network_id` (BUG-S1-004): regtest by default;
    /// mainnet/testnet modes exercise the configured chain's rules.
    pub fn create_test_blockchain_for_network(
        db_path: &Path,
        network_id: u32,
    ) -> BlockchainFacade {
        fs::create_dir_all(db_path).expect("Failed to create test DB directory");
        let storage = crate::storage::Storage::new(db_path).expect("Failed to open test DB");
        let mut bc = Blockchain {
            chain: vec![],
            balances: crate::blockchain::state_cache::StateCache::new(),
            difficulty: 0,
            mempool: crate::mempool::Mempool::new(network_id),
            storage,
            allow_grant_blocks: true,
            allow_zero_state_root: true,
            total_work: [0, 0, 0, 0],
            rules: crate::blockchain::consensus_manager::ConsensusManager::new(),
            chain_id: network_id,
        };
        bc.create_genesis_block();
        BlockchainFacade::from_blockchain(bc)
    }

    pub fn create_test_blockchain(db_path: &Path) -> BlockchainFacade {
        create_test_blockchain_for_network(db_path, crate::consensus::CHAIN_ID_REGTEST)
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
            sync_rx: tokio::sync::mpsc::unbounded_channel().1,
            rate_limiter: Arc::new(crate::network::RateLimiter::new(10, 100)),
            shutdown: Arc::new(AtomicBool::new(false)),
            accept_task: Arc::new(Mutex::new(None)),
            sync_task_handle: Arc::new(Mutex::new(None)),
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
            sync_rx: tokio::sync::mpsc::unbounded_channel().1,
            rate_limiter: Arc::clone(rate_limiter),
            shutdown: Arc::new(AtomicBool::new(false)),
            accept_task: Arc::new(Mutex::new(None)),
            sync_task_handle: Arc::new(Mutex::new(None)),
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
        let addr = crate::address::encode_address(&pk, crate::consensus::CHAIN_ID_REGTEST)
            .expect("Failed to encode address for regtest");
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
            chain_id: bc.chain_id(),
            signature: Vec::new(),
            is_coinbase: false,
        };
        sign_transaction(&mut tx, secret_key);
        assert!(bc.apply_tx(tx).is_ok(), "Transaction rejected");
        mine_current(bc);
    }
}

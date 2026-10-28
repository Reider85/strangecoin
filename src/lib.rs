use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime};
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info, warn, Instrument};

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
/// S2-P19: outbound dial cadence — try to establish long-lived gossip
/// sessions with known peers.
pub const DIAL_INTERVAL: Duration = Duration::from_secs(30);
/// S2-P19: peer-store maintenance cadence (ban expiry, score re-check —
/// ADR-0022 recommends every 60 s).
pub const PEER_TICK_INTERVAL: Duration = Duration::from_secs(60);
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
    /// S2-P19: connection orchestration (Noise accept/dial, bans, scoring).
    pub peer_manager: Arc<strangecoin_net::PeerManager>,
    /// S2-P19: per-peer gossip known-sets shared by the announcer and the
    /// connection loops.
    pub gossip_state: Arc<Mutex<strangecoin_net::GossipState>>,
    /// S2-P19: shared Erlay reconciliation state (known-tx set + triggers).
    pub reconciliation_state: Arc<Mutex<strangecoin_net::ReconciliationState>>,
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
        rate_limit_config: crate::network::RateLimitConfig,
        peer_manager: Arc<strangecoin_net::PeerManager>,
    ) -> Self {
        let blockchain = Arc::new(BlockchainFacade::with_event_bus(
            port,
            event_bus.clone(),
            network_id,
        ));
        let peers = Arc::new(Mutex::new(vec![]));
        let rate_limiter = Arc::new(crate::network::RateLimiter::new(rate_limit_config));
        let shutdown = Arc::new(AtomicBool::new(false));
        let gossip_state = Arc::new(Mutex::new(strangecoin_net::GossipState::new()));
        let reconciliation_state = Arc::new(Mutex::new(
            strangecoin_net::ReconciliationState::with_defaults(),
        ));
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
            peer_manager: peer_manager.clone(),
            gossip_state: gossip_state.clone(),
            reconciliation_state: reconciliation_state.clone(),
        };
        // ADR-0011: the mining worker is a tokio task. It is spawned with the
        // same dual-mode rule as the old `sync_engine::spawn`: on a runtime it
        // becomes a task; without one (plain `#[test]` threads) it runs on a
        // dedicated thread with its own current-thread runtime.
        spawn_mining_worker(
            mining_rx,
            peers.clone(),
            address.clone(),
            network_id,
            peer_manager,
            gossip_state,
            reconciliation_state,
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
    pub async fn start_server(&mut self, port: u16, sync_tx: UnboundedSender<ChainSnapshot>) {
        let start_time = SystemTime::now();
        // ADR-0010: the engine spawned here is the only adopter of incoming
        // data; the sync loop enqueues candidates into its inbox (the
        // inbound handler serves read-only requests and does not enqueue —
        // S2-P18 removed the legacy `UPDATE_BLOCKCHAIN` path that did).
        if self.inbox.is_none() {
            self.inbox = Some(crate::network::sync_engine::spawn(
                // Plain `clone()` (not `Arc::clone(&…)`): the spawn signature
                // takes `Arc<dyn BlockchainView>` / `Arc<dyn EventSink>`, and
                // the unsized coercion happens on the owned value at the
                // argument position, not through the `&Arc<T>` reference.
                self.blockchain.clone(),
                self.event_bus.clone(),
                sync_tx.clone(),
                Arc::clone(&self.shutdown),
                Arc::clone(&self.rate_limiter),
            ));
        }
        let blockchain = Arc::clone(&self.blockchain);
        let shutdown = Arc::clone(&self.shutdown);
        let peer_manager = Arc::clone(&self.peer_manager);
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let gossip_state = Arc::clone(&self.gossip_state);
        let reconciliation_state = Arc::clone(&self.reconciliation_state);
        let inbox = self
            .inbox
            .clone()
            .expect("SyncEngine inbox must be spawned before the server starts");
        let address = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&address)
            .await
            .expect("Не удалось запустить сервер");

        // S2-P19: the accept loop completes the Noise XX responder handshake
        // + encrypted HELLO through `PeerManager::accept_inbound`, then hands
        // the session to a `run_connection` worker (read → rate-limit →
        // dispatch: sync serving, gossip, compact blocks, Erlay).
        let accept_task = tokio::spawn(
            async move {
                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        info!("Shutdown signal received, stopping server");
                        break;
                    }
                    // Accept with a tick-bounded timeout so the shutdown flag
                    // is observed even when no connection arrives.
                    let accepted = match tokio::time::timeout(SYNC_TICK, listener.accept()).await {
                        Ok(result) => result,
                        Err(_) => continue,
                    };
                    match accepted {
                        Ok((stream, peer_addr)) => {
                            let ctx = strangecoin_net::ConnectionContext {
                                peer_manager: Arc::clone(&peer_manager),
                                blockchain: blockchain.clone(),
                                inbox: inbox.clone(),
                                gossip: Arc::clone(&gossip_state),
                                rate_limiter: Arc::clone(&rate_limiter),
                                reconciliation: Arc::clone(&reconciliation_state),
                                shutdown: Arc::clone(&shutdown),
                            };
                            let pm = Arc::clone(&peer_manager);
                            let sd = Arc::clone(&shutdown);
                            tokio::spawn(
                                async move {
                                    if sd.load(Ordering::Relaxed) {
                                        return;
                                    }
                                    match pm.accept_inbound(stream, peer_addr).await {
                                        Ok(conn) => {
                                            strangecoin_net::run_connection(conn, ctx).await;
                                        }
                                        Err(err) => {
                                            debug!(
                                                target = "net::accept",
                                                peer = %peer_addr,
                                                error = %err,
                                                "inbound handshake/admission rejected"
                                            );
                                        }
                                    }
                                }
                                .instrument(tracing::info_span!(
                                    "net::connection",
                                    peer = %peer_addr
                                )),
                            );
                        }
                        Err(e) => error!(error = %e, "Ошибка обработки входящего соединения"),
                    }
                }
            }
            .instrument(tracing::info_span!("net::accept", port)),
        );
        *self.accept_task.lock().unwrap() = Some(accept_task);

        // S2-P19: outbound dial loop — keep long-lived gossip sessions to
        // known peers (peer-store candidates + the legacy network.json list).
        {
            let pm = Arc::clone(&self.peer_manager);
            let sd = Arc::clone(&self.shutdown);
            let chain = Arc::clone(&self.blockchain);
            let rate = Arc::clone(&self.rate_limiter);
            let gossip = Arc::clone(&self.gossip_state);
            let recon = Arc::clone(&self.reconciliation_state);
            let engine_inbox = self
                .inbox
                .clone()
                .expect("SyncEngine inbox must be spawned before the server starts");
            let legacy_peers = Arc::clone(&self.peers);
            let dial_task = tokio::spawn(
                async move {
                    let mut ticker = tokio::time::interval(DIAL_INTERVAL);
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        ticker.tick().await;
                        if sd.load(Ordering::Relaxed) {
                            break;
                        }
                        pm.set_local_height(chain.chain_len().saturating_sub(1) as u64);
                        let mut targets: Vec<(SocketAddr, Option<strangecoin_net::NodeId>)> =
                            Vec::new();
                        for entry in pm.dial_candidates(128) {
                            targets.push((entry.addr, Some(entry.node_id)));
                        }
                        for peer in legacy_peers.lock().expect("peers lock").iter() {
                            if let Ok(addr) = peer.parse::<SocketAddr>() {
                                targets.push((addr, None));
                            }
                        }
                        let connected = pm.connected_peers();
                        for (addr, expected) in targets {
                            if sd.load(Ordering::Relaxed) {
                                break;
                            }
                            if expected.as_ref().is_some_and(|id| connected.contains(id)) {
                                continue;
                            }
                            match pm.connect_to_peer(addr, expected).await {
                                Ok(node_id) => {
                                    let conn = match pm.connection(&node_id) {
                                        Some(conn) => conn,
                                        None => continue,
                                    };
                                    let ctx = strangecoin_net::ConnectionContext {
                                        peer_manager: Arc::clone(&pm),
                                        blockchain: chain.clone(),
                                        inbox: engine_inbox.clone(),
                                        gossip: Arc::clone(&gossip),
                                        rate_limiter: Arc::clone(&rate),
                                        reconciliation: Arc::clone(&recon),
                                        shutdown: Arc::clone(&sd),
                                    };
                                    tokio::spawn(
                                        strangecoin_net::run_connection(conn, ctx).instrument(
                                            tracing::info_span!("net::connection", peer = %node_id),
                                        ),
                                    );
                                }
                                Err(err) => {
                                    debug!(
                                        target: "net::dial",
                                        peer = %addr,
                                        error = %err,
                                        "dial failed (will retry)"
                                    );
                                }
                            }
                        }
                    }
                }
                .instrument(tracing::info_span!("net::dial")),
            );
            // Not stored on the node: the shutdown flag stops the loop; Drop
            // only aborts accept/sync (both were pre-existing handles).
            drop(dial_task);
        }

        // S2-P19: peer-store maintenance — ban expiry + score re-evaluation
        // (ADR-0022).
        {
            let pm = Arc::clone(&self.peer_manager);
            let sd = Arc::clone(&self.shutdown);
            let tick_task = tokio::spawn(
                async move {
                    let mut ticker = tokio::time::interval(PEER_TICK_INTERVAL);
                    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        ticker.tick().await;
                        if sd.load(Ordering::Relaxed) {
                            break;
                        }
                        let report = pm.tick();
                        if report != strangecoin_net::TickReport::default() {
                            debug!(
                                target: "net::peer",
                                expired = report.expired_bans,
                                banned = report.newly_banned,
                                warned = report.warned,
                                "peer tick finished"
                            );
                        }
                    }
                }
                .instrument(tracing::info_span!("net::peer_tick")),
            );
            drop(tick_task);
        }

        // S2-P19: gossip announcer — lifecycle events from the SyncEngine
        // (via the EventBus) become INV / CompactBlock announces to the
        // connected peers.
        {
            let mut events_rx = self.event_bus.subscribe_async(256);
            let pm = Arc::clone(&self.peer_manager);
            let sd = Arc::clone(&self.shutdown);
            let chain = Arc::clone(&self.blockchain);
            let gossip = Arc::clone(&self.gossip_state);
            let recon = Arc::clone(&self.reconciliation_state);
            let announce_task = tokio::spawn(
                async move {
                    let mut shutdown_tick = tokio::time::interval(SYNC_TICK);
                    shutdown_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tokio::select! {
                            biased;
                            _ = shutdown_tick.tick() => {
                                if sd.load(Ordering::Relaxed) {
                                    break;
                                }
                            }
                            event = events_rx.recv() => match event {
                                Some(events::NodeEvent::BlockApplied { hash, .. }) => {
                                    announce_new_block(&hash, &pm, &gossip, &chain);
                                }
                                Some(events::NodeEvent::TxAccepted { txid }) => {
                                    announce_new_tx(&txid, &pm, &gossip, &recon);
                                }
                                Some(_) => {}
                                None => break,
                            }
                        }
                    }
                }
                .instrument(tracing::info_span!("net::announce")),
            );
            drop(announce_task);
        }

        let duration = SystemTime::now()
            .duration_since(start_time)
            .unwrap()
            .as_secs_f64();
        info!(port, duration_secs = duration, "Сервер запущен");
    }
    /// One outgoing sync round against all known peers (ADR-0011: async —
    /// the headers-first download awaits instead of blocking a thread or
    /// runtime worker). S2-P18 removed the v0.8.6 text protocol: there is
    /// no `UPDATE_BLOCKCHAIN` gossip push and no `GET_BLOCKCHAIN` fallback
    /// anymore — headers-first is the only sync path.
    pub async fn sync_blockchain(&mut self) {
        let start_time = SystemTime::now();
        let rate_limiter = Arc::clone(&self.rate_limiter);
        let shutdown = Arc::clone(&self.shutdown);
        let inbox = self.inbox.clone();
        let network_id = self.network_id;
        let blockchain = Arc::clone(&self.blockchain);
        let peer_manager = Arc::clone(&self.peer_manager);
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

            // S1-P16: headers-first sync. Errors mean the peer does not
            // serve the binary protocol (pre-P16 peers never answer) — the
            // peer is then skipped for this round (S2-P18 removed the
            // legacy `GET_BLOCKCHAIN` fallback that would have followed). An
            // enqueued candidate or "nothing better" both end the round for
            // this peer. ADR-0010: the candidate is adopted by the engine.
            // S2-P19: the round runs over a short-lived Noise session (the
            // peer's accept loop is the responder).
            let local_chain = blockchain.chain_snapshot();
            match crate::network::sync::sync_headers_first(
                addr,
                network_id,
                peer_manager.identity(),
                &local_chain,
                &shutdown,
            )
            .await
            {
                Ok(outcome) => {
                    if let Some(candidate) = outcome.candidate {
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
                        "Headers-first недоступен, пропускаем пир (S2-P18: fallback удалён)"
                    );
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

// ---------------------------------------------------------- gossip announce

/// S2-P19 announcer: a new block was applied (SyncEngine → EventBus) —
/// relay it to every connected, non-banned peer.
///
/// Peers that negotiated `Capability::CompactBlocks` get the BIP 152
/// compact form (S2-P14 live wiring); everyone else gets a classic one-item
/// `INV` and pulls the body through `GET_DATA`. Per-peer dedup via
/// [`GossipState`] — a peer that already announced or was served the object
/// is skipped.
fn announce_new_block(
    hash: &str,
    peer_manager: &Arc<strangecoin_net::PeerManager>,
    gossip_state: &Arc<Mutex<strangecoin_net::GossipState>>,
    blockchain: &Arc<BlockchainFacade>,
) {
    use strangecoin_net::protocol::hello::Capability;
    use strangecoin_net::{CompactBlock, Inv, InventoryItem};

    let Some(block) = blockchain.block_by_hash(hash) else {
        debug!(target: "net::announce", hash, "applied block not in canonical chain — skip announce");
        return;
    };
    let hash_bytes: [u8; 32] = match hex::decode(hash)
        .map_err(|e| e.to_string())
        .and_then(|bytes| bytes.try_into().map_err(|_| "hash length".to_string()))
    {
        Ok(bytes) => bytes,
        Err(err) => {
            warn!(target: "net::announce", hash, error = %err, "non-hex block hash — skip announce");
            return;
        }
    };
    let item = InventoryItem::block(hash_bytes);
    let mut gossip = gossip_state.lock().expect("gossip state poisoned");
    for peer in peer_manager.connected_peers() {
        if peer_manager.is_banned(&peer) || gossip.knows(&peer, &item) {
            continue;
        }
        let caps = peer_manager.peer_capabilities(&peer).unwrap_or(0);
        let payload = if caps & Capability::CompactBlocks.bit() != 0 {
            CompactBlock::from_block(&block).encode()
        } else {
            Inv { items: vec![item] }.encode()
        };
        match payload {
            Ok(payload) => match peer_manager.send_to_peer(&peer, &payload) {
                Ok(()) => gossip.mark_known(peer, item),
                Err(err) => debug!(
                    target: "net::announce",
                    peer = %peer, error = %err, "block announce send failed"
                ),
            },
            Err(err) => {
                warn!(target: "net::announce", error = %err, "block announce encode failed")
            }
        }
    }
}

/// S2-P19 announcer: a new transaction entered the mempool — feed the Erlay
/// known-set and broadcast `INV(tx)` (S2-P13 dedup per peer).
fn announce_new_tx(
    txid_hex: &str,
    peer_manager: &Arc<strangecoin_net::PeerManager>,
    gossip_state: &Arc<Mutex<strangecoin_net::GossipState>>,
    reconciliation_state: &Arc<Mutex<strangecoin_net::ReconciliationState>>,
) {
    let txid: [u8; 32] = match hex::decode(txid_hex)
        .map_err(|e| e.to_string())
        .and_then(|bytes| bytes.try_into().map_err(|_| "txid length".to_string()))
    {
        Ok(bytes) => bytes,
        Err(err) => {
            warn!(target: "net::announce", txid = txid_hex, error = %err, "bad txid — skip announce");
            return;
        }
    };
    reconciliation_state
        .lock()
        .expect("reconciliation state poisoned")
        .note_new_tx(txid);
    let gossip = gossip_state.lock().expect("gossip state poisoned");
    strangecoin_net::announce_tx(txid, peer_manager, &gossip);
}

// ---------------------------------------------------------- mining worker

/// Spawn the mining worker with the same dual-mode rule the sync engine
/// used pre-S2-P19 (ADR-0011): on a tokio runtime the worker is a task;
/// without one (plain `#[test]` threads) it runs on a dedicated thread with
/// its own current-thread runtime. Mining stays on this hybrid path — full
/// mining async migration is Stage 6+ (out of Stage 2 scope).
#[allow(clippy::too_many_arguments)]
fn spawn_mining_worker(
    mining_rx: tokio::sync::mpsc::UnboundedReceiver<MiningTask>,
    peers: Arc<Mutex<Vec<String>>>,
    address: String,
    network_id: u32,
    peer_manager: Arc<strangecoin_net::PeerManager>,
    gossip_state: Arc<Mutex<strangecoin_net::GossipState>>,
    reconciliation_state: Arc<Mutex<strangecoin_net::ReconciliationState>>,
) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(mining_worker_loop(
                mining_rx,
                peers,
                address,
                network_id,
                peer_manager,
                gossip_state,
                reconciliation_state,
            ));
        }
        Err(_) => {
            std::thread::Builder::new()
                .name("mining-worker".into())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("mining worker runtime");
                    rt.block_on(mining_worker_loop(
                        mining_rx,
                        peers,
                        address,
                        network_id,
                        peer_manager,
                        gossip_state,
                        reconciliation_state,
                    ));
                })
                .expect("mining worker thread");
        }
    }
}

/// The mining worker loop (ADR-0011). CPU-bound `apply_tx` + `mine_block`
/// (which holds the facade write lock for the whole PoW search) run inside
/// `spawn_blocking` so runtime workers are never blocked; status updates and
/// the post-mine gossip round await between tasks.
#[allow(clippy::too_many_arguments)]
async fn mining_worker_loop(
    mut mining_rx: tokio::sync::mpsc::UnboundedReceiver<MiningTask>,
    peers: Arc<Mutex<Vec<String>>>,
    address: String,
    network_id: u32,
    peer_manager: Arc<strangecoin_net::PeerManager>,
    gossip_state: Arc<Mutex<strangecoin_net::GossipState>>,
    reconciliation_state: Arc<Mutex<strangecoin_net::ReconciliationState>>,
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
            let _ = status_tx.send(format!("Транзакция отправлена, блок добавлен: {:?}", block));
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
                peer_manager: peer_manager.clone(),
                gossip_state: gossip_state.clone(),
                reconciliation_state: reconciliation_state.clone(),
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
                    debug!(
                        mining_count,
                        ?mining_status_guard,
                        "Статус майнинга обновлён"
                    );
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
    #[cfg(feature = "net")]
    if args.len() > 2 && args[1] == "shadow" && args[2] == "replay" {
        match crate::cli::shadow_replay(&args[3..]).await {
            Ok(code) => std::process::exit(code),
            Err(err) => {
                eprintln!("shadow replay failed: {err}");
                std::process::exit(1);
            }
        }
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
                ..Default::default()
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
        // S2-P11: the generated network.json carries the `seed_config`
        // schema (strangecoin_net::discovery::seeds::SeedConfig) so the
        // operator has a section to fill in. The write goes through the
        // already-parsed document: an existing `seed_config` is preserved
        // instead of being erased by this default path.
        let mut doc = network_config;
        doc["peers"] = serde_json::json!(peers);
        if doc.get("seed_config").is_none() {
            doc["seed_config"] = serde_json::json!({
                "hard_coded_ips": [],
                "dns_seeds": [],
                "dnssec_required": true,
                "default_port": 8081
            });
        }
        let network_content =
            serde_json::to_string_pretty(&doc).expect("Ошибка сериализации network.json");
        fs::write(&network_path, network_content).expect("Ошибка записи в network.json");
    }

    let (mining_tx, mining_rx) = tokio::sync::mpsc::unbounded_channel();
    let (sync_tx, sync_rx) = tokio::sync::mpsc::unbounded_channel();
    let event_bus = Arc::new(events::EventBus::new());
    info!("Каналы майнинга и синхронизации созданы");

    let listen_addr = config.network.listen_addr;
    let port = listen_addr.port();

    // S2-P19: the node's P2P identity (Ed25519, keystore/node/) + the
    // PeerManager that owns connections, bans and scoring. The peer store
    // persists next to the legacy network.json (known_peers/banned_peers).
    let node_identity = wallet::load_or_create_node_keypair(&config.data_dir)
        .expect("Не удалось загрузить или создать Ed25519 node keypair");
    let mut peer_manager_config = config.peer_manager_config();
    peer_manager_config.network_json_path = Some(network_path.clone());
    let peer_manager = Arc::new(strangecoin_net::PeerManager::new(
        peer_manager_config,
        node_identity,
    ));

    // Create shared shutdown signal for graceful shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_ctrlc = Arc::clone(&shutdown);

    let mut node = Node::new(
        listen_addr.to_string(),
        mining_rx,
        port,
        event_bus.clone(),
        config.network_id,
        config.rate_limit_config(),
        Arc::clone(&peer_manager),
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
    let node_peer_manager = Arc::clone(&node.peer_manager);
    let node_gossip_state = Arc::clone(&node.gossip_state);
    let node_reconciliation_state = Arc::clone(&node.reconciliation_state);

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
            peer_manager: node_peer_manager,
            gossip_state: node_gossip_state,
            reconciliation_state: node_reconciliation_state,
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
            peer_manager: Arc::clone(&node.peer_manager),
            gossip_state: Arc::clone(&node.gossip_state),
            reconciliation_state: Arc::clone(&node.reconciliation_state),
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
    /// Delegates to [`BlockchainFacade::open_local`] (S2-P20) — the same
    /// standalone constructor the shadow-replay CLI uses.
    pub fn create_test_blockchain_for_network(db_path: &Path, network_id: u32) -> BlockchainFacade {
        BlockchainFacade::open_local(db_path, network_id)
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

    /// Deterministic-per-process in-memory PeerManager for tests: a fixed
    /// seed base combined with a per-call counter — every manager gets a
    /// distinct Ed25519 identity (a shared identity would make one node's
    /// sync dial look like a self-connection to the peer's accept path),
    /// regtest network id, no network.json persistence (S2-P19).
    pub fn test_peer_manager() -> Arc<strangecoin_net::PeerManager> {
        use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
        use strangecoin_net::{NodeKeypair, PeerManagerConfig};
        static COUNTER: AtomicU32 = AtomicU32::new(1);
        let counter = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let mut seed = [0x53u8; 32];
        seed[0..4].copy_from_slice(&counter.to_be_bytes());
        let identity = NodeKeypair::from_seed(seed);
        let config = PeerManagerConfig {
            network_id: crate::consensus::CHAIN_ID_REGTEST,
            ..PeerManagerConfig::default()
        };
        Arc::new(strangecoin_net::PeerManager::new(config, identity))
    }

    pub fn create_node_for_test(
        bc: &Arc<BlockchainFacade>,
        port: u16,
    ) -> (Node, Arc<Mutex<Vec<String>>>) {
        let peers = Arc::new(Mutex::new(vec![]));
        // S2-P16: integration tests keep the Stage-1-equivalent budget
        // (100 burst / 10 per s) instead of the ADR production default
        // (1000/100) — `tests/rate_limit.rs` floods 200 junk messages and
        // must reach the ban threshold inside that flood.
        let test_rate_limit = crate::network::RateLimitConfig {
            main: crate::network::BucketSpec::new(100, 10),
            ..crate::network::RateLimitConfig::default()
        };
        let node = Node {
            blockchain: Arc::clone(bc),
            peers: Arc::clone(&peers),
            address: format!("127.0.0.1:{}", port),
            sync_rx: tokio::sync::mpsc::unbounded_channel().1,
            rate_limiter: Arc::new(crate::network::RateLimiter::new(test_rate_limit)),
            shutdown: Arc::new(AtomicBool::new(false)),
            accept_task: Arc::new(Mutex::new(None)),
            sync_task_handle: Arc::new(Mutex::new(None)),
            // Один bus на node и facade — как в проде: RBF-замена публикует
            // TxRejected из facade, а майнинг-задача — из node.
            event_bus: bc.event_bus(),
            network_id: crate::consensus::CHAIN_ID_REGTEST,
            // Spawned by `start_server` when the test starts a server.
            inbox: None,
            peer_manager: test_peer_manager(),
            gossip_state: Arc::new(Mutex::new(strangecoin_net::GossipState::new())),
            reconciliation_state: Arc::new(Mutex::new(
                strangecoin_net::ReconciliationState::with_defaults(),
            )),
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
            peer_manager: test_peer_manager(),
            gossip_state: Arc::new(Mutex::new(strangecoin_net::GossipState::new())),
            reconciliation_state: Arc::new(Mutex::new(
                strangecoin_net::ReconciliationState::with_defaults(),
            )),
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
        *tx.signature_mut() = sig_vec;
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
        let mut tx = Transaction::transfer(
            sender_addr,
            receiver_addr,
            amount,
            sender_nonce + 1,
            bc.chain_id(),
        );
        sign_transaction(&mut tx, secret_key);
        assert!(bc.apply_tx(tx).is_ok(), "Transaction rejected");
        mine_current(bc);
    }
}

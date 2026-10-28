//! S2-P12 — `PeerManager`: connection orchestration, ban logic and the
//! periodic tick (ARCHITECT3 §3.6, ARCHITECT.md §11.6: «PeerManager — логика»).
//!
//! The manager owns a [`PeerStore`] (records/bans, behind an `RwLock` — the
//! AGENTS.md lock-ordering layer *between* `blockchain` and `connection_pool`)
//! and a [`ConnectionPool`] (behind a `Mutex`, innermost). Unlike the
//! «pure logic» sketch in ARCHITECT.md §11.6, S2-P12 wires the *outbound*
//! connection flow directly into the manager (explicit decision, S2-P12 plan):
//! [`connect_to_peer`](PeerManager::connect_to_peer) dials TCP, drives the
//! Noise XX handshake (S2-P07..S2-P09) and the HELLO exchange (S2-P10), then
//! registers the resulting [`Connection`]. The *inbound* read loop (app
//! messages arriving after HELLO) is deliberately **not** here — it lands with
//! the async connection workers in S2-P19; until then this module sends
//! (`encrypt_frame` + `try_write`) but does not receive.
//!
//! Ban state machine per ADR-0022: `warn` (score < 0.4, stays connected) →
//! `temp ban 1h` (score < `ban_threshold`, in-memory, expiry via
//! [`tick_at`](PeerManager::tick_at)) → `persistent ban 30d` (score < 0.05,
//! written to `network.json: banned_peers`, survives restart). Rate-limit
//! violations feed the same counter through
//! [`record_rate_violation`](PeerManager::record_rate_violation) (ADR-0023 /
//! S2-P16 integration seam).
//!
//! **Determinism** (Stage 2 checklist, threat V-21 extension): scoring is a
//! pure function of record counters ([`PeerStore::update_score`]); broadcast
//! and tick iterate `node_id`-sorted; no RNG anywhere; wall time appears only
//! at ban-expiry edges — tests inject a simulated clock through `tick_at`.

use super::pool::{Connection, ConnectionPool};
use super::store::{BanReason, PeerStore};
use crate::discovery::addr_gossip::{AddrEntry, AddrSender};
use crate::error::NetError;
use crate::noise::{NodeId, NodeKeypair, NoiseHandshake, NoiseTransport};
use crate::protocol::hello::{
    negotiate_capabilities, read_hello, write_hello, Hello, STAGE2_CAPABILITIES,
};
use crate::protocol::{read_length_prefixed_async, write_length_prefixed_async};
use crate::rate_limiter::RateLimitError;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Score below which a peer is warned (ADR-0022; tunable via
/// `peer_scoring_*` in S2-P17).
pub const DEFAULT_WARN_THRESHOLD: f32 = 0.4;
/// Score below which a peer is temp-banned — the prompt's
/// `config.toml: peer_ban_threshold` (ADR-0022).
pub const DEFAULT_BAN_THRESHOLD: f32 = 0.2;
/// Score below which a peer is persistently banned (ADR-0022).
pub const DEFAULT_PERSISTENT_THRESHOLD: f32 = 0.05;
/// Temp-ban duration (ADR-0022).
pub const DEFAULT_TEMP_BAN_DURATION: Duration = Duration::from_secs(60 * 60);
/// Persistent-ban duration (ADR-0022).
pub const DEFAULT_PERSISTENT_BAN_DURATION: Duration = Duration::from_secs(60 * 60 * 24 * 30);
/// Default total-connection cap (`config.toml: max_connections`).
pub const DEFAULT_MAX_CONNECTIONS: usize = 50;
/// Default per-IP-range cap (`config.toml: max_per_ip_range`, threat V-17).
pub const DEFAULT_MAX_PER_IP_RANGE: usize = 8;
/// Suggested periodic cadence for [`tick`](PeerManager::tick) (ADR-0022:
/// every 60 s). The interval driver itself arrives with the async workers
/// (S2-P19); S2-P12 exposes the primitive.
pub const TICK_INTERVAL: Duration = Duration::from_secs(60);

/// TCP dial budget for one outbound connection attempt.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Budget for one handshake/HELLO frame on an established socket.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest XX handshake message we accept off the wire (round 2 is 192 bytes
/// at S2-P09 auth-blob sizes; the cap rejects gross garbage before `snow`).
const MAX_HANDSHAKE_MSG: usize = 512;
/// Scratch buffer for one handshake message.
const HANDSHAKE_BUF: usize = 512;

/// Tunables for [`PeerManager`] — the `config.toml` mirror (root-crate
/// `Config::peer_manager_config` maps the operator's file onto this) plus
/// operational parameters that are not file tunables.
#[derive(Debug, Clone)]
pub struct PeerManagerConfig {
    /// `peer_ban_threshold` — temp-ban score threshold (ADR-0022).
    pub ban_threshold: f32,
    pub warn_threshold: f32,
    pub persistent_threshold: f32,
    pub temp_ban_duration: Duration,
    pub persistent_ban_duration: Duration,
    /// `max_connections` — total concurrent TCP connections.
    pub max_connections: usize,
    /// `max_per_ip_range` — Sybil cap per IPv4 /24 or IPv6 /64 (V-17).
    pub max_per_ip_range: usize,
    /// HELLO discriminator (invariant #10) — mismatched peers are dropped.
    pub network_id: u32,
    /// Capability bitfield this node advertises (S2-P10).
    pub local_capabilities: u32,
    /// `network.json` path for peer/ban persistence; `None` = in-memory only.
    pub network_json_path: Option<PathBuf>,
}

impl Default for PeerManagerConfig {
    fn default() -> Self {
        PeerManagerConfig {
            ban_threshold: DEFAULT_BAN_THRESHOLD,
            warn_threshold: DEFAULT_WARN_THRESHOLD,
            persistent_threshold: DEFAULT_PERSISTENT_THRESHOLD,
            temp_ban_duration: DEFAULT_TEMP_BAN_DURATION,
            persistent_ban_duration: DEFAULT_PERSISTENT_BAN_DURATION,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            max_per_ip_range: DEFAULT_MAX_PER_IP_RANGE,
            network_id: 3,
            local_capabilities: STAGE2_CAPABILITIES,
            network_json_path: None,
        }
    }
}

/// What one [`tick_at`](PeerManager::tick_at) pass did (observability + test
/// assertions).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickReport {
    pub expired_bans: usize,
    pub newly_banned: usize,
    pub warned: usize,
}

/// Threshold-band evaluation of one score (ADR-0022 ban state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BanDecision {
    NoAction,
    Warn,
    TempBan { until: Instant },
    PersistentBan { until: Instant },
}

fn decide(score: f32, cfg: &PeerManagerConfig, now: Instant) -> BanDecision {
    if score < cfg.persistent_threshold {
        BanDecision::PersistentBan {
            until: now + cfg.persistent_ban_duration,
        }
    } else if score < cfg.ban_threshold {
        BanDecision::TempBan {
            until: now + cfg.temp_ban_duration,
        }
    } else if score < cfg.warn_threshold {
        BanDecision::Warn
    } else {
        BanDecision::NoAction
    }
}

/// Which counter a recorded event increments (ADR-0022 has one penalty
/// channel per counter family).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Counter {
    Invalid,
    Violation,
}

/// Active connection orchestration + ban logic for one node identity.
pub struct PeerManager {
    store: RwLock<PeerStore>,
    pool: Mutex<ConnectionPool>,
    config: PeerManagerConfig,
    identity: NodeKeypair,
    local_height: AtomicU64,
}

impl PeerManager {
    /// Build the manager, loading persisted peers/bans from
    /// `config.network_json_path` when set.
    pub fn new(config: PeerManagerConfig, identity: NodeKeypair) -> Self {
        let mut store = PeerStore::new();
        if let Some(path) = &config.network_json_path {
            if let Err(err) = store.load_from_network_json(path) {
                tracing::warn!(
                    target = "net::peer",
                    error = %err,
                    "failed to load peer state, starting empty"
                );
            }
        }
        Self {
            store: RwLock::new(store),
            pool: Mutex::new(ConnectionPool::new(config.max_per_ip_range)),
            config,
            identity,
            local_height: AtomicU64::new(0),
        }
    }

    pub fn config(&self) -> &PeerManagerConfig {
        &self.config
    }

    /// This node's `node_id` (Ed25519 identity, ADR-0020).
    pub fn local_node_id(&self) -> NodeId {
        self.identity.node_id()
    }

    /// Chain height advertised in outgoing HELLO messages. The orchestrator
    /// refreshes it (wiring lands with S2-P13/S2-P19); default 0.
    pub fn set_local_height(&self, height: u64) {
        self.local_height.store(height, Ordering::Relaxed);
    }

    fn store(&self) -> RwLockWriteGuard<'_, PeerStore> {
        self.store.write().unwrap_or_else(|p| p.into_inner())
    }

    fn store_read(&self) -> RwLockReadGuard<'_, PeerStore> {
        self.store.read().unwrap_or_else(|p| p.into_inner())
    }

    fn pool(&self) -> MutexGuard<'_, ConnectionPool> {
        self.pool.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Snapshot accessors (cloned — no lock escapes the call).
    pub fn get_peer(&self, node_id: &NodeId) -> Option<super::store::PeerRecord> {
        self.store_read().get_peer(node_id).cloned()
    }

    pub fn is_banned(&self, node_id: &NodeId) -> bool {
        self.store_read().is_banned(node_id)
    }

    pub fn peer_count(&self) -> usize {
        self.store_read().len()
    }

    pub fn connected_peers(&self) -> Vec<NodeId> {
        self.pool().node_ids_sorted()
    }

    /// Fold ADDR-gossip entries into the store as unverified peers (S2-P11
    /// → S2-P12 integration). Returns how many identities were new.
    pub fn learn_addrs(&self, entries: Vec<AddrEntry>) -> usize {
        self.store().ingest_addr_entries(entries, Instant::now())
    }

    /// Dial `addr`, run the Noise XX handshake (pinning `expected_node_id`
    /// when known — MITM detection, ADR-0020) and exchange HELLO (S2-P10).
    ///
    /// Admission is checked before the dial (existing ban, total-connection
    /// cap, per-IP-range cap) and re-checked after the handshake, when the
    /// peer's authenticated `node_id` is known. On success the session is
    /// registered in the pool and the store (`verified = true`).
    ///
    /// Returns the authenticated `node_id`. The *inbound* message loop for
    /// this connection is S2-P19's job — S2-P12 sends but does not receive.
    pub async fn connect_to_peer(
        &self,
        addr: std::net::SocketAddr,
        expected_node_id: Option<NodeId>,
    ) -> Result<NodeId, NetError> {
        if expected_node_id == Some(self.local_node_id()) {
            return Err(NetError::Peer("refusing to dial self".to_string()));
        }
        if let Some(node_id) = expected_node_id {
            if self.is_banned(&node_id) {
                return Err(NetError::Peer(format!("peer {node_id} is banned")));
            }
        }
        self.check_admission(addr)?;

        let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| NetError::Transport(format!("connect to {addr} timed out")))?
            .map_err(|err| NetError::Transport(format!("connect to {addr}: {err}")))?;

        let mut handshake = NoiseHandshake::initiator(
            &self.identity.noise_static_private_key(),
            &self.identity,
            expected_node_id,
        )?;
        drive_initiator_handshake(&mut handshake, &mut stream).await?;
        let (mut transport, peer_node_id) = handshake.into_transport()?;

        // The identity is now authenticated — re-check bans and caps before
        // anything is registered (an attacker rotating IPs cannot escape a
        // node_id-keyed ban; ADR-0022).
        if self.is_banned(&peer_node_id) {
            return Err(NetError::Peer(format!("peer {peer_node_id} is banned")));
        }
        self.check_admission(addr)?;

        let hello = Hello::new(
            self.config.network_id,
            self.config.local_capabilities,
            self.local_height.load(Ordering::Relaxed),
        );
        let mut out = Vec::new();
        write_hello(&mut transport, &hello, &mut out)?;
        timeout(IO_TIMEOUT, write_all_bytes(&mut stream, &out))
            .await
            .map_err(|_| NetError::Transport("HELLO write timed out".to_string()))??;

        let peer_hello =
            read_hello_frame(&mut transport, &mut stream, self.config.network_id).await?;
        let capabilities =
            negotiate_capabilities(self.config.local_capabilities, peer_hello.capabilities);
        let peer_addr = stream
            .peer_addr()
            .map_err(|err| NetError::Transport(err.to_string()))?;

        let connection = Arc::new(Connection::new(
            transport,
            stream,
            peer_addr,
            peer_node_id,
            capabilities,
        ));
        self.register_connection(connection)?;
        tracing::info!(
            target = "net::peer",
            peer = %peer_node_id,
            addr = %addr,
            capabilities,
            "outbound peer connection established"
        );
        Ok(peer_node_id)
    }

    /// Register an already-established session (post-handshake + HELLO).
    /// Used by [`connect_to_peer`](Self::connect_to_peer) and — once the
    /// inbound accept loop lands (S2-P19) — by the responder side.
    pub fn register_connection(&self, connection: Arc<Connection>) -> Result<NodeId, NetError> {
        let node_id = connection.node_id();
        if node_id == self.local_node_id() {
            return Err(NetError::Peer("refusing to register self".to_string()));
        }
        if self.is_banned(&node_id) {
            return Err(NetError::Peer(format!("peer {node_id} is banned")));
        }
        {
            let mut pool = self.pool();
            if pool.len() >= self.config.max_connections {
                return Err(NetError::Peer(format!(
                    "max_connections {} reached",
                    self.config.max_connections
                )));
            }
            pool.add_connection(connection.clone())?;
        }
        self.store()
            .add_peer(node_id, connection.peer_addr(), true, Instant::now());
        Ok(node_id)
    }

    /// Drop the live session for `node_id`. The store record (score, ban
    /// eligibility) is kept — only the socket goes.
    pub fn disconnect_peer(&self, node_id: &NodeId) -> bool {
        self.pool().remove_connection(node_id).is_some()
    }

    /// Encrypt `payload` and send it to every connected, non-banned peer
    /// (ordered by `node_id`, determinism #4). Returns how many peers got the
    /// frame; per-peer send failures are logged and skipped.
    pub fn broadcast_message(&self, payload: &[u8]) -> usize {
        let banned = self.store_read();
        let pool = self.pool();
        let mut sent = 0usize;
        for node_id in pool.node_ids_sorted() {
            if banned.is_banned(&node_id) {
                continue;
            }
            let Some(connection) = pool.get_connection(&node_id) else {
                continue;
            };
            match connection.send_payload(payload) {
                Ok(()) => sent += 1,
                Err(err) => tracing::warn!(
                    target = "net::peer",
                    peer = %node_id,
                    error = %err,
                    "broadcast to peer failed, continuing"
                ),
            }
        }
        sent
    }

    /// Encrypt `payload` and send it to one connected peer (S2-P13 gossip
    /// per-peer INV path; also the S2-P11 `AddrSender` primitive). Errors
    /// when the peer is not connected.
    pub fn send_to_peer(&self, node_id: &NodeId, payload: &[u8]) -> Result<(), NetError> {
        let connection = self
            .pool()
            .get_connection(node_id)
            .ok_or_else(|| NetError::Peer(format!("peer {node_id} not connected")))?;
        connection.send_payload(payload)
    }

    /// A peer sent a block (or tx) that failed validation — ADR-0022
    /// `invalid_blocks_txs_sent` counter (threat V-15).
    pub fn record_invalid_block(&self, node_id: NodeId) {
        self.record_event(node_id, BanReason::InvalidBlocks, Counter::Invalid);
    }

    /// Protocol misuse (malformed message, unexpected payload, …) — ADR-0022
    /// `protocol_violations` counter.
    pub fn record_protocol_violation(&self, node_id: NodeId) {
        self.record_event(node_id, BanReason::ProtocolViolation, Counter::Violation);
    }

    /// Rate-limit exceed — the ADR-0023 / S2-P16 seam: feeds the same
    /// `protocol_violations` penalty channel, banning under `RateLimit`.
    pub fn record_rate_violation(&self, node_id: NodeId) {
        self.record_event(node_id, BanReason::RateLimit, Counter::Violation);
    }

    /// Apply a rate-limiter verdict to the slow path (ADR-0023 §Integration,
    /// S2-P16).
    ///
    /// * `Ok` — nothing to do.
    /// * [`RateLimitError::Exceeded`] — one refused message = one violation:
    ///   [`record_rate_violation`](Self::record_rate_violation) feeds the
    ///   ADR-0022 `protocol_violations` penalty channel (score drops on the
    ///   next tick → warn/temp/persistent ladder).
    /// * [`RateLimitError::Ban`] — the same violation record **plus** an
    ///   explicit [`ban_peer`](Self::ban_peer) under
    ///   [`BanReason::RateLimit`] until `now + ban_duration`. The ban is
    ///   written through to `network.json` immediately (`persistent = true`
    ///   is the S2-P12 "durable write" flag — the ban still expires by
    ///   `until`, it simply survives a restart for its remaining duration).
    ///
    /// The fast path (the address-keyed limiter) reacts in milliseconds; this
    /// slow path converts repetition into reputation damage. Production call
    /// sites arrive with the S2-P19 inbound read loop (node_id is only known
    /// after the Noise auth round); S2-P16 wires and tests the seam.
    ///
    /// Note: [`record_rate_violation`](Self::record_rate_violation) ignores
    /// unknown peers — the caller must have `learn_addrs`/`register_connection`
    /// records for `node_id` (tests do this explicitly).
    pub fn apply_rate_verdict(
        &self,
        node_id: NodeId,
        verdict: &Result<(), RateLimitError>,
        ban_duration: Duration,
    ) {
        let Err(err) = verdict else {
            return;
        };
        // ADR-0023: each refused message is one violation — both verdict
        // classes feed the scoring channel (Exceeded = the refusal itself,
        // Ban = the refusal that crossed the threshold).
        self.record_rate_violation(node_id);
        if matches!(err, RateLimitError::Ban) {
            let until = Instant::now() + ban_duration;
            self.ban_peer(node_id, BanReason::RateLimit, until, true);
        }
    }

    /// Feed one observed message-latency sample into the record's EWMA
    /// (`ewma = 0.9 * ewma + 0.1 * sample`, ADR-0022 α = 0.1).
    ///
    /// Grading only — latency never triggers a ban decision here; that
    /// structural false-positive guarantee (a responsive slow peer stays
    /// above the temp threshold) is ADR-0022's. The formula is provisional
    /// until `scoring.rs` (S2-P17) owns all factor updates.
    pub fn record_latency(&self, node_id: NodeId, sample: Duration) {
        let mut store = self.store();
        if let Some(record) = store.get_peer_mut(&node_id) {
            let next = 0.9 * record.latency_ewma.as_secs_f32() + 0.1 * sample.as_secs_f32();
            record.latency_ewma = Duration::from_secs_f32(next.max(0.0));
            PeerStore::update_score(record);
        }
    }

    /// The active ban for `node_id`, if any (cloned snapshot).
    pub fn get_ban(&self, node_id: &NodeId) -> Option<super::store::BanRecord> {
        self.store_read().ban(node_id).cloned()
    }

    /// How many bans were lifted into the audit table (ADR-0022
    /// `historical_bans`).
    pub fn historical_bans_len(&self) -> usize {
        self.store_read().historical_bans().len()
    }

    fn record_event(&self, node_id: NodeId, reason: BanReason, counter: Counter) {
        let now = Instant::now();
        let mut banned = false;
        let mut needs_save = false;
        {
            let mut store = self.store();
            let Some(record) = store.get_peer_mut(&node_id) else {
                tracing::debug!(
                    target = "net::peer",
                    peer = %node_id,
                    "event for unknown peer ignored"
                );
                return;
            };
            match counter {
                Counter::Invalid => record.invalid_count += 1,
                Counter::Violation => record.violation_count += 1,
            }
            record.last_seen = now;
            let score = PeerStore::update_score(record);
            match decide(score, &self.config, now) {
                BanDecision::NoAction => {}
                BanDecision::Warn => {
                    tracing::warn!(
                        target = "net::peer",
                        peer = %node_id,
                        score,
                        "peer score in warn band"
                    );
                }
                BanDecision::TempBan { until } => {
                    store.ban_peer(node_id, reason, until, now, false);
                    banned = true;
                }
                BanDecision::PersistentBan { until } => {
                    store.ban_peer(node_id, reason, until, now, true);
                    banned = true;
                    needs_save = true;
                }
            }
        }
        if banned {
            tracing::warn!(
                target = "net::peer",
                peer = %node_id,
                ?reason,
                "peer banned by scoring"
            );
            self.disconnect_peer(&node_id);
        }
        if needs_save {
            self.persist();
        }
    }

    /// Ban a peer explicitly (operator action, S2-P16/S2-P17 integration).
    /// Persistent bans are written to `network.json` immediately.
    pub fn ban_peer(
        &self,
        node_id: NodeId,
        reason: BanReason,
        until: Instant,
        persistent: bool,
    ) -> bool {
        let banned_at = Instant::now();
        let added = self
            .store()
            .ban_peer(node_id, reason, until, banned_at, persistent);
        if added {
            self.disconnect_peer(&node_id);
            if persistent {
                self.persist();
            }
        }
        added
    }

    /// Lift a ban (moves the record to the audit table and rewrites the
    /// persisted `banned_peers` list).
    pub fn unban_peer(&self, node_id: &NodeId) -> bool {
        let removed = self.store().unban_peer(node_id).is_some();
        if removed {
            self.persist();
        }
        removed
    }

    /// Periodic maintenance with the real clock — expiry + score re-check of
    /// every connected peer. See [`tick_at`](Self::tick_at).
    pub fn tick(&self) -> TickReport {
        self.tick_at(Instant::now())
    }

    /// Same as [`tick`](Self::tick) with an injected clock — the ban-expiry
    /// edge ADR-0022 §Determinism allows, made testable (simulated 1 h / 30 d
    /// without sleeping).
    pub fn tick_at(&self, now: Instant) -> TickReport {
        let mut report = TickReport::default();
        let connected = self.pool().node_ids_sorted();
        let mut to_disconnect: Vec<NodeId> = Vec::new();
        let mut needs_save = false;
        {
            let mut store = self.store();
            let expired = store.expire_bans(now);
            if !expired.is_empty() {
                report.expired_bans = expired.len();
                needs_save = true;
                for node_id in &expired {
                    tracing::info!(
                        target = "net::peer",
                        peer = %node_id,
                        "ban expired, peer reinstated (audit trail kept)"
                    );
                }
            }
            for node_id in connected {
                let Some(record) = store.get_peer_mut(&node_id) else {
                    continue;
                };
                let score = PeerStore::update_score(record);
                match decide(score, &self.config, now) {
                    BanDecision::NoAction => {}
                    BanDecision::Warn => {
                        report.warned += 1;
                        tracing::warn!(
                            target = "net::peer",
                            peer = %node_id,
                            score,
                            "peer score in warn band (tick)"
                        );
                    }
                    BanDecision::TempBan { until } => {
                        if store.ban_peer(node_id, BanReason::ProtocolViolation, until, now, false)
                        {
                            report.newly_banned += 1;
                            to_disconnect.push(node_id);
                        }
                    }
                    BanDecision::PersistentBan { until } => {
                        if store.ban_peer(node_id, BanReason::ProtocolViolation, until, now, true) {
                            report.newly_banned += 1;
                            to_disconnect.push(node_id);
                            needs_save = true;
                        }
                    }
                }
            }
        }
        for node_id in &to_disconnect {
            self.disconnect_peer(node_id);
        }
        if needs_save {
            self.persist();
        }
        report
    }

    /// Write the store to `network.json` (no-op without a configured path).
    fn persist(&self) {
        let Some(path) = &self.config.network_json_path else {
            return;
        };
        if let Err(err) = self.store_read().save_to_network_json(path) {
            tracing::warn!(
                target = "net::peer",
                error = %err,
                "failed to persist peer state"
            );
        }
    }

    /// Pre-dial admission: total cap + per-IP-range cap (threat V-17).
    fn check_admission(&self, addr: std::net::SocketAddr) -> Result<(), NetError> {
        let pool = self.pool();
        if pool.len() >= self.config.max_connections {
            return Err(NetError::Peer(format!(
                "max_connections {} reached",
                self.config.max_connections
            )));
        }
        if pool.count_by_ip_range(addr.ip()) >= self.config.max_per_ip_range {
            return Err(NetError::Peer(format!(
                "per-IP-range connection limit reached for {}",
                addr.ip()
            )));
        }
        Ok(())
    }
}

impl AddrSender for PeerManager {
    fn send_addr(&self, peer: &NodeId, payload: &[u8]) -> Result<(), NetError> {
        self.send_to_peer(peer, payload)
    }
}

/// Drive the three XX round trips on the initiator side, length-prefixed
/// framing per round (the same `[len][msg]` shape `sync.rs` uses on the
/// plaintext protocol).
async fn drive_initiator_handshake(
    handshake: &mut NoiseHandshake,
    stream: &mut TcpStream,
) -> Result<(), NetError> {
    let mut buf = [0u8; HANDSHAKE_BUF];
    let mut payload = [0u8; HANDSHAKE_BUF];

    // Round 1: e.
    let n = handshake.write_message(&[], &mut buf)?;
    timeout(IO_TIMEOUT, write_length_prefixed_async(stream, &buf[..n]))
        .await
        .map_err(|_| NetError::Transport("handshake round 1 write timed out".to_string()))??;
    // Round 2: read the responder's e, ee, s, es + auth blob.
    let msg = read_bounded_handshake_msg(stream).await?;
    handshake.read_message(&msg, &mut payload)?;

    // Round 3: s, se + auth blob — finishes the handshake.
    let n = handshake.write_message(&[], &mut buf)?;
    timeout(IO_TIMEOUT, write_length_prefixed_async(stream, &buf[..n]))
        .await
        .map_err(|_| NetError::Transport("handshake round 3 write timed out".to_string()))??;
    Ok(())
}

/// Read one length-prefixed handshake message, rejecting anything above
/// [`MAX_HANDSHAKE_MSG`] before it reaches `snow` (invariant #16 discipline).
async fn read_bounded_handshake_msg(stream: &mut TcpStream) -> Result<Vec<u8>, NetError> {
    let msg = timeout(IO_TIMEOUT, read_length_prefixed_async(stream))
        .await
        .map_err(|_| NetError::Transport("handshake read timed out".to_string()))??;
    if msg.len() > MAX_HANDSHAKE_MSG {
        return Err(NetError::Protocol(format!(
            "handshake message {} bytes > {MAX_HANDSHAKE_MSG}",
            msg.len()
        )));
    }
    Ok(msg)
}

/// Read one encrypted HELLO frame off the socket and validate it against the
/// local network (S2-P10: version + network_id rejection rules).
async fn read_hello_frame(
    transport: &mut NoiseTransport,
    stream: &mut TcpStream,
    network_id: u32,
) -> Result<Hello, NetError> {
    let body = timeout(IO_TIMEOUT, read_length_prefixed_async(stream))
        .await
        .map_err(|_| NetError::Transport("HELLO read timed out".to_string()))??;
    // `read_hello`/`decrypt_frame` want the frame *with* its length prefix;
    // the async reader stripped it — put it back.
    let mut raw = Vec::with_capacity(4 + body.len());
    raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
    raw.extend_from_slice(&body);
    read_hello(transport, &raw, network_id)
}

async fn write_all_bytes(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), NetError> {
    use tokio::io::AsyncWriteExt;
    stream
        .write_all(bytes)
        .await
        .map_err(|err| NetError::Transport(err.to_string()))?;
    Ok(())
}

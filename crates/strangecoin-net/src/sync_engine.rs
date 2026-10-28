//! SyncEngine (ADR-0010): the single consumer of incoming data.
//!
//! Network handlers and the sync loop never touch blockchain write
//! paths — they parse, dedupe and `try_send` into the bounded inbox channels
//! ([`Inbox`]); this task drains those channels strictly sequentially and
//! performs the whole validate → apply → announce pipeline against
//! [`BlockchainView`]. Read-serving (`GET_HEADERS` / `GET_BLOCKS`) stays on
//! read-only facade calls and does not pass through here. (ADR-0011:
//! handlers are tokio tasks, not threads.)

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{error::TrySendError, Receiver, Sender, UnboundedSender};
use tracing::{debug, info, warn};

use crate::blockchain_view::BlockchainView;
use crate::rate_limiter::RateLimiter;
use strangecoin_core::types::{AccountState, Block, BlockHeader, ChainSnapshot, Transaction};

/// Publishing side of the engine's announce pipeline (S2-P06 cycle break):
/// the engine emits lifecycle events through this object-safe trait instead
/// of the node's concrete event bus — the monolith implements it for its
/// `EventBus` and forwards to `NodeEvent`.
///
/// Object-safe by design: `SyncEngine` holds `Arc<dyn EventSink>`.
pub trait EventSink: Send + Sync {
    fn block_applied(&self, height: u64, hash: String);
    fn block_reorged(&self, old_tip: String, new_tip: String);
    fn tx_accepted(&self, txid: String);
    fn tx_rejected(&self, txid: String, reason: String);
    fn state_persisted(&self, height: u64);
}

/// Capacity of the headers lane (ADR-0010: headers get priority).
const HEADERS_INBOX_CAP: usize = 64;
/// Capacity of the blocks/tx/candidate lane.
const BLOCKS_INBOX_CAP: usize = 256;
/// Bound of the seen-set used for dedupe; cleared when it overflows.
const SEEN_CAP: usize = 4096;
/// Wake-up period while idle: the engine observes the shared shutdown flag
/// at least this often even when no messages arrive.
const SHUTDOWN_POLL: Duration = Duration::from_millis(100);

// ------------------------------------------------------------------ incoming

/// One unit of work for the engine. Producers build these from wire data and
/// enqueue them through [`Inbox`]; `from` attributes the message to the peer
/// it came from (rate limiting / bans).
#[derive(Debug, Clone)]
pub enum Incoming {
    NewBlock {
        block: Block,
        from: Option<SocketAddr>,
    },
    NewHeaders {
        headers: Vec<BlockHeader>,
        from: Option<SocketAddr>,
    },
    NewTx {
        tx: Transaction,
        from: Option<SocketAddr>,
    },
    /// A full candidate chain: the adopted shape used by the headers-first
    /// download result (the v0.8.6 `UPDATE_BLOCKCHAIN` gossip path that also
    /// produced it was removed in S2-P18; `balances`/`mempool_txs`/
    /// `pending_transactions` stay for the wire-snapshot shape
    /// `BlockchainDeserialize` and future snapshot sync).
    CandidateChain {
        chain: Vec<Block>,
        balances: Option<HashMap<String, AccountState>>,
        mempool_txs: Vec<Transaction>,
        pending_transactions: Vec<Transaction>,
        difficulty: u32,
        from: Option<SocketAddr>,
    },
}

impl Incoming {
    /// Attribution of the message: the peer whose connection produced it.
    pub fn sender(&self) -> Option<SocketAddr> {
        match self {
            Incoming::NewBlock { from, .. }
            | Incoming::NewHeaders { from, .. }
            | Incoming::NewTx { from, .. }
            | Incoming::CandidateChain { from, .. } => *from,
        }
    }

    /// Key for the bounded seen-set (`None` = never deduplicated).
    fn dedupe_key(&self) -> Option<String> {
        match self {
            Incoming::NewBlock { block, .. } => Some(format!("b:{}", block.hash)),
            Incoming::NewTx { tx, .. } => Some(format!(
                "t:{}",
                hex::encode(strangecoin_core::serialize::txid(tx))
            )),
            // A block hash commits to its ancestry, so the tip hash uniquely
            // identifies the whole candidate chain.
            Incoming::CandidateChain { chain, .. } => chain.last().map(|b| format!("c:{}", b.hash)),
            Incoming::NewHeaders { .. } => None,
        }
    }
}

// --------------------------------------------------------------------- inbox

/// Bounded, deduplicated producer handle for the engine's two lanes.
///
/// Clone it to hand it to handler threads, the sync task and mining — every
/// producer shares the same channels and seen-set.
#[derive(Clone)]
pub struct Inbox {
    headers: Sender<Incoming>,
    blocks: Sender<Incoming>,
    seen: Arc<Mutex<SeenCache>>,
    rate_limiter: Arc<RateLimiter>,
}

impl Inbox {
    /// Enqueue without attribution-based bans (pull results: headers-first
    /// candidate; tests).
    ///
    /// Returns `false` when the message was dropped — a duplicate that is
    /// already in the seen-set, or a full/closed lane.
    pub fn push(&self, msg: Incoming) -> bool {
        self.enqueue(msg, false)
    }

    /// Enqueue for an inbound connection (future gossip wiring, S2-P19):
    /// like [`push`](Self::push), but a producer that pushes into a FULL
    /// inbox gets its attributed peer banned through the [`RateLimiter`]
    /// (ADR-0010 backpressure).
    pub fn push_inbound(&self, msg: Incoming) -> bool {
        self.enqueue(msg, true)
    }

    fn enqueue(&self, msg: Incoming, inbound: bool) -> bool {
        let key = msg.dedupe_key();
        if let Some(key) = &key {
            let mut seen = self.seen.lock().expect("sync engine seen-set poisoned");
            if !seen.reserve(key) {
                debug!(key = %key, "Duplicate incoming message dropped");
                return false;
            }
        }
        let lane: &Sender<Incoming> = match &msg {
            Incoming::NewHeaders { .. } => &self.headers,
            _ => &self.blocks,
        };
        match lane.try_send(msg) {
            Ok(()) => true,
            Err(TrySendError::Full(msg)) => {
                warn!(peer = ?msg.sender(), inbound, "Sync inbox full, message dropped");
                if inbound {
                    if let Some(addr) = msg.sender() {
                        self.rate_limiter.ban(addr);
                    }
                }
                if let Some(key) = key {
                    self.seen
                        .lock()
                        .expect("sync engine seen-set poisoned")
                        .release(&key);
                }
                false
            }
            Err(TrySendError::Closed(msg)) => {
                debug!(peer = ?msg.sender(), "Sync inbox closed, message dropped");
                if let Some(key) = key {
                    self.seen
                        .lock()
                        .expect("sync engine seen-set poisoned")
                        .release(&key);
                }
                false
            }
        }
    }
}

/// Bounded set of keys already queued or handled; dropped duplicates never
/// reach validation. Cleared on overflow (fresh traffic beats stale marks).
#[derive(Default)]
struct SeenCache {
    keys: HashSet<String>,
}

impl SeenCache {
    /// Reserve `key`: `true` when it was not present before (and now is).
    fn reserve(&mut self, key: &str) -> bool {
        if self.keys.contains(key) {
            return false;
        }
        if self.keys.len() >= SEEN_CAP {
            debug!("Seen-set full, clearing dedupe cache");
            self.keys.clear();
        }
        self.keys.insert(key.to_string());
        true
    }

    /// Give `key` back when the message could not be queued after all.
    fn release(&mut self, key: &str) {
        self.keys.remove(key);
    }
}

// -------------------------------------------------------------------- engine

/// The single consumer task: drains both lanes sequentially and owns the
/// validate → apply → announce pipeline (ADR-0010).
pub struct SyncEngine {
    headers_rx: Receiver<Incoming>,
    blocks_rx: Receiver<Incoming>,
    facade: Arc<dyn BlockchainView>,
    bus: Arc<dyn EventSink>,
    sync_tx: UnboundedSender<ChainSnapshot>,
    shutdown: Arc<AtomicBool>,
}

impl SyncEngine {
    /// Drain until the shutdown flag flips or both lanes close.
    ///
    /// `biased` selection polls the headers lane first, so a peer's headers
    /// are processed before its blocks (ADR-0010). The tick branch is last:
    /// data always wins, and an idle engine still wakes to observe shutdown.
    pub async fn run(self) {
        let SyncEngine {
            mut headers_rx,
            mut blocks_rx,
            facade,
            bus,
            sync_tx,
            shutdown,
        } = self;
        info!("SyncEngine started");
        let mut tick = tokio::time::interval(SHUTDOWN_POLL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut headers_open = true;
        let mut blocks_open = true;
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if !headers_open && !blocks_open {
                break;
            }
            tokio::select! {
                biased;
                msg = headers_rx.recv(), if headers_open => match msg {
                    Some(m) => process(facade.as_ref(), bus.as_ref(), &sync_tx, m),
                    None => headers_open = false,
                },
                msg = blocks_rx.recv(), if blocks_open => match msg {
                    Some(m) => process(facade.as_ref(), bus.as_ref(), &sync_tx, m),
                    None => blocks_open = false,
                },
                _ = tick.tick() => {}
            }
        }
        info!("SyncEngine stopped");
    }
}

/// Spawn the engine and return the producer handle.
///
/// S2-P19: must be called **on** a tokio runtime — the engine becomes a
/// task on it (`tokio::spawn`). The pre-S2-P19 dedicated-thread fallback for
/// plain `#[test]` contexts is gone: the whole network stack is tokio now
/// (ADR-0011 continuation), and callers without a runtime wrap their work in
/// `#[tokio::test]` / `Runtime::block_on`.
pub fn spawn(
    facade: Arc<dyn BlockchainView>,
    bus: Arc<dyn EventSink>,
    sync_tx: UnboundedSender<ChainSnapshot>,
    shutdown: Arc<AtomicBool>,
    rate_limiter: Arc<RateLimiter>,
) -> Inbox {
    let (headers_tx, headers_rx) = tokio::sync::mpsc::channel(HEADERS_INBOX_CAP);
    let (blocks_tx, blocks_rx) = tokio::sync::mpsc::channel(BLOCKS_INBOX_CAP);
    let inbox = Inbox {
        headers: headers_tx,
        blocks: blocks_tx,
        seen: Arc::new(Mutex::new(SeenCache::default())),
        rate_limiter,
    };
    let engine = SyncEngine {
        headers_rx,
        blocks_rx,
        facade,
        bus,
        sync_tx,
        shutdown,
    };
    tokio::spawn(engine.run());
    inbox
}

// --------------------------------------------------------------- processing

fn process(
    facade: &dyn BlockchainView,
    bus: &dyn EventSink,
    sync_tx: &UnboundedSender<ChainSnapshot>,
    msg: Incoming,
) {
    match msg {
        Incoming::CandidateChain {
            chain,
            balances,
            mempool_txs,
            pending_transactions,
            difficulty,
            from,
        } => {
            let peer = peer_label(from);
            let old_tip = facade.tip_hash();
            match facade.adopt_candidate(chain, balances, mempool_txs.clone(), difficulty) {
                Ok(true) => {
                    info!(peer = %peer, "Candidate chain adopted");
                    announce(facade, bus, sync_tx, old_tip);
                }
                Ok(false) => {
                    // Fork choice or validation rejected the candidate:
                    // salvage its mempool payload (legacy behaviour) without
                    // announcing anything — the tip did not change.
                    let mut salvaged = 0usize;
                    for tx in mempool_txs.iter().chain(pending_transactions.iter()) {
                        if !facade.chain_contains_tx(tx)
                            && !facade.mempool_contains(&strangecoin_core::serialize::txid(tx))
                            && facade.apply_tx(tx.clone()).is_ok()
                        {
                            salvaged += 1;
                        }
                    }
                    facade.save_state();
                    debug!(peer = %peer, salvaged, "Candidate rejected, mempool payload salvaged");
                }
                Err(e) => warn!(peer = %peer, error = %e, "Candidate adoption failed"),
            }
        }
        Incoming::NewBlock { block, from } => {
            let peer = peer_label(from);
            let old_tip = facade.tip_hash();
            match facade.add_block(block) {
                Ok(()) => {
                    info!(peer = %peer, "Block applied");
                    announce(facade, bus, sync_tx, old_tip);
                }
                Err(e) => debug!(peer = %peer, error = %e, "Block rejected"),
            }
        }
        Incoming::NewTx { tx, from } => {
            let peer = peer_label(from);
            let txid = hex::encode(strangecoin_core::serialize::txid(&tx));
            match facade.apply_tx(tx) {
                Ok(()) => {
                    debug!(peer = %peer, txid = %txid, "Transaction accepted");
                    bus.tx_accepted(txid);
                }
                Err(e) => {
                    debug!(peer = %peer, txid = %txid, error = %e, "Transaction rejected");
                    bus.tx_rejected(txid, e);
                }
            }
        }
        Incoming::NewHeaders { headers, .. } => {
            // Stage 1 sync is pull-driven: the headers-first loop dials peers
            // itself, so an inbound announcement is acknowledged only.
            debug!(
                count = headers.len(),
                "NewHeaders acknowledged (pull-based sync in Stage 1)"
            );
        }
    }
}

/// Announce a successful apply: reorg (when the old tip did not survive),
/// persistence and application events, then the GUI snapshot over `sync_tx`.
fn announce(
    facade: &dyn BlockchainView,
    bus: &dyn EventSink,
    sync_tx: &UnboundedSender<ChainSnapshot>,
    old_tip: String,
) {
    // saturating_sub: a view whose add_block did not extend the chain (test
    // stubs) must not underflow the announced height (S2-P19 hardening).
    let height = (facade.chain_len() as u64).saturating_sub(1);
    let new_tip = facade.tip_hash();
    if new_tip != old_tip {
        let old_survives = facade.chain_contains_block(&old_tip);
        if !old_survives {
            bus.block_reorged(old_tip, new_tip.clone());
        }
    }
    bus.state_persisted(height);
    bus.block_applied(height, new_tip);
    let _ = sync_tx.send(facade.snapshot_wire());
}

fn peer_label(from: Option<SocketAddr>) -> String {
    from.map(|a| a.to_string())
        .unwrap_or_else(|| "pull".to_string())
}

// -------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn test_inbox() -> (Inbox, Receiver<Incoming>, Receiver<Incoming>) {
        let (headers_tx, headers_rx) = tokio::sync::mpsc::channel(HEADERS_INBOX_CAP);
        let (blocks_tx, blocks_rx) = tokio::sync::mpsc::channel(BLOCKS_INBOX_CAP);
        let inbox = Inbox {
            headers: headers_tx,
            blocks: blocks_tx,
            seen: Arc::new(Mutex::new(SeenCache::default())),
            rate_limiter: Arc::new(RateLimiter::default()),
        };
        (inbox, blocks_rx, headers_rx)
    }

    fn candidate(tip: &str, from: Option<SocketAddr>) -> Incoming {
        let block = Block {
            index: 1,
            timestamp: 0,
            transactions: Vec::new(),
            previous_hash: "0".repeat(64),
            hash: tip.to_string(),
            nonce: 0,
            target: "ff".repeat(32),
            consensus_version: 1,
            state_root: [0u8; 32],
            tx_root: [0u8; 32],
            receipts_root: [0u8; 32],
        };
        Incoming::CandidateChain {
            chain: vec![block],
            balances: None,
            mempool_txs: Vec::new(),
            pending_transactions: Vec::new(),
            difficulty: 1,
            from,
        }
    }

    fn addr(port: u16) -> SocketAddr {
        format!("127.0.0.1:{}", port).parse().expect("socket addr")
    }

    fn drain_count(rx: &mut Receiver<Incoming>) -> usize {
        let mut n = 0;
        while rx.try_recv().is_ok() {
            n += 1;
        }
        n
    }

    #[test]
    fn duplicate_candidate_is_dropped_at_the_inbox() {
        let (inbox, mut blocks_rx, _headers_rx) = test_inbox();
        assert!(inbox.push(candidate("aa", None)));
        assert!(
            !inbox.push(candidate("aa", None)),
            "duplicate tip must be deduplicated before validation"
        );
        let first = blocks_rx.try_recv().expect("first push must be queued");
        assert!(matches!(first, Incoming::CandidateChain { .. }));
        assert!(
            blocks_rx.try_recv().is_err(),
            "duplicate must not be queued twice"
        );
    }

    #[test]
    fn full_pull_lane_drops_without_ban() {
        let (inbox, mut blocks_rx, _headers_rx) = test_inbox();
        let peer = addr(19001);
        for i in 0..BLOCKS_INBOX_CAP {
            assert!(inbox.push(candidate(&format!("tip{}", i), Some(peer))));
        }
        assert!(
            !inbox.push(candidate("overflow", Some(peer))),
            "message over capacity must be dropped"
        );
        assert_eq!(drain_count(&mut blocks_rx), BLOCKS_INBOX_CAP);
        assert!(
            !inbox.rate_limiter.is_banned(peer),
            "pull-path drops must not ban the peer (ADR-0010)"
        );
    }

    #[test]
    fn full_inbound_lane_bans_the_producer() {
        let (inbox, mut blocks_rx, _headers_rx) = test_inbox();
        let peer = addr(19002);
        // The accept path counts the connection first, so the peer has a
        // counter to ban — same order as the production handler.
        inbox.rate_limiter.check(peer).expect("first check passes");
        for i in 0..BLOCKS_INBOX_CAP {
            assert!(inbox.push_inbound(candidate(&format!("tip{}", i), Some(peer))));
        }
        assert!(
            !inbox.push_inbound(candidate("overflow", Some(peer))),
            "message over capacity must be dropped"
        );
        assert_eq!(drain_count(&mut blocks_rx), BLOCKS_INBOX_CAP);
        assert!(
            inbox.rate_limiter.is_banned(peer),
            "inbound push into a full inbox must ban (ADR-0010)"
        );
    }

    #[test]
    fn headers_lane_is_separate_from_blocks() {
        let (inbox, mut blocks_rx, mut headers_rx) = test_inbox();
        assert!(inbox.push(Incoming::NewHeaders {
            headers: Vec::new(),
            from: None,
        }));
        assert_eq!(drain_count(&mut headers_rx), 1);
        assert!(
            blocks_rx.try_recv().is_err(),
            "headers must not occupy the blocks lane"
        );
        assert!(inbox.push(candidate("tip", None)));
        assert_eq!(drain_count(&mut blocks_rx), 1);
    }

    // ---------------------------------------------------- clean shutdown

    struct NoopView;

    impl BlockchainView for NoopView {
        fn tip_hash(&self) -> String {
            String::new()
        }
        fn chain_len(&self) -> usize {
            0
        }
        fn adopt_candidate(
            &self,
            _candidate_chain: Vec<Block>,
            _candidate_balances: Option<std::collections::HashMap<String, AccountState>>,
            _candidate_mempool_txs: Vec<Transaction>,
            _candidate_difficulty: u32,
        ) -> Result<bool, String> {
            Ok(false)
        }
        fn apply_tx(&self, _transaction: Transaction) -> Result<(), String> {
            Ok(())
        }
        fn add_block(&self, _block: Block) -> Result<(), String> {
            Ok(())
        }
        fn chain_contains_tx(&self, _tx: &Transaction) -> bool {
            false
        }
        fn mempool_contains(&self, _txid: &[u8; 32]) -> bool {
            false
        }
        fn mempool_tx(&self, _txid: &[u8; 32]) -> Option<Transaction> {
            None
        }
        fn mempool_txs(&self) -> Vec<Transaction> {
            Vec::new()
        }
        fn block_by_hash(&self, _block_hash: &str) -> Option<Block> {
            None
        }
        fn save_state(&self) {}
        fn chain_contains_block(&self, _block_hash: &str) -> bool {
            false
        }
        fn snapshot_wire(&self) -> strangecoin_core::types::ChainSnapshot {
            unreachable!("shutdown test never snapshots")
        }
        fn headers_from_height(
            &self,
            _from_height: u64,
            _max: usize,
        ) -> Vec<strangecoin_core::types::BlockHeader> {
            Vec::new()
        }
        fn blocks_by_hashes(&self, _hashes: &[[u8; 32]], _max: usize) -> Vec<Block> {
            Vec::new()
        }
    }

    struct NoopSink;

    impl EventSink for NoopSink {
        fn block_applied(&self, _height: u64, _hash: String) {}
        fn block_reorged(&self, _old_tip: String, _new_tip: String) {}
        fn tx_accepted(&self, _txid: String) {}
        fn tx_rejected(&self, _txid: String, _reason: String) {}
        fn state_persisted(&self, _height: u64) {}
    }

    /// S2-P19 (КГ «tokio runtime properly shut down»): the engine task exits
    /// within a bounded time after the shutdown flag flips — no hung task
    /// keeps the runtime alive.
    #[tokio::test]
    async fn run_stops_cleanly_on_shutdown_flag() {
        let (headers_tx, headers_rx) = tokio::sync::mpsc::channel(HEADERS_INBOX_CAP);
        let (blocks_tx, blocks_rx) = tokio::sync::mpsc::channel(BLOCKS_INBOX_CAP);
        let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let engine = SyncEngine {
            headers_rx,
            blocks_rx,
            facade: Arc::new(NoopView),
            bus: Arc::new(NoopSink),
            sync_tx,
            shutdown: Arc::clone(&shutdown),
        };
        let handle = tokio::spawn(engine.run());
        // Let the engine reach its select loop, then request shutdown.
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutdown.store(true, Ordering::Relaxed);
        let joined = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("engine task must finish within 2s of shutdown");
        assert!(joined.is_ok(), "engine task must not panic");
        // Keep the senders alive until after the join so the loop saw open
        // lanes while running.
        drop((headers_tx, blocks_tx));
    }
}

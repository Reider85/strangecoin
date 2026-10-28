//! S2-P19 — the async connection read loop: one task per live peer session.
//!
//! The loop claims the parked socket read half of a registered
//! [`Connection`] ([`Connection::take_read_half`]), then for every inbound
//! wire frame:
//!
//! 1. decrypt it through the session's Noise transport (`decrypt_frame`,
//!    length-checked before allocation — invariant #16);
//! 2. charge the ADR-0023 rate limiter (per-message-type sub-buckets where
//!    they exist, legacy sync classes for `GET_HEADERS`/`GET_BLOCKS`, the
//!    main bucket otherwise) and apply verdicts through
//!    [`PeerManager::apply_rate_verdict`] (S2-P16 production wiring);
//! 3. dispatch:
//!    * `GET_HEADERS` / `GET_BLOCKS` — served from read-only
//!      [`BlockchainView`] calls (the S2-P18 sync contract);
//!    * gossip `INV`/`GET_DATA`/`TX`/`BLOCKS`/`NOTFOUND`
//!      (`handle_inv` / `handle_get_data`, ADR-0010: inbound data is
//!      *enqueued* into the [`Inbox`], never applied here);
//!    * compact blocks `COMPACT_BLOCK`/`GET_BLOCK_TXN`/`BLOCK_TXN` (S2-P14
//!      live wiring; the announce-side gate on `Capability::CompactBlocks`
//!      lives in the node's announcer);
//!    * Erlay `ERLAY_RECONCILE` sketches (S2-P15 live wiring, gated on
//!      `Capability::Erlay`);
//!    * unknown future tags — log + ignore (S2-P18 forward-compat, never a
//!      crash and never a ban).
//!
//! Clean exit: the loop breaks on shutdown flag, socket EOF, read timeout
//! ([`IDLE_READ_TIMEOUT`] — long-lived gossip sessions must outlive the
//! 10 s sync-exchange budget) or a rate-limit `Ban` verdict; on exit the
//! session is unregistered and the per-peer gossip/reconciliation state is
//! dropped.
//!
//! **Determinism** (Stage 2 checklist): the loop reads no clock except
//! timeout budgets (parameters) and the injected `Instant` edge of
//! `reconcile_at`/`due_at`; it uses no RNG (handshake keys come from the
//! SeedResolver); per-peer state maps are only point-accessed, never
//! iterated for output. Inbound dispatch order is wire order — arrival
//! order, not wall-clock order.

use super::manager::PeerManager;
use super::pool::Connection;
use crate::blockchain_view::BlockchainView;
use crate::error::NetError;
use crate::gossip::compact_blocks::{
    apply_block_txn, fill_up, handle_get_block_txn, CompactBlock, FillOutcome, GetBlockTxn,
};
use crate::gossip::erlay::fallback::fallback_full_relay;
use crate::gossip::erlay::reconcile::ReconciliationState;
use crate::gossip::erlay::ErlayReconcile;
use crate::gossip::inv::{Inv, InventoryItem, MAX_INV_BATCH};
use crate::gossip::{handle_get_data, handle_inv, GossipMessage, GossipState};
use crate::noise::NodeId;
use crate::protocol::hello::Capability;
use crate::protocol::{
    decode_message, decrypt_frame, encode_blocks, encode_headers, read_length_prefixed_async,
    InboundMessage, MAX_BLOCKS_BATCH, MAX_HEADERS_BATCH, MSG_GET_BLOCKS, MSG_GET_HEADERS,
};
use crate::rate_limiter::{MessageClass, MessageType, RateLimitError, RateLimiter};
use crate::sync_engine::{Inbox, Incoming};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::tcp::OwnedReadHalf;
use tokio::time::{interval, timeout, MissedTickBehavior};
use tracing::{debug, info, warn};

/// Budget for one idle read on a live session: long enough that a quiet
/// gossip peer is not dropped every few seconds, short enough that a dead
/// socket is noticed promptly.
pub const IDLE_READ_TIMEOUT: Duration = Duration::from_secs(300);
/// Poll cadence of the Erlay round trigger inside the loop.
const RECONCILE_POLL: Duration = Duration::from_secs(5);
/// Shutdown-flag observation cadence.
const SHUTDOWN_POLL: Duration = Duration::from_millis(100);
/// Read budget for the first frame after registration (handshake already
/// proved liveness; this only bounds a peer that connects and goes silent).
const FIRST_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything the connection loop needs from the node, bundled once per
/// session (all `Arc`s — cheap to clone into the spawned task).
pub struct ConnectionContext {
    pub peer_manager: Arc<PeerManager>,
    pub blockchain: Arc<dyn BlockchainView>,
    pub inbox: Inbox,
    pub gossip: Arc<Mutex<GossipState>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub reconciliation: Arc<Mutex<ReconciliationState>>,
    pub shutdown: Arc<AtomicBool>,
}

/// Drive one registered connection until shutdown, EOF, timeout or ban.
///
/// Claims the parked read half; a connection whose half was already claimed
/// (or never parked) returns immediately — one loop per session, never two
/// readers on one Noise transport (nonces must be consumed in order).
pub async fn run_connection(conn: Arc<Connection>, ctx: ConnectionContext) {
    let node_id = conn.node_id();
    let peer_addr = conn.peer_addr();
    let capabilities = conn.capabilities();
    let Some(mut read) = conn.take_read_half() else {
        warn!(
            target = "net::conn",
            peer = %node_id,
            "connection has no read half (already claimed?) — loop not started"
        );
        return;
    };
    info!(
        target = "net::conn",
        peer = %node_id,
        addr = %peer_addr,
        capabilities,
        "connection loop started"
    );

    // Compact blocks: keep the last compact message per hash so an arriving
    // BLOCK_TXN can complete it (bounded — only in-flight blocks).
    let mut pending_compact: HashMap<[u8; 32], (CompactBlock, GetBlockTxn)> = HashMap::new();
    let mut first_frame = true;

    let mut shutdown_tick = interval(SHUTDOWN_POLL);
    shutdown_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut reconcile_tick = interval(RECONCILE_POLL);
    reconcile_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        if ctx.shutdown.load(Ordering::Relaxed) {
            break;
        }
        let read_budget = if first_frame {
            FIRST_READ_TIMEOUT
        } else {
            IDLE_READ_TIMEOUT
        };
        tokio::select! {
            biased;
            _ = shutdown_tick.tick() => {}
            frame = read_encrypted_frame(&mut read, &conn, read_budget) => {
                first_frame = false;
                let raw = match frame {
                    Ok(raw) => raw,
                    Err(err) => {
                        debug!(
                            target: "net::conn",
                            peer = %node_id,
                            error = %err,
                            "connection read ended"
                        );
                        break;
                    }
                };
                if !dispatch_frame(&ctx, &conn, &raw, &mut pending_compact) {
                    break;
                }
            }
            _ = reconcile_tick.tick() => {
                maybe_start_reconciliation(&ctx, &conn, &mut reconcile_tick);
            }
        }
    }

    ctx.peer_manager.disconnect_peer(&node_id);
    ctx.gossip
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .forget_peer(&node_id);
    ctx.reconciliation
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .forget_peer(&node_id);
    info!(target = "net::conn", peer = %node_id, "connection loop stopped");
}

/// Read one length-prefixed frame and decrypt it. The async reader strips
/// the 4-byte length prefix; `decrypt_frame` wants it back (same trick as
/// the manager's HELLO reader).
async fn read_encrypted_frame(
    read: &mut OwnedReadHalf,
    conn: &Connection,
    budget: Duration,
) -> Result<Vec<u8>, NetError> {
    let body = timeout(budget, read_length_prefixed_async(read))
        .await
        .map_err(|_| NetError::Transport("connection read timeout".to_string()))??;
    let mut raw = Vec::with_capacity(4 + body.len());
    raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
    raw.extend_from_slice(&body);
    let mut transport = conn.transport_mutex();
    decrypt_frame(&mut transport, &raw)
}

/// Handle one decrypted inbound payload. Returns `false` when the loop must
/// close the session.
fn dispatch_frame(
    ctx: &ConnectionContext,
    conn: &Connection,
    payload: &[u8],
    pending_compact: &mut HashMap<[u8; 32], (CompactBlock, GetBlockTxn)>,
) -> bool {
    let node_id = conn.node_id();
    let peer_addr = conn.peer_addr();
    let tag = payload.first().copied();

    // ADR-0023 (S2-P16 production wiring): charge the limiter before any
    // parsing. Sync classes ride their legacy sub-buckets; gossip types
    // their dedicated ones; everything else the main bucket.
    let verdict = match tag {
        Some(MSG_GET_HEADERS) => ctx
            .rate_limiter
            .check_class(peer_addr, MessageClass::GetHeaders),
        Some(MSG_GET_BLOCKS) => ctx
            .rate_limiter
            .check_class(peer_addr, MessageClass::GetBlocks),
        Some(t) => match message_type(t) {
            Some(msg_type) => ctx.rate_limiter.check_rate(peer_addr, msg_type),
            None => ctx.rate_limiter.check(peer_addr),
        },
        None => ctx.rate_limiter.check(peer_addr),
    };
    match verdict {
        Ok(()) => {}
        Err(RateLimitError::Exceeded) => {
            warn!(
                target: "net::conn",
                peer = %node_id,
                ?tag,
                "rate limit exceeded — message dropped"
            );
            ctx.peer_manager.apply_rate_verdict(
                node_id,
                &verdict,
                ctx.rate_limiter.config().ban_duration,
            );
            return true;
        }
        Err(RateLimitError::Ban) => {
            warn!(
                target = "net::conn",
                peer = %node_id,
                ?tag,
                "rate limit ban — closing connection"
            );
            ctx.peer_manager.apply_rate_verdict(
                node_id,
                &verdict,
                ctx.rate_limiter.config().ban_duration,
            );
            return false;
        }
    }

    match decode_message(payload) {
        Ok(InboundMessage::GetHeaders(from_height)) => {
            let headers = ctx
                .blockchain
                .headers_from_height(from_height, MAX_HEADERS_BATCH);
            match encode_headers(&headers) {
                Ok(out) => send_or_log(ctx, node_id, &out),
                Err(err) => {
                    warn!(target = "net::conn", peer = %node_id, error = %err, "headers encode failed");
                }
            }
            true
        }
        Ok(InboundMessage::GetBlocks(hashes)) => {
            let blocks = ctx.blockchain.blocks_by_hashes(&hashes, MAX_BLOCKS_BATCH);
            match encode_blocks(&blocks) {
                Ok(out) => send_or_log(ctx, node_id, &out),
                Err(err) => {
                    warn!(target = "net::conn", peer = %node_id, error = %err, "blocks encode failed");
                }
            }
            true
        }
        Ok(InboundMessage::KnownTag(_tag)) => dispatch_gossip(ctx, conn, payload, pending_compact),
        Ok(InboundMessage::UnknownMessage(bytes)) => {
            debug!(
                target = "net::conn",
                peer = %node_id,
                len = bytes.len(),
                tag = ?bytes.first(),
                "unknown message type — ignored (forward-compat, S2-P18)"
            );
            true
        }
        Err(err) => {
            warn!(
                target = "net::conn",
                peer = %node_id,
                error = %err,
                "malformed known request — protocol violation (closing)"
            );
            ctx.peer_manager.record_protocol_violation(node_id);
            false
        }
    }
}

/// Gossip-layer dispatch for every known non-sync tag.
fn dispatch_gossip(
    ctx: &ConnectionContext,
    conn: &Connection,
    payload: &[u8],
    pending_compact: &mut HashMap<[u8; 32], (CompactBlock, GetBlockTxn)>,
) -> bool {
    let node_id = conn.node_id();
    let peer_addr = conn.peer_addr();
    let message = match GossipMessage::parse(payload) {
        Ok(message) => message,
        Err(err) => {
            warn!(
                target = "net::conn",
                peer = %node_id,
                error = %err,
                "malformed gossip message — protocol violation"
            );
            ctx.peer_manager.record_protocol_violation(node_id);
            return true;
        }
    };

    match message {
        GossipMessage::Inv(inv) => {
            let responses = {
                let mut gossip = ctx.gossip.lock().unwrap_or_else(|p| p.into_inner());
                handle_inv(&inv, node_id, &mut gossip, ctx.blockchain.as_ref())
            };
            match responses {
                Ok(messages) => send_gossip_messages(ctx, node_id, &messages),
                Err(err) => {
                    warn!(target = "net::conn", peer = %node_id, error = %err, "INV handling failed");
                }
            }
        }
        GossipMessage::GetData(get_data) => {
            let responses = {
                let mut gossip = ctx.gossip.lock().unwrap_or_else(|p| p.into_inner());
                handle_get_data(&get_data, node_id, &mut gossip, ctx.blockchain.as_ref())
            };
            send_gossip_messages(ctx, node_id, &responses);
        }
        GossipMessage::Tx(tx) => {
            let txid = strangecoin_core::serialize::txid(&tx);
            let queued = ctx.inbox.push_inbound(Incoming::NewTx {
                tx,
                from: Some(peer_addr),
            });
            if queued {
                ctx.reconciliation
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .note_new_tx(txid);
            }
        }
        GossipMessage::Block(block) => {
            ctx.inbox.push_inbound(Incoming::NewBlock {
                block,
                from: Some(peer_addr),
            });
        }
        GossipMessage::NotFound(not_found) => {
            debug!(
                target = "net::conn",
                peer = %node_id,
                items = not_found.items.len(),
                "NOTFOUND received"
            );
        }
        GossipMessage::CompactBlock(compact) => match fill_up(&compact, ctx.blockchain.as_ref()) {
            Ok(FillOutcome::Complete(block)) => {
                ctx.inbox.push_inbound(Incoming::NewBlock {
                    block,
                    from: Some(peer_addr),
                });
            }
            Ok(FillOutcome::Missing(request)) => {
                match request.encode() {
                    Ok(out) => send_or_log(ctx, node_id, &out),
                    Err(err) => warn!(
                        target = "net::conn",
                        peer = %node_id,
                        error = %err,
                        "GET_BLOCK_TXN encode failed"
                    ),
                }
                let hash = strangecoin_core::serialize::header_hash(&compact.header);
                pending_compact.insert(hash, (compact, request));
            }
            Err(err) => {
                warn!(
                    target = "net::conn",
                    peer = %node_id,
                    error = %err,
                    "compact block fill-up failed — protocol violation"
                );
                ctx.peer_manager.record_protocol_violation(node_id);
            }
        },
        GossipMessage::GetBlockTxn(request) => {
            if let Some(response) = handle_get_block_txn(&request, ctx.blockchain.as_ref()) {
                match response.encode() {
                    Ok(out) => send_or_log(ctx, node_id, &out),
                    Err(err) => warn!(
                        target = "net::conn",
                        peer = %node_id,
                        error = %err,
                        "BLOCK_TXN encode failed"
                    ),
                }
            }
        }
        GossipMessage::BlockTxn(txn) => {
            if let Some((compact, request)) = pending_compact.remove(&txn.block_hash) {
                match apply_block_txn(
                    &compact,
                    &request.missing_indices,
                    &txn,
                    ctx.blockchain.as_ref(),
                ) {
                    Ok(block) => {
                        ctx.inbox.push_inbound(Incoming::NewBlock {
                            block,
                            from: Some(peer_addr),
                        });
                    }
                    Err(err) => {
                        warn!(
                            target = "net::conn",
                            peer = %node_id,
                            error = %err,
                            "BLOCK_TXN completion failed — protocol violation"
                        );
                        ctx.peer_manager.record_protocol_violation(node_id);
                    }
                }
            } else {
                debug!(
                    target = "net::conn",
                    peer = %node_id,
                    "BLOCK_TXN without a pending compact block — ignored"
                );
            }
        }
        GossipMessage::ErlayReconcile(reconcile) => {
            handle_erlay_reconcile(ctx, node_id, &reconcile);
        }
    }
    true
}

/// Handle an inbound reconciliation sketch: decode the difference, announce
/// our exclusive half as INV (or fall back to full relay), and answer with
/// our sketch when the peer asked for one.
fn handle_erlay_reconcile(ctx: &ConnectionContext, node_id: NodeId, message: &ErlayReconcile) {
    let now = Instant::now();
    let (outcome, reply_sketch) = {
        let mut reconciliation = ctx.reconciliation.lock().unwrap_or_else(|p| p.into_inner());
        let outcome = reconciliation.reconcile_at(&node_id, &message.sketch, now);
        let reply_sketch = if message.respond {
            // `reconcile_at` already built and stored the local sketch.
            reconciliation
                .peer(&node_id)
                .map(|record| record.last_sketch.clone())
        } else {
            None
        };
        (outcome, reply_sketch)
    };
    match outcome {
        Ok(txids) => {
            // Lock discipline: resolve `knows` under the gossip lock, then
            // release it before any send (pool lock is inner — never hold
            // an outer lock across it).
            let mut fresh_batches: Vec<Vec<InventoryItem>> = Vec::new();
            {
                let gossip = ctx.gossip.lock().unwrap_or_else(|p| p.into_inner());
                for chunk in txids.chunks(MAX_INV_BATCH) {
                    let fresh: Vec<InventoryItem> = chunk
                        .iter()
                        .copied()
                        .map(InventoryItem::tx)
                        .filter(|item| !gossip.knows(&node_id, item))
                        .collect();
                    if !fresh.is_empty() {
                        fresh_batches.push(fresh);
                    }
                }
            }
            for fresh in fresh_batches {
                match (Inv { items: fresh }).encode() {
                    Ok(out) => send_or_log(ctx, node_id, &out),
                    Err(err) => warn!(
                        target = "net::conn",
                        peer = %node_id,
                        error = %err,
                        "erlay INV encode failed"
                    ),
                }
            }
        }
        Err(err) => {
            warn!(
                target = "net::conn",
                peer = %node_id,
                error = %err,
                "erlay reconcile decode failed — full-relay fallback"
            );
            let mut gossip = ctx.gossip.lock().unwrap_or_else(|p| p.into_inner());
            fallback_full_relay(
                &[],
                &node_id,
                &ctx.peer_manager,
                &mut gossip,
                ctx.blockchain.as_ref(),
            );
        }
    }
    if let Some(sketch) = reply_sketch {
        match (ErlayReconcile {
            respond: false,
            sketch,
        })
        .encode()
        {
            Ok(out) => send_or_log(ctx, node_id, &out),
            Err(err) => warn!(
                target = "net::conn",
                peer = %node_id,
                error = %err,
                "erlay reply encode failed"
            ),
        }
    }
}

/// Periodically initiate a reconciliation round with an Erlay-capable peer
/// when the shared trigger says so (ADR-0021: interval **or** new-tx count).
fn maybe_start_reconciliation(
    ctx: &ConnectionContext,
    conn: &Connection,
    tick: &mut tokio::time::Interval,
) {
    let node_id = conn.node_id();
    if conn.capabilities() & Capability::Erlay.bit() == 0 {
        return;
    }
    let now = Instant::now();
    let sketch = {
        let mut reconciliation = ctx.reconciliation.lock().unwrap_or_else(|p| p.into_inner());
        if !reconciliation.due_at(now) {
            return;
        }
        let sketch = reconciliation.local_sketch(&node_id);
        // Stamp at send: the peer's answer resets the trigger again via
        // `reconcile_at`; without the stamp a silent peer would make the
        // loop re-send every poll tick.
        reconciliation.mark_reconciled_at(now);
        sketch
    };
    match (ErlayReconcile {
        respond: true,
        sketch,
    })
    .encode()
    {
        Ok(out) => send_or_log(ctx, node_id, &out),
        Err(err) => warn!(
            target = "net::conn",
            peer = %node_id,
            error = %err,
            "erlay sketch encode failed"
        ),
    }
    // Reset the poll interval so the next check is one full poll away.
    tick.reset();
}

fn send_gossip_messages(ctx: &ConnectionContext, node_id: NodeId, messages: &[GossipMessage]) {
    for message in messages {
        match message.encode() {
            Ok(out) => send_or_log(ctx, node_id, &out),
            Err(err) => warn!(
                target = "net::conn",
                peer = %node_id,
                error = %err,
                "gossip response encode failed"
            ),
        }
    }
}

fn send_or_log(ctx: &ConnectionContext, node_id: NodeId, payload: &[u8]) {
    if let Err(err) = ctx.peer_manager.send_to_peer(&node_id, payload) {
        debug!(
            target = "net::conn",
            peer = %node_id,
            error = %err,
            "response send failed (peer may have disconnected)"
        );
    }
}

/// Map a wire tag onto its ADR-0023 [`MessageType`] sub-bucket (types with
/// no dedicated bucket return `None` → main bucket only).
fn message_type(tag: u8) -> Option<MessageType> {
    use crate::discovery::addr_gossip::MSG_ADDR;
    use crate::gossip::compact_blocks::{MSG_BLOCK_TXN, MSG_COMPACT_BLOCK, MSG_GET_BLOCK_TXN};
    use crate::gossip::erlay::MSG_ERLAY_RECONCILE;
    use crate::gossip::get_data::MSG_GET_DATA;
    use crate::gossip::inv::{MSG_INV, MSG_TX};
    use crate::protocol::MSG_BLOCKS;
    match tag {
        MSG_INV => Some(MessageType::Inv),
        MSG_TX => Some(MessageType::Tx),
        MSG_BLOCKS => Some(MessageType::Block),
        MSG_GET_DATA => Some(MessageType::GetData),
        MSG_COMPACT_BLOCK => Some(MessageType::CompactBlock),
        MSG_GET_BLOCK_TXN => Some(MessageType::GetBlockTxn),
        MSG_BLOCK_TXN => Some(MessageType::BlockTxn),
        MSG_ERLAY_RECONCILE => Some(MessageType::ErlayReconcile),
        MSG_ADDR => Some(MessageType::Addr),
        _ => None,
    }
}

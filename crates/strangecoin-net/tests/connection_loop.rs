//! S2-P19 — live connection-loop integration: Noise accept/dial over real
//! sockets, gossip INV→GET_DATA→TX through both loops, sync serving,
//! compact-block reconstruction, Erlay sketch exchange and the
//! rate-limit-ban close path.
//!
//! Every test spins two real `PeerManager`s (server + client) on localhost:
//! the server accepts through [`PeerManager::accept_inbound`] and drives
//! [`run_connection`]; the client dials with [`PeerManager::connect_to_peer`]
//! and runs its own loop. Assertions observe side effects (engine events,
//! store state, limiter bans) — never private frames.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;

use strangecoin_core::consensus::U256;
use strangecoin_core::serialize::{block_hash, header_hash, txid};
use strangecoin_core::types::{
    AccountState, Block, BlockHeader, ChainSnapshot, Transaction, TransferTx,
};
use strangecoin_net::gossip::compact_blocks::CompactBlock;
use strangecoin_net::protocol::hello::Hello;
use strangecoin_net::protocol::{
    decrypt_frame, encode_get_headers, encrypt_frame, parse_headers, read_length_prefixed_async,
    MSG_HEADERS,
};
use strangecoin_net::sync_engine::{self, EventSink};
use strangecoin_net::{
    run_connection, BlockchainView, BucketSpec, ConnectionContext, GossipMessage, GossipState, Inv,
    InventoryItem, NodeKeypair, NoiseHandshake, PeerManager, PeerManagerConfig, RateLimitConfig,
    RateLimiter, ReconciliationState,
};

// ------------------------------------------------------------------ helpers

/// Chain + mempool view: a two-block local chain plus an optional mempool.
/// Blocks applied through the engine land in `applied` (the trait is
/// `&self`, so mutation goes through a Mutex) — `tip_hash`/`block_by_hash`
/// reflect them so engine announcements carry real hashes.
#[derive(Default)]
struct TestView {
    blocks: Vec<Block>,
    mempool: HashMap<[u8; 32], Transaction>,
    applied: Mutex<Vec<Block>>,
}

impl TestView {
    fn with_chain(len: usize) -> Self {
        let mut blocks: Vec<Block> = Vec::new();
        for index in 0..len as u64 {
            let mut header = BlockHeader {
                index,
                timestamp: 1_700_000_000 + index,
                previous_hash: if index == 0 {
                    "0".repeat(64)
                } else {
                    blocks[(index - 1) as usize].hash.clone()
                },
                hash: String::new(),
                nonce: index,
                target: "ff".repeat(32),
                consensus_version: 1,
                state_root: [0u8; 32],
                tx_root: [0u8; 32],
                receipts_root: [0u8; 32],
            };
            header.hash = hex::encode(header_hash(&header));
            blocks.push(Block::from_header(header, Vec::new()));
        }
        Self {
            blocks,
            mempool: HashMap::new(),
            applied: Mutex::new(Vec::new()),
        }
    }

    fn all_blocks(&self) -> Vec<Block> {
        let mut blocks = self.blocks.clone();
        blocks.extend(self.applied.lock().unwrap().iter().cloned());
        blocks
    }
}

impl BlockchainView for TestView {
    fn tip_hash(&self) -> String {
        self.all_blocks()
            .last()
            .map(|b| b.hash.clone())
            .unwrap_or_default()
    }
    fn chain_len(&self) -> usize {
        self.all_blocks().len()
    }
    fn adopt_candidate(
        &self,
        _candidate_chain: Vec<Block>,
        _candidate_balances: Option<HashMap<String, AccountState>>,
        _candidate_mempool_txs: Vec<Transaction>,
        _candidate_difficulty: u32,
    ) -> Result<bool, String> {
        Ok(false)
    }
    fn apply_tx(&self, transaction: Transaction) -> Result<(), String> {
        let _ = transaction;
        Ok(())
    }
    fn add_block(&self, block: Block) -> Result<(), String> {
        self.applied.lock().unwrap().push(block);
        Ok(())
    }
    fn chain_contains_tx(&self, _tx: &Transaction) -> bool {
        false
    }
    fn mempool_contains(&self, txid: &[u8; 32]) -> bool {
        self.mempool.contains_key(txid)
    }
    fn mempool_tx(&self, txid: &[u8; 32]) -> Option<Transaction> {
        self.mempool.get(txid).cloned()
    }
    fn mempool_txs(&self) -> Vec<Transaction> {
        self.mempool.values().cloned().collect()
    }
    fn block_by_hash(&self, block_hash: &str) -> Option<Block> {
        self.all_blocks()
            .iter()
            .find(|b| b.hash == block_hash)
            .cloned()
    }
    fn save_state(&self) {}
    fn chain_contains_block(&self, block_hash: &str) -> bool {
        self.all_blocks().iter().any(|b| b.hash == block_hash)
    }
    fn snapshot_wire(&self) -> ChainSnapshot {
        ChainSnapshot {
            chain: self.blocks.clone(),
            balances: HashMap::new(),
            difficulty: 0,
            mempool_txs: self.mempool.values().cloned().collect(),
            total_work: U256::default(),
        }
    }
    fn headers_from_height(&self, from_height: u64, max: usize) -> Vec<BlockHeader> {
        let mut headers: Vec<BlockHeader> = self
            .blocks
            .iter()
            .filter(|b| b.index >= from_height)
            .map(|b| b.header())
            .collect();
        headers.sort_by_key(|h| h.index);
        headers.truncate(max);
        headers
    }
    fn blocks_by_hashes(&self, hashes: &[[u8; 32]], max: usize) -> Vec<Block> {
        let mut out = Vec::new();
        for hash in hashes {
            if out.len() >= max {
                break;
            }
            let want = hex::encode(hash);
            if let Some(block) = self.blocks.iter().find(|b| b.hash == want) {
                out.push(block.clone());
            }
        }
        out
    }
}

/// Engine event capture.
#[derive(Default)]
struct CaptureSink {
    tx_accepted: Mutex<Vec<String>>,
    block_applied: Mutex<Vec<String>>,
}

impl EventSink for CaptureSink {
    fn block_applied(&self, _height: u64, hash: String) {
        self.block_applied.lock().unwrap().push(hash);
    }
    fn block_reorged(&self, _old_tip: String, _new_tip: String) {}
    fn tx_accepted(&self, txid: String) {
        self.tx_accepted.lock().unwrap().push(txid);
    }
    fn tx_rejected(&self, _txid: String, _reason: String) {}
    fn state_persisted(&self, _height: u64) {}
}

fn sample_tx(nonce: u64) -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "rsc1sender".into(),
        to: "rsc1receiver".into(),
        amount: 7,
        fee: 0,
        nonce,
        chain_id: 3,
        signature: vec![1u8; 65],
        is_coinbase: false,
    })
}

fn manager(seed: u8) -> Arc<PeerManager> {
    let identity = NodeKeypair::from_seed([seed; 32]);
    let config = PeerManagerConfig::default();
    Arc::new(PeerManager::new(config, identity))
}

/// One node's shared wiring: manager + view + engine sink + loop context
/// handles. The engine is spawned so inbound TX/BLOCK messages are applied
/// exactly as production does (ADR-0010).
struct Node {
    pm: Arc<PeerManager>,
    view: Arc<TestView>,
    sink: Arc<CaptureSink>,
    gossip: Arc<Mutex<GossipState>>,
    recon: Arc<Mutex<ReconciliationState>>,
    rate: Arc<RateLimiter>,
    shutdown: Arc<AtomicBool>,
    inbox_shutdown: Arc<AtomicBool>,
    /// The SyncEngine producer handle — moved out by [`Node::context`] (one
    /// connection context per node in these tests).
    inbox: Option<strangecoin_net::sync_engine::Inbox>,
}

impl Node {
    fn new(seed: u8, view: TestView) -> Self {
        let pm = manager(seed);
        let view = Arc::new(view);
        let sink = Arc::new(CaptureSink::default());
        let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel();
        let inbox_shutdown = Arc::new(AtomicBool::new(false));
        let inbox = sync_engine::spawn(
            view.clone(),
            sink.clone(),
            sync_tx,
            Arc::clone(&inbox_shutdown),
            Arc::new(RateLimiter::default()),
        );
        Self {
            pm,
            view,
            sink,
            gossip: Arc::new(Mutex::new(GossipState::new())),
            recon: Arc::new(Mutex::new(ReconciliationState::with_defaults())),
            rate: Arc::new(RateLimiter::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
            inbox_shutdown,
            inbox: Some(inbox),
        }
    }

    fn context(&mut self) -> ConnectionContext {
        ConnectionContext {
            peer_manager: Arc::clone(&self.pm),
            blockchain: self.view.clone() as Arc<dyn BlockchainView>,
            inbox: self
                .inbox
                .take()
                .expect("one connection context per node in these tests"),
            gossip: Arc::clone(&self.gossip),
            rate_limiter: Arc::clone(&self.rate),
            reconciliation: Arc::clone(&self.recon),
            shutdown: Arc::clone(&self.shutdown),
        }
    }

    fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.inbox_shutdown.store(true, Ordering::Relaxed);
    }
}

async fn encrypted_send(
    transport: &mut strangecoin_net::NoiseTransport,
    stream: &mut tokio::net::TcpStream,
    payload: &[u8],
) {
    use tokio::io::AsyncWriteExt;
    let mut frame = Vec::new();
    encrypt_frame(transport, payload, &mut frame).expect("encrypt");
    stream.write_all(&frame).await.expect("write frame");
}

async fn encrypted_recv(
    transport: &mut strangecoin_net::NoiseTransport,
    stream: &mut tokio::net::TcpStream,
) -> Vec<u8> {
    let body = tokio::time::timeout(Duration::from_secs(5), read_length_prefixed_async(stream))
        .await
        .expect("frame arrives")
        .expect("frame reads");
    let mut raw = Vec::with_capacity(4 + body.len());
    raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
    raw.extend_from_slice(&body);
    decrypt_frame(transport, &raw).expect("decrypt")
}

/// Raw Noise XX initiator + encrypted HELLO against a listening
/// `accept_inbound` (used by the sync-serving test where no PeerManager
/// dial is needed).
async fn noise_client_session(
    stream: &mut tokio::net::TcpStream,
    identity: &NodeKeypair,
    network_id: u32,
) -> strangecoin_net::NoiseTransport {
    use strangecoin_net::protocol::hello::write_hello;
    let mut handshake =
        NoiseHandshake::initiator(&identity.noise_static_private_key(), identity, None)
            .expect("initiator");
    let mut buf = [0u8; 512];
    let mut payload = [0u8; 512];

    let n = handshake.write_message(&[], &mut buf).expect("r1 w");
    write_length(stream, &buf[..n]).await;
    let msg = read_length(stream).await;
    handshake.read_message(&msg, &mut payload).expect("r2 r");
    let n = handshake.write_message(&[], &mut buf).expect("r3 w");
    write_length(stream, &buf[..n]).await;

    let (mut transport, _peer) = handshake.into_transport().expect("transport");
    // Advertise Noise only: these raw client tests read exactly the frames
    // they ask for, so the server must not auto-send Erlay sketches (which
    // STAGE2_CAPABILITIES would enable).
    let hello = Hello::new(
        network_id,
        strangecoin_net::protocol::hello::Capability::Noise.bit(),
        0,
    );
    let mut out = Vec::new();
    write_hello(&mut transport, &hello, &mut out).expect("hello encode");
    stream.write_all(&out).await.expect("hello write");
    // Read the responder's HELLO.
    let _ = encrypted_recv(&mut transport, stream).await;
    transport
}

async fn write_length(stream: &mut tokio::net::TcpStream, bytes: &[u8]) {
    use tokio::io::AsyncWriteExt;
    let mut framed = Vec::with_capacity(4 + bytes.len());
    framed.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    framed.extend_from_slice(bytes);
    stream.write_all(&framed).await.expect("framed write");
}

async fn read_length(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(5), read_length_prefixed_async(stream))
        .await
        .expect("handshake frame arrives")
        .expect("handshake frame reads")
}

async fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

// -------------------------------------------------------------------- tests

/// Accept path: a raw Noise client dials the listener, `accept_inbound`
/// completes the responder handshake + HELLO, `run_connection` serves
/// GET_HEADERS from the view over the encrypted framing.
#[tokio::test(flavor = "multi_thread")]
async fn accept_loop_serves_get_headers_over_noise() {
    let mut node = Node::new(0xA1, TestView::with_chain(3));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let pm = Arc::clone(&node.pm);
    let ctx = node.context();
    let server = tokio::spawn(async move {
        let (stream, peer_addr) = listener.accept().await.expect("accept");
        let conn = pm
            .accept_inbound(stream, peer_addr)
            .await
            .expect("accept_inbound");
        run_connection(conn, ctx).await;
    });

    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut transport =
        noise_client_session(&mut stream, &NodeKeypair::from_seed([0xB1; 32]), 3).await;
    encrypted_send(&mut transport, &mut stream, &encode_get_headers(0)).await;
    let response = encrypted_recv(&mut transport, &mut stream).await;
    assert_eq!(response[0], MSG_HEADERS, "served a HEADERS reply");
    let headers = parse_headers(&response).expect("headers parse");
    assert_eq!(headers.len(), 3, "all three headers served");

    node.stop();
    let _ = server.await;
}

/// Gossip round trip through both live loops: client announces INV(tx) →
/// server answers GET_DATA → client serves TX → the server's SyncEngine
/// applies it (ADR-0010 inbox path) and the capture sink fires.
#[tokio::test(flavor = "multi_thread")]
async fn inv_get_data_tx_round_trip_via_connection_loops() {
    let tx = sample_tx(1);
    let txid_hex = hex::encode(txid(&tx));

    let mut client_view = TestView::default();
    client_view.mempool.insert(txid(&tx), tx.clone());
    let mut client = Node::new(0xC1, client_view);
    let mut server = Node::new(0xC2, TestView::default());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let pm = Arc::clone(&server.pm);
    let server_ctx = server.context();
    let server_task = tokio::spawn(async move {
        let (stream, peer_addr) = listener.accept().await.expect("accept");
        let conn = pm.accept_inbound(stream, peer_addr).await.expect("accept");
        run_connection(conn, server_ctx).await;
    });

    // Client dials through its manager and runs its own loop (it must serve
    // the GET_DATA the server will send back).
    client
        .pm
        .connect_to_peer(addr, None)
        .await
        .expect("dial succeeds");
    let server_node_id = client
        .pm
        .connected_peers()
        .first()
        .copied()
        .expect("client sees the server");
    let client_conn = client.pm.connection(&server_node_id).expect("conn");
    let client_ctx = client.context();
    let client_conn_for_loop = client_conn.clone();
    tokio::spawn(run_connection(client_conn_for_loop, client_ctx));

    // Announce INV(tx) to the server.
    let inv = Inv {
        items: vec![InventoryItem::tx(txid(&tx))],
    };
    let payload = GossipMessage::Inv(inv).encode().expect("inv encode");
    client
        .pm
        .send_to_peer(&server_node_id, &payload)
        .expect("send INV");

    let applied = wait_until(Duration::from_secs(5), || {
        server.sink.tx_accepted.lock().unwrap().contains(&txid_hex)
    })
    .await;
    assert!(
        applied,
        "server engine must apply the tx fetched via GET_DATA/TX; sink = {:?}",
        server.sink.tx_accepted.lock().unwrap()
    );

    client.stop();
    server.stop();
    let _ = server_task.await;
}

/// Sync serving of GET_BLOCKS over the encrypted loop.
#[tokio::test(flavor = "multi_thread")]
async fn accept_loop_serves_get_blocks_over_noise() {
    let mut node = Node::new(0xD1, TestView::with_chain(2));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let pm = Arc::clone(&node.pm);
    let ctx = node.context();
    let server = tokio::spawn(async move {
        let (stream, peer_addr) = listener.accept().await.expect("accept");
        let conn = pm
            .accept_inbound(stream, peer_addr)
            .await
            .expect("accept_inbound");
        run_connection(conn, ctx).await;
    });

    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut transport =
        noise_client_session(&mut stream, &NodeKeypair::from_seed([0xD2; 32]), 3).await;

    let tip_hash = node.view.blocks.last().expect("tip").hash.clone();
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&hex::decode(&tip_hash).expect("hex hash"));
    let request = strangecoin_net::protocol::encode_get_blocks(std::slice::from_ref(&hash_bytes))
        .expect("encode get_blocks");
    encrypted_send(&mut transport, &mut stream, &request).await;
    let response = encrypted_recv(&mut transport, &mut stream).await;
    let blocks = strangecoin_net::protocol::parse_blocks(&response).expect("blocks parse");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].hash, tip_hash, "served the requested block");
    // The hash must match its canonical content (serving path integrity).
    assert_eq!(hex::encode(block_hash(&blocks[0])), tip_hash);

    node.stop();
    let _ = server.await;
}

/// A COMPACT_BLOCK announced by the client reconstructs on the server from
/// its mempool (S2-P14 live wiring) and lands in the engine as a NewBlock.
#[tokio::test(flavor = "multi_thread")]
async fn compact_block_reconstructs_via_the_loop() {
    let tx1 = sample_tx(1);
    let tx2 = sample_tx(2);
    let coinbase = Transaction::Transfer(TransferTx {
        from: "rsc1miner".into(),
        to: "rsc1miner".into(),
        amount: 50,
        fee: 0,
        nonce: 0,
        chain_id: 3,
        signature: vec![1u8; 65],
        is_coinbase: true,
    });
    let mut block = Block {
        index: 1,
        timestamp: 1_700_000_001,
        transactions: vec![coinbase, tx1.clone(), tx2.clone()],
        previous_hash: "aa".repeat(32),
        hash: String::new(),
        nonce: 7,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [0u8; 32],
        tx_root: [0u8; 32],
        receipts_root: [0u8; 32],
    };
    // The compact-block fill-up verifies the merkle root over the
    // assembled transaction list — the sample block must commit to the real
    // root, not a placeholder.
    let txids: Vec<[u8; 32]> = block.transactions.iter().map(txid).collect();
    block.tx_root = strangecoin_core::serialize::merkle_root(&txids);
    block.hash = hex::encode(block_hash(&block));

    let mut server_view = TestView::default();
    server_view.mempool.insert(txid(&tx1), tx1.clone());
    server_view.mempool.insert(txid(&tx2), tx2.clone());
    let mut server = Node::new(0xE1, server_view);
    let client = Node::new(0xE2, TestView::default());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let pm = Arc::clone(&server.pm);
    let server_ctx = server.context();
    let server_task = tokio::spawn(async move {
        let (stream, peer_addr) = listener.accept().await.expect("accept");
        let conn = pm.accept_inbound(stream, peer_addr).await.expect("accept");
        run_connection(conn, server_ctx).await;
    });

    client.pm.connect_to_peer(addr, None).await.expect("dial");
    let server_node_id = client
        .pm
        .connected_peers()
        .first()
        .copied()
        .expect("connected");
    let payload = GossipMessage::CompactBlock(CompactBlock::from_block(&block))
        .encode()
        .expect("compact encode");
    client
        .pm
        .send_to_peer(&server_node_id, &payload)
        .expect("send compact block");

    let applied = wait_until(Duration::from_secs(5), || {
        server
            .sink
            .block_applied
            .lock()
            .unwrap()
            .contains(&block.hash)
    })
    .await;
    assert!(
        applied,
        "server engine must apply the reconstructed compact block; sink = {:?}",
        server.sink.block_applied.lock().unwrap()
    );

    client.stop();
    server.stop();
    let _ = server_task.await;
}

/// Both loops auto-run an Erlay round on the shared trigger; after the
/// exchange neither side is due anymore (the sketch/response pair
/// reconciled, S2-P15 live wiring).
#[tokio::test(flavor = "multi_thread")]
async fn erlay_sketch_round_completes_over_the_loops() {
    let mut client = Node::new(0xF1, TestView::default());
    let mut server = Node::new(0xF2, TestView::default());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let pm = Arc::clone(&server.pm);
    let server_ctx = server.context();
    let server_task = tokio::spawn(async move {
        let (stream, peer_addr) = listener.accept().await.expect("accept");
        let conn = pm.accept_inbound(stream, peer_addr).await.expect("accept");
        run_connection(conn, server_ctx).await;
    });

    client.pm.connect_to_peer(addr, None).await.expect("dial");
    let server_node_id = client
        .pm
        .connected_peers()
        .first()
        .copied()
        .expect("connected");
    let client_conn = client.pm.connection(&server_node_id).expect("conn");
    let client_ctx = client.context();
    tokio::spawn(run_connection(client_conn, client_ctx));

    let completed = wait_until(Duration::from_secs(5), || {
        let client_due = client.recon.lock().unwrap().due_at(Instant::now());
        let server_due = server.recon.lock().unwrap().due_at(Instant::now());
        !client_due && !server_due
    })
    .await;
    assert!(
        completed,
        "both sides must complete a reconciliation round over the wire"
    );

    client.stop();
    server.stop();
    let _ = server_task.await;
}

/// A junk-flooding peer crosses the ADR-0023 threshold: the loop closes the
/// session, `apply_rate_verdict` bans the authenticated node_id in the
/// peer store and the address-keyed limiter refuses further traffic.
#[tokio::test(flavor = "multi_thread")]
async fn junk_flood_bans_peer_and_closes_connection() {
    let client = Node::new(0x21, TestView::default());
    let server = Node::new(0x22, TestView::default());
    // Small budget so the flood trips the threshold quickly: main bucket
    // 4 tokens burst, 1/s refill; ban after 2 violations.
    let tight = RateLimiter::new(RateLimitConfig {
        main: BucketSpec::new(4, 1),
        ban_threshold: 2,
        ..RateLimitConfig::default()
    });
    let server_rate = Arc::new(tight);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let pm = Arc::clone(&server.pm);
    let ctx = ConnectionContext {
        peer_manager: Arc::clone(&server.pm),
        blockchain: server.view.clone() as Arc<dyn BlockchainView>,
        inbox: {
            // The flood never reaches the engine; an inbox is still required.
            let (sync_tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let shutdown = Arc::new(AtomicBool::new(false));
            sync_engine::spawn(
                server.view.clone(),
                server.sink.clone(),
                sync_tx,
                shutdown,
                Arc::new(RateLimiter::default()),
            )
        },
        gossip: Arc::clone(&server.gossip),
        rate_limiter: Arc::clone(&server_rate),
        reconciliation: Arc::clone(&server.recon),
        shutdown: Arc::clone(&server.shutdown),
    };
    let server_task = tokio::spawn(async move {
        let (stream, peer_addr) = listener.accept().await.expect("accept");
        let conn = pm.accept_inbound(stream, peer_addr).await.expect("accept");
        run_connection(conn, ctx).await;
    });

    client.pm.connect_to_peer(addr, None).await.expect("dial");
    let server_node_id = client
        .pm
        .connected_peers()
        .first()
        .copied()
        .expect("connected");
    let conn = client.pm.connection(&server_node_id).expect("conn");
    // The flooder's own identity — the one the server's verdicts target.
    let client_node_id = client.pm.local_node_id();

    // 30 junk payloads on the same connection: the main bucket refuses most
    // of them; the violation window trips the ban (threshold 2) and the
    // server loop breaks.
    let mut send_error = false;
    for _ in 0..30 {
        if conn.send_payload(b"PINGjunk").is_err() {
            send_error = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let banned = wait_until(Duration::from_secs(3), || {
        server.pm.is_banned(&client_node_id)
    })
    .await;
    assert!(
        banned || send_error,
        "server must ban the flooder (node_id ban or the closed socket \
         surfacing as a send error); send_error={send_error}"
    );
    assert!(
        server.pm.is_banned(&client_node_id),
        "apply_rate_verdict must ban the authenticated node_id"
    );
    // The limiter is keyed by the source address the *server* saw — the
    // peer store recorded it with the session.
    let client_seen_addr = server
        .pm
        .get_peer(&client_node_id)
        .expect("peer record")
        .addr;
    assert!(
        server_rate.check(client_seen_addr).is_err(),
        "the address-keyed limiter must refuse the banned address"
    );

    client.stop();
    server.stop();
    let _ = server_task.await;
}

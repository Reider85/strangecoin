//! S2-P13 — gossip protocol: INV/GET_DATA announce, dedup and propagation.
//!
//! The propagation budget test runs a full handler-level simulation (100
//! simulated peers, no sockets — prompt: «simulated clock, no real
//! network»); the announce test drives a real `PeerManager` over real
//! localhost sockets (repo test philosophy: no mocks where a connection is
//! involved) and decrypts the INV on the receiving side.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use strangecoin_core::consensus::U256;
use strangecoin_core::serialize::txid;
use strangecoin_core::types::{AccountState, Block, ChainSnapshot, Transaction, TransferTx};
use strangecoin_net::gossip::broadcast::{handle_get_data, handle_inv};
use strangecoin_net::protocol::decrypt_frame;
use strangecoin_net::protocol::read_length_prefixed_async;
use strangecoin_net::{
    announce_tx, Connection, GetData, GossipMessage, GossipState, Inv, InventoryItem, NodeId,
    NodeKeypair, NoiseHandshake, PeerManager, PeerManagerConfig,
};
use tokio::net::{TcpListener, TcpStream};

const NETWORK_ID: u32 = 3;
const HANDSHAKE_BUF: usize = 512;

// ------------------------------------------------------------------- helpers

fn sample_tx(nonce: u64) -> Transaction {
    Transaction::Transfer(TransferTx {
        from: "rsc1sender".into(),
        to: "rsc1receiver".into(),
        amount: 10,
        fee: 0,
        nonce,
        chain_id: 3,
        signature: vec![1u8; 65],
        is_coinbase: false,
    })
}

fn sample_block(tx: Transaction) -> Block {
    let mut block = Block {
        index: 5,
        timestamp: 1_700_000_005,
        transactions: vec![tx],
        previous_hash: "cc".repeat(32),
        hash: String::new(),
        nonce: 21,
        target: "ff".repeat(32),
        consensus_version: 1,
        state_root: [4u8; 32],
        tx_root: [5u8; 32],
        receipts_root: [0u8; 32],
    };
    block.hash = hex::encode(strangecoin_core::serialize::block_hash(&block));
    block
}

fn node_id(seed: u8) -> NodeId {
    NodeKeypair::from_seed([seed; 32]).node_id()
}

/// In-memory `BlockchainView` for the gossip handlers: a HashMap mempool
/// plus a HashMap chain. Unused sync methods return trivial values.
#[derive(Default)]
struct TestView {
    mempool: HashMap<[u8; 32], Transaction>,
    blocks: HashMap<String, Block>,
}

impl strangecoin_net::BlockchainView for TestView {
    fn tip_hash(&self) -> String {
        self.blocks
            .values()
            .max_by_key(|b| b.index)
            .map(|b| b.hash.clone())
            .unwrap_or_default()
    }

    fn chain_len(&self) -> usize {
        self.blocks.len()
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

    fn apply_tx(&self, _transaction: Transaction) -> Result<(), String> {
        Ok(())
    }

    fn add_block(&self, _block: Block) -> Result<(), String> {
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
        self.blocks.get(block_hash).cloned()
    }

    fn save_state(&self) {}

    fn chain_contains_block(&self, block_hash: &str) -> bool {
        self.blocks.contains_key(block_hash)
    }

    fn snapshot_wire(&self) -> ChainSnapshot {
        ChainSnapshot {
            chain: Vec::new(),
            balances: HashMap::new(),
            difficulty: 0,
            mempool_txs: self.mempool.values().cloned().collect(),
            total_work: U256::default(),
        }
    }
}

fn parse_gossip(payload: &[u8]) -> GossipMessage {
    GossipMessage::parse(payload).expect("gossip payload parses")
}

fn expect_get_data(msg: GossipMessage) -> GetData {
    match msg {
        GossipMessage::GetData(get_data) => get_data,
        other => panic!("expected GET_DATA, got {other:?}"),
    }
}

// ---------------------------------------------------- 100-peer propagation

#[test]
fn propagates_tx_to_100_simulated_peers_within_one_second() {
    let tx = sample_tx(1);
    let id = txid(&tx);

    // The origin node has the tx pending in its mempool.
    let mut origin = TestView::default();
    origin.mempool.insert(id, tx.clone());
    let origin_id = node_id(0x00);

    let wire_inv = GossipMessage::Inv(Inv {
        items: vec![InventoryItem::tx(id)],
    })
    .encode()
    .unwrap();

    let started = Instant::now();
    for i in 0..100u8 {
        let peer = node_id(i + 1);
        let mut peer_state = GossipState::new();
        let mut peer_view = TestView::default();

        // 1) the peer receives the INV and requests the tx it lacks
        //    (checked against the *peer's* view, which is empty).
        let GossipMessage::Inv(inv) = parse_gossip(&wire_inv) else {
            panic!("wire payload is INV");
        };
        let requests =
            handle_inv(&inv, origin_id, &mut peer_state, &peer_view).expect("handle_inv");
        assert_eq!(requests.len(), 1, "peer {i} requests the tx");
        let wire_get_data = requests[0].encode().unwrap();

        // 2) the origin serves the GET_DATA from its mempool.
        let get_data = expect_get_data(parse_gossip(&wire_get_data));
        let mut origin_state = GossipState::new();
        let replies = handle_get_data(&get_data, peer, &mut origin_state, &origin);
        assert_eq!(replies.len(), 1, "origin answers exactly one TX");
        let wire_tx = replies[0].encode().unwrap();

        // 3) the peer receives the TX and accepts it into its mempool.
        let GossipMessage::Tx(received) = parse_gossip(&wire_tx) else {
            panic!("reply is TX");
        };
        assert_eq!(txid(&received), id);
        peer_view.mempool.insert(id, received);
        assert!(
            strangecoin_net::BlockchainView::mempool_contains(&peer_view, &id),
            "peer {i} has the tx"
        );

        // The peer marked the origin as known-to-have the tx (no re-INV).
        assert!(peer_state.knows(&origin_id, &InventoryItem::tx(id)));
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed <= Duration::from_secs(1),
        "100-peer propagation took {elapsed:?}"
    );
}

// ----------------------------------------------------------- INV dedup

#[test]
fn inv_dedup_peer_does_not_request_known_tx() {
    let tx = sample_tx(2);
    let id = txid(&tx);

    let mut view = TestView::default();
    view.mempool.insert(id, tx);

    let inv = Inv {
        items: vec![InventoryItem::tx(id), InventoryItem::tx([0x77; 32])],
    };
    let peer = node_id(2);
    let mut state = GossipState::new();
    let responses = handle_inv(&inv, peer, &mut state, &view).expect("handle_inv");

    // Only the unknown item is requested; the known tx is deduplicated.
    let get_data = expect_get_data(responses.into_iter().next().expect("GET_DATA"));
    assert_eq!(get_data.items, vec![InventoryItem::tx([0x77; 32])]);
    // Announcing marked both items known-for-peer on the receiver side.
    assert!(state.knows(&peer, &InventoryItem::tx(id)));
    assert!(state.knows(&peer, &InventoryItem::tx([0x77; 32])));
}

#[test]
fn inv_dedup_announce_skips_peers_that_already_know() {
    // Pure known-set check: with no connected peers the announce fan-out is
    // zero, and a marked peer is filtered before any send attempt.
    let manager = manager_with(|_| {});
    let known_peer = node_id(3);
    let txid = [0x31u8; 32];

    let mut state = GossipState::new();
    state.mark_known(known_peer, InventoryItem::tx(txid));
    assert!(state.knows(&known_peer, &InventoryItem::tx(txid)));

    // No connected peers → nothing sent, but the known-set filter itself is
    // exercised by the real-socket announce test below.
    assert_eq!(announce_tx(txid, &manager, &state), 0);
}

// ------------------------------------------------------- GET_DATA responses

#[test]
fn get_data_known_tx_yields_tx_and_known_block_yields_block() {
    let tx = sample_tx(3);
    let id = txid(&tx);
    let block = sample_block(sample_tx(4));
    let block_hash_bytes: [u8; 32] = {
        let decoded = hex::decode(&block.hash).expect("hex hash");
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&decoded);
        hash
    };

    let mut view = TestView::default();
    view.mempool.insert(id, tx);
    view.blocks.insert(block.hash.clone(), block.clone());

    let get_data = GetData {
        items: vec![
            InventoryItem::tx(id),
            InventoryItem::block(block_hash_bytes),
        ],
    };
    let peer = node_id(4);
    let mut state = GossipState::new();
    let replies = handle_get_data(&get_data, peer, &mut state, &view);
    assert_eq!(replies.len(), 2, "TX + BLOCK, no NOTFOUND");
    match &replies[0] {
        GossipMessage::Tx(reply_tx) => assert_eq!(txid(reply_tx), id),
        other => panic!("expected TX, got {other:?}"),
    }
    match &replies[1] {
        GossipMessage::Block(reply_block) => {
            assert_eq!(reply_block.hash, block.hash);
            assert_eq!(reply_block.index, block.index);
        }
        other => panic!("expected BLOCK, got {other:?}"),
    }
    // Serving marked the objects known-for-peer (no re-announce needed).
    assert!(state.knows(&peer, &InventoryItem::tx(id)));
    assert!(state.knows(&peer, &InventoryItem::block(block_hash_bytes)));
}

#[test]
fn get_data_unknown_items_yield_single_notfound() {
    let view = TestView::default();
    let get_data = GetData {
        items: vec![
            InventoryItem::tx([0x01; 32]),
            InventoryItem::block([0x02; 32]),
        ],
    };
    let peer = node_id(5);
    let mut state = GossipState::new();
    let replies = handle_get_data(&get_data, peer, &mut state, &view);
    assert_eq!(replies.len(), 1, "one NOTFOUND lumping both items");
    match &replies[0] {
        GossipMessage::NotFound(not_found) => assert_eq!(not_found.items, get_data.items),
        other => panic!("expected NOTFOUND, got {other:?}"),
    }
    // Nothing was served, so nothing was marked known.
    assert!(!state.knows(&peer, &get_data.items[0]));
}

// ------------------------------------------------- real-socket announce

fn manager_with(overrides: impl FnOnce(&mut PeerManagerConfig)) -> PeerManager {
    let mut config = PeerManagerConfig {
        network_id: NETWORK_ID,
        ..PeerManagerConfig::default()
    };
    overrides(&mut config);
    PeerManager::new(config, NodeKeypair::from_seed([0xC1; 32]))
}

fn addr(last: u8) -> SocketAddr {
    SocketAddr::new(IpAddr::from([10, 9, 0, last]), 8081)
}

/// A finished in-memory XX handshake — both transports plus the identities
/// they authenticate (same helper shape as `tests/peer_management.rs`).
fn transport_pair(
    identity_a: [u8; 32],
    identity_b: [u8; 32],
) -> (
    strangecoin_net::NoiseTransport,
    strangecoin_net::NoiseTransport,
    NodeId,
) {
    let mut init =
        NoiseHandshake::initiator(&[0x11; 32], &NodeKeypair::from_seed(identity_a), None)
            .expect("initiator");
    let mut resp =
        NoiseHandshake::responder(&[0x22; 32], &NodeKeypair::from_seed(identity_b), None)
            .expect("responder");
    let mut msg = [0u8; HANDSHAKE_BUF];
    let mut payload = [0u8; HANDSHAKE_BUF];
    let n1 = init.write_message(&[], &mut msg).expect("r1 w");
    resp.read_message(&msg[..n1], &mut payload).expect("r1 r");
    let n2 = resp.write_message(&[], &mut msg).expect("r2 w");
    init.read_message(&msg[..n2], &mut payload).expect("r2 r");
    let n3 = init.write_message(&[], &mut msg).expect("r3 w");
    resp.read_message(&msg[..n3], &mut payload).expect("r3 r");
    let (transport_a, _) = init.into_transport().expect("init transport");
    let (transport_b, peer_b) = resp.into_transport().expect("resp transport");
    (transport_a, transport_b, peer_b)
}

async fn local_stream_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let dial = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(dial).await.expect("connect");
    let (server, _) = listener.accept().await.expect("accept");
    (client, server)
}

/// Pool-ready connection for `node_seed` plus the responder-side transport
/// and socket needed to decrypt what the manager sends.
async fn make_connection(
    node_seed: u8,
    peer_addr: SocketAddr,
) -> (Arc<Connection>, strangecoin_net::NoiseTransport, TcpStream) {
    let (transport_a, transport_b, peer_b) = transport_pair([node_seed; 32], [0xEE; 32]);
    let (client, server) = local_stream_pair().await;
    let connection = Arc::new(Connection::new(
        transport_b,
        client,
        peer_addr,
        peer_b,
        strangecoin_net::protocol::hello::STAGE2_CAPABILITIES,
    ));
    (connection, transport_a, server)
}

/// Read one encrypted frame off `stream` and decrypt it into plaintext.
async fn recv_payload(
    transport: &mut strangecoin_net::NoiseTransport,
    stream: &mut TcpStream,
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

#[tokio::test(flavor = "multi_thread")]
async fn announce_broadcasts_inv_over_real_sockets_and_skips_known_peers() {
    let manager = manager_with(|config| {
        config.max_connections = 8;
        config.max_per_ip_range = 8;
    });

    let (conn_a, mut transport_a, mut server_a) = make_connection(0xA1, addr(1)).await;
    let (conn_b, mut transport_b, mut server_b) = make_connection(0xB2, addr(2)).await;
    let id_a = conn_a.node_id();
    let id_b = conn_b.node_id();
    manager.register_connection(conn_a).expect("peer a");
    manager.register_connection(conn_b).expect("peer b");

    let tx = sample_tx(5);
    let id = txid(&tx);

    // First announce: both peers get the INV.
    let mut state = GossipState::new();
    assert_eq!(announce_tx(id, &manager, &state), 2);

    let payload_a = recv_payload(&mut transport_a, &mut server_a).await;
    let payload_b = recv_payload(&mut transport_b, &mut server_b).await;
    for payload in [&payload_a, &payload_b] {
        match parse_gossip(payload) {
            GossipMessage::Inv(inv) => {
                assert_eq!(inv.items, vec![InventoryItem::tx(id)]);
            }
            other => panic!("expected INV, got {other:?}"),
        }
    }

    // Mark peer A as knowing the tx (e.g. it announced it back): the second
    // announce reaches only peer B.
    state.mark_known(id_a, InventoryItem::tx(id));
    assert_eq!(announce_tx(id, &manager, &state), 1);
    let payload_b2 = recv_payload(&mut transport_b, &mut server_b).await;
    assert!(matches!(parse_gossip(&payload_b2), GossipMessage::Inv(_)));
    // Peer A receives nothing — the send is skipped before any socket write;
    // a zero-length probe would time out, so assert the filter directly.
    assert!(state.knows(&id_a, &InventoryItem::tx(id)));
    assert!(!state.knows(&id_b, &InventoryItem::tx(id)));
}

//! S2-P12 — peer management: store persistence, the ban state machine,
//! the per-IP-range Sybil cap and the outbound Noise+HELLO dial.
//!
//! Everything runs on real localhost TCP sockets where a connection is
//! involved (repo test philosophy: no mocks); store-level tests use a
//! simulated clock (`tick_at`) so a 1 h ban expires in zero wall time.

use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use strangecoin_net::discovery::addr_gossip::{AddrEntry, AddrSender};
use strangecoin_net::noise::{NodeKeypair, NoiseHandshake};
use strangecoin_net::peer::scoring::{self, ScoringConfig};
use strangecoin_net::protocol::decrypt_frame;
use strangecoin_net::protocol::hello::{read_hello, write_hello, Hello, STAGE2_CAPABILITIES};
use strangecoin_net::protocol::read_length_prefixed_async;
use strangecoin_net::{
    BanReason, Connection, ConnectionPool, NetError, NodeId, PeerManager, PeerManagerConfig,
    PeerStore,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

const NETWORK_ID: u32 = 3;
const HANDSHAKE_BUF: usize = 512;

// ------------------------------------------------------------------ helpers

fn node_id(seed: u8) -> NodeId {
    NodeKeypair::from_seed([seed; 32]).node_id()
}

fn addr(last: u8) -> SocketAddr {
    SocketAddr::new(IpAddr::from([10, 0, 0, last]), 8081)
}

fn entry(seed: u8, last: u8) -> AddrEntry {
    AddrEntry {
        node_id: node_id(seed),
        addr: addr(last),
    }
}

/// Fresh per-test directory in the system temp dir (not `target/`).
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("strangecoin-s2p12-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn manager_with(
    overrides: impl FnOnce(&mut PeerManagerConfig),
    network_json: Option<PathBuf>,
) -> PeerManager {
    let mut config = PeerManagerConfig {
        network_id: NETWORK_ID,
        ..PeerManagerConfig::default()
    };
    config.network_json_path = network_json;
    overrides(&mut config);
    PeerManager::new(config, NodeKeypair::from_seed([0xA1; 32]))
}

/// A finished in-memory XX handshake — both transports plus the identities
/// they authenticate. Used to build [`Connection`]s for pool tests without
/// a network round trip (the dial path itself is covered by the e2e test).
fn transport_pair(
    identity_a: [u8; 32],
    identity_b: [u8; 32],
) -> (
    strangecoin_net::NoiseTransport,
    NodeId,
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
    let (transport_a, id_a) = init.into_transport().expect("init transport");
    let (transport_b, id_b) = resp.into_transport().expect("resp transport");
    (transport_a, id_a, transport_b, id_b)
}

/// Real localhost socket pair (client, server).
async fn local_stream_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let client = TcpStream::connect(addr).await.expect("connect");
    let (server, _) = listener.accept().await.expect("accept");
    (client, server)
}

/// A pool-ready [`Connection`] for `node_seed`, tagged with `peer_addr`
/// (the range metadata the per-IP cap counts against).
///
/// `into_transport` returns the *peer's* `node_id`, so the pair is taken
/// from the responder side: its authenticated peer is the initiator identity
/// `[node_seed; 32]` — distinct per seed, which is what the pool keys on.
async fn make_connection(node_seed: u8, peer_addr: SocketAddr) -> Arc<Connection> {
    let (_transport_a, _peer_a, transport_b, peer_b) = transport_pair([node_seed; 32], [0xEE; 32]);
    let (client, _server) = local_stream_pair().await;
    let (_read_half, write_half) = Connection::split_stream(client);
    Arc::new(Connection::new(
        transport_b,
        write_half,
        peer_addr,
        peer_b,
        STAGE2_CAPABILITIES,
    ))
}

// ------------------------------------------------------- store persistence

#[test]
fn peer_store_roundtrip_recovers_peers_and_preserves_other_keys() {
    let dir = temp_dir("store-roundtrip");
    let path = dir.join("network.json");
    // Pre-existing file: legacy `peers` strings + `seed_config` must survive
    // the read-modify-write.
    fs::write(
        &path,
        r#"{
            "peers": ["127.0.0.1:8081"],
            "seed_config": { "hard_coded_ips": ["203.0.113.10"] }
        }"#,
    )
    .unwrap();

    let now = Instant::now();
    let mut store = PeerStore::new();
    assert!(store.add_peer(node_id(1), addr(1), true, now));
    assert!(store.add_peer(node_id(2), addr(2), false, now), "new peer");
    {
        let record = store.get_peer_mut(&node_id(1)).expect("record");
        record.invalid_count = 3;
        record.latency_ewma = Duration::from_millis(50);
        scoring::update_score(record, &ScoringConfig::default(), now);
    }
    assert!(store.ban_peer(
        node_id(2),
        BanReason::InvalidBlocks,
        now + Duration::from_secs(3600),
        now,
        true,
    ));
    store.save_to_network_json(&path).expect("save");

    // The file grew the two new keys and kept the old ones byte-compatible.
    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        doc["peers"].as_array().unwrap().len(),
        1,
        "legacy untouched"
    );
    assert!(
        doc["seed_config"]["hard_coded_ips"].is_array(),
        "seed_config untouched"
    );
    assert_eq!(doc["known_peers"].as_array().unwrap().len(), 2);
    assert_eq!(doc["banned_peers"].as_array().unwrap().len(), 1);

    // "Restart": a fresh store recovers everything from disk.
    let mut loaded = PeerStore::new();
    loaded.load_from_network_json(&path).expect("load");
    assert_eq!(loaded.len(), 2, "legacy string peers are not peer records");
    let recovered = loaded.get_peer(&node_id(1)).expect("peer 1 recovered");
    assert_eq!(recovered.addr, addr(1));
    assert!(recovered.verified);
    assert_eq!(recovered.invalid_count, 3);
    assert_eq!(recovered.latency_ewma, Duration::from_millis(50));
    assert!(recovered.score < 1.0, "score persisted");
    let recovered2 = loaded.get_peer(&node_id(2)).expect("peer 2 recovered");
    assert!(!recovered2.verified, "unverified flag survives");
    assert!(loaded.is_banned(&node_id(2)), "ban survives restart");
    let ban = loaded.ban(&node_id(2)).expect("ban record");
    assert!(ban.persistent);
    assert_eq!(ban.reason, BanReason::InvalidBlocks);
    assert!(ban.until > Instant::now(), "remaining time preserved");

    // A ban whose remaining time ran out while the node was down is dropped.
    let expired_path = dir.join("expired.json");
    let expired_json = serde_json::json!({
        "known_peers": [],
        "banned_peers": [{
            "node_id": hex::encode(node_id(9).as_bytes()),
            "ban_remaining_secs": 0,
            "banned_at_age_secs": 10,
            "reason": "manual",
            "persistent": true
        }]
    });
    fs::write(&expired_path, expired_json.to_string()).unwrap();
    let mut fresh = PeerStore::new();
    fresh.load_from_network_json(&expired_path).expect("load");
    assert!(
        !fresh.is_banned(&node_id(9)),
        "expired ban does not resurrect"
    );
}

// ------------------------------------------------------- ban state machine

#[test]
fn malicious_peer_banned_after_n_invalid_blocks_at_configured_threshold() {
    let manager = manager_with(|config| config.scoring.temp_ban_threshold = 0.5, None);
    manager.learn_addrs(vec![entry(1, 1)]);

    // ADR-0022 formula: quality 1.0 − 0.5·invalid/6. Six invalid blocks →
    // 0.500 (not < the configured 0.5 threshold); the seventh crosses it.
    for _ in 0..6 {
        manager.record_invalid_block(node_id(1));
        assert!(!manager.is_banned(&node_id(1)), "not banned yet");
    }
    manager.record_invalid_block(node_id(1));
    assert!(manager.is_banned(&node_id(1)), "banned at the threshold");
    let record = manager.get_peer(&node_id(1)).expect("record");
    assert_eq!(record.invalid_count, 7);
    assert!(record.score < 0.5);

    // Threshold tunability: the same event count under the default 0.2
    // threshold leaves the peer connected.
    let lenient = manager_with(|_| {}, None);
    lenient.learn_addrs(vec![entry(2, 2)]);
    for _ in 0..7 {
        lenient.record_invalid_block(node_id(2));
    }
    assert!(!lenient.is_banned(&node_id(2)), "0.417 > default 0.2");
}

#[tokio::test]
async fn temp_ban_expires_after_one_hour_simulated() {
    let dir = temp_dir("ban-expiry");
    let manager = manager_with(|_| {}, Some(dir.join("network.json")));
    manager.learn_addrs(vec![entry(1, 1)]);
    for _ in 0..10 {
        manager.record_invalid_block(node_id(1));
    }
    assert!(manager.is_banned(&node_id(1)));

    let after_ban = Instant::now();
    let report = manager.tick_at(after_ban + Duration::from_secs(3601));
    assert_eq!(report.expired_bans, 1);
    assert!(!manager.is_banned(&node_id(1)), "temp ban lifted after 1 h");
    assert_eq!(
        manager.historical_bans_len(),
        1,
        "unbanned record kept in the audit table"
    );
    // The persisted ban list was rewritten on expiry.
    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("network.json")).unwrap()).unwrap();
    assert_eq!(doc["banned_peers"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn persistent_ban_survives_restart() {
    let dir = temp_dir("persistent-restart");
    let path = dir.join("network.json");
    let manager = manager_with(|_| {}, Some(path.clone()));
    manager.learn_addrs(vec![entry(3, 3)]);
    // 20 protocol violations → ADR-0022 score 0.0 → persistent band.
    for _ in 0..20 {
        manager.record_protocol_violation(node_id(3));
    }
    assert!(manager.is_banned(&node_id(3)));

    // "Restart": a new manager on the same network.json loads the ban.
    let reloaded = manager_with(|_| {}, Some(path));
    assert!(
        reloaded.is_banned(&node_id(3)),
        "persistent ban reloaded from network.json"
    );
    let ban = reloaded
        .get_ban(&node_id(3))
        .expect("ban record after reload");
    assert!(ban.persistent);
    assert_eq!(ban.reason, BanReason::ProtocolViolation);
}

#[tokio::test]
async fn high_latency_but_valid_peer_is_never_banned() {
    let manager = manager_with(|_| {}, None);
    manager.learn_addrs(vec![entry(4, 4)]);

    // 200 ms on every message — the worst-case grading factor. The EWMA
    // converges toward 200 ms; the score floors at ~0.333 (Warn band),
    // above the 0.2 temp threshold.
    for _ in 0..30 {
        manager.record_latency(node_id(4), Duration::from_millis(200));
    }
    let report = manager.tick();
    assert_eq!(report.newly_banned, 0);
    assert!(
        !manager.is_banned(&node_id(4)),
        "slow ≠ malicious (ADR-0022)"
    );
    let record = manager.get_peer(&node_id(4)).expect("record");
    assert!(
        record.score > 0.3 && record.score < 0.4,
        "score in the warn band, got {}",
        record.score
    );
}

// ------------------------------------------------- connection pool limits

#[tokio::test]
async fn connection_pool_enforces_per_ip_range_limit() {
    let mut pool = ConnectionPool::new(2);

    // Two peers inside the same IPv4 /24 — allowed.
    let a = make_connection(0x01, addr(1)).await;
    let b = make_connection(0x02, addr(2)).await;
    let a_id = a.node_id();
    pool.add_connection(a).expect("first in range");
    pool.add_connection(b).expect("second in range");
    assert_eq!(pool.count_by_ip_range(addr(3).ip()), 2);

    // A third identity from the same /24 is refused (Sybil cap, V-17).
    let c = make_connection(0x03, addr(3)).await;
    let err = pool.add_connection(c).expect_err("range full");
    assert!(matches!(err, NetError::Peer(_)), "got {err:?}");

    // A different /24 is a fresh range — admitted.
    let d = make_connection(0x04, SocketAddr::new(IpAddr::from([10, 0, 1, 1]), 8081)).await;
    pool.add_connection(d).expect("other range");
    assert_eq!(pool.len(), 3);

    // Duplicate identity is refused; removal frees a slot.
    let duplicate = make_connection(0x01, addr(9)).await;
    // Same identity seed → same node_id, different socket.
    assert_eq!(duplicate.node_id(), a_id);
    let err = pool
        .add_connection(duplicate)
        .expect_err("duplicate node_id");
    assert!(matches!(err, NetError::Peer(_)));
    assert!(pool.remove_connection(&a_id).is_some());
    let retried = make_connection(0x03, addr(3)).await;
    pool.add_connection(retried).expect("slot freed");
}

#[tokio::test]
async fn manager_register_connection_respects_max_connections() {
    let manager = manager_with(
        |config| {
            config.max_connections = 1;
            config.max_per_ip_range = 8;
        },
        None,
    );
    let first = make_connection(0x21, addr(21)).await;
    let first_id = first.node_id();
    manager
        .register_connection(first)
        .expect("first registered");
    assert!(manager.get_peer(&first_id).unwrap().verified);

    let second = make_connection(0x22, addr(22)).await;
    let err = manager
        .register_connection(second)
        .expect_err("max_connections = 1");
    assert!(matches!(err, NetError::Peer(_)), "got {err:?}");

    // Disconnect frees the slot.
    assert!(manager.disconnect_peer(&first_id));
    let second = make_connection(0x22, addr(22)).await;
    manager.register_connection(second).expect("slot freed");
    assert_eq!(manager.connected_peers().len(), 1);
}

// ------------------------------------------------------------- e2e dial

const RESPONDER_SEED: [u8; 32] = [0xB2; 32];
const RESPONDER_STATIC: [u8; 32] = [0x22; 32];

/// Responder side of a Stage 2 connection: Noise XX (length-prefixed
/// framing) → HELLO exchange → decrypted-payload pump into `tx`.
async fn responder_session(
    stream: &mut TcpStream,
    identity: NodeKeypair,
    network_id: u32,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
) -> Result<(), NetError> {
    let mut handshake = NoiseHandshake::responder(&RESPONDER_STATIC, &identity, None)?;
    let mut buf = [0u8; HANDSHAKE_BUF];
    let mut payload = [0u8; HANDSHAKE_BUF];

    let msg = read_length_prefixed_async(stream).await?;
    handshake.read_message(&msg, &mut payload)?;
    let n = handshake.write_message(&[], &mut buf)?;
    write_frame(stream, &buf[..n]).await?;
    let msg = read_length_prefixed_async(stream).await?;
    handshake.read_message(&msg, &mut payload)?;
    let (mut transport, _peer_id) = handshake.into_transport()?;

    let hello_raw = read_frame_with_prefix(stream).await?;
    let _peer_hello = read_hello(&mut transport, &hello_raw, network_id)?;
    let ours = Hello::new(network_id, STAGE2_CAPABILITIES, 0);
    let mut out = Vec::new();
    write_hello(&mut transport, &ours, &mut out)?;
    use tokio::io::AsyncWriteExt;
    stream.write_all(&out).await?;

    loop {
        let raw = read_frame_with_prefix(stream).await?;
        let plaintext = decrypt_frame(&mut transport, &raw)?;
        if tx.send(plaintext).is_err() {
            break;
        }
    }
    Ok(())
}

async fn write_frame(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), NetError> {
    use tokio::io::AsyncWriteExt;
    let len = (bytes.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(bytes).await?;
    Ok(())
}

/// Length-prefixed read that reconstructs the full `[len][body]` frame
/// `decrypt_frame`/`read_hello` expect.
async fn read_frame_with_prefix(stream: &mut TcpStream) -> Result<Vec<u8>, NetError> {
    let body = read_length_prefixed_async(stream).await?;
    let mut raw = Vec::with_capacity(4 + body.len());
    raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
    raw.extend_from_slice(&body);
    Ok(raw)
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_to_peer_dials_noise_hello_registers_and_broadcasts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let dial_addr = listener.local_addr().expect("local_addr");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let responder_identity = NodeKeypair::from_seed(RESPONDER_SEED);
    let responder_id = responder_identity.node_id();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let identity = responder_identity.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = responder_session(&mut stream, identity, NETWORK_ID, &tx).await;
            });
        }
    });

    let manager = manager_with(|_| {}, None);
    let peer = manager
        .connect_to_peer(dial_addr, None)
        .await
        .expect("outbound dial completes");
    assert_eq!(
        peer, responder_id,
        "authenticated node_id from the handshake"
    );
    assert!(manager.connected_peers().contains(&peer));
    let record = manager.get_peer(&peer).expect("store record");
    assert!(record.verified, "handshake-verified peer");
    assert_eq!(record.addr, dial_addr);

    // Broadcast goes out encrypted; the responder decrypts the same payload.
    assert_eq!(manager.broadcast_message(b"hello-gossip"), 1);
    let payload = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("broadcast arrives")
        .expect("channel open");
    assert_eq!(payload, b"hello-gossip");

    // The AddrSender seam (S2-P11 → S2-P12) rides the same connection.
    manager
        .send_addr(&peer, &[0x05, 0, 0, 0, 0])
        .expect("send_addr");
    let addr_msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("addr message arrives")
        .expect("channel open");
    assert_eq!(addr_msg, vec![0x05, 0, 0, 0, 0]);

    // MITM/pinning: dialling a known-wrong identity is rejected at the
    // handshake (ADR-0020), before registration.
    let wrong = node_id(0x77);
    let err = manager
        .connect_to_peer(dial_addr, Some(wrong))
        .await
        .expect_err("expected_node_id mismatch must fail");
    assert!(matches!(err, NetError::Handshake(_)), "got {err:?}");
    assert!(!manager.connected_peers().contains(&wrong));

    // Banned identities are refused before a dial even starts.
    manager.ban_peer(
        responder_id,
        BanReason::Manual,
        Instant::now() + Duration::from_secs(600),
        false,
    );
    assert!(
        !manager.connected_peers().contains(&responder_id),
        "live session dropped on ban"
    );
    let err = manager
        .connect_to_peer(dial_addr, Some(responder_id))
        .await
        .expect_err("banned peer must be refused");
    assert!(matches!(err, NetError::Peer(_)), "got {err:?}");
    assert!(
        err.to_string().contains("banned"),
        "operator-visible reason, got {err}"
    );
}

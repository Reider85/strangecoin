//! BUG-S1-005 / ADR-0023 (S2-P16 + S2-P19 production wiring): per-message
//! rate limit. A single long-lived connection that streams junk messages is
//! banned once its bucket overflows — the accept-time check alone only
//! throttles connection rate, not the message rate of an established
//! session (THREAT_MODEL V-01/V-17).
//!
//! S2-P19: the flood rides the Noise-encrypted wire — the client dials
//! through a real `PeerManager` and pushes junk via `Connection::
//! send_payload`; the server's connection loop charges the ADR-0023
//! limiter per message and closes the session on the `Ban` verdict.

mod common;

use common::*;
use std::sync::Arc;
use std::time::Duration;
use strangecoin::ChainSnapshot;
use strangecoin_net::{NodeKeypair, PeerManager, PeerManagerConfig};

fn to_addr(port: u16) -> std::net::SocketAddr {
    format!("127.0.0.1:{}", port)
        .parse()
        .expect("valid socket addr")
}

// ADR-0011: the server accepts on the test runtime; multi_thread keeps it
// live while the test drives the Noise flood client. The NETWORK_TEST_LOCK
// std Mutex is held for the whole test by design (cross-binary serialization).
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn per_message_flood_bans_peer_on_single_connection() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let port = random_port();
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();

    let dir = TestDir::new("rate_limit_server");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let (mut node, _peers) = create_node_for_test(&bc, port);
    node.start_server(port, sync_tx).await;

    // 1. One Noise session to the server (correct network_id — the flood is
    //    a *rate* violation, not a handshake violation).
    let client_identity = NodeKeypair::from_seed([0x91; 32]);
    let client_node_id = client_identity.node_id();
    let client_pm = Arc::new(PeerManager::new(
        PeerManagerConfig::default(),
        client_identity,
    ));
    client_pm
        .connect_to_peer(to_addr(port), None)
        .await
        .expect("dial must succeed");
    let server_node_id = client_pm
        .connected_peers()
        .first()
        .copied()
        .expect("client sees the server");
    let conn = client_pm.connection(&server_node_id).expect("connection");

    // 2. Flood junk on the SAME connection. `PINGjunk` is an unknown text
    //    payload (main bucket, Stage-1-equivalent budget 100/10 from
    //    `create_node_for_test`): the server ignores the content, but the
    //    per-message check arms the violation ladder — P13 «peer sends
    //    200 msgs → ban mid-flood». Writes after the server closes surface
    //    as transport errors — tolerated, the ban has already landed.
    for _ in 0..200 {
        if conn.send_payload(b"PINGjunk").is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    // 3. The offender is banned: the authenticated node_id in the peer
    //    store (apply_rate_verdict) AND the server's address-keyed limiter
    //    (the same handle the loop charged).
    let banned_by_scoring = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if node.peer_manager.is_banned(&client_node_id) {
                break true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or(false);
    assert!(
        banned_by_scoring,
        "per-message flood must ban the authenticated peer (peer store)"
    );

    // The server recorded the session under the client's ephemeral address;
    // its limiter entry is banned too (fast path, ADR-0023).
    let store_record = node
        .peer_manager
        .get_peer(&client_node_id)
        .expect("peer record exists");
    let server_seen = store_record.addr;
    assert!(
        node.rate_limiter.check(server_seen).is_err(),
        "per-message flood must ban the peer at the address-keyed limiter"
    );

    drop(node);
}

//! BUG-S1-005: per-message rate limit (P13). A single long-lived TCP
//! connection that streams junk messages is banned once its per-class bucket
//! overflows — the accept-time check alone only throttles connection rate,
//! not the message rate of an established session (THREAT_MODEL V-01/V-17).

mod common;

use common::*;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;
use strangecoin::{network::protocol, ChainSnapshot};

fn to_addr(port: u16) -> SocketAddr {
    format!("127.0.0.1:{}", port)
        .parse()
        .expect("valid socket addr")
}

fn connect(port: u16) -> TcpStream {
    TcpStream::connect(to_addr(port)).expect("server must accept connections")
}

// ADR-0011: the server accepts on the test runtime; multi_thread keeps it
// live while the test thread drives the raw-TcpStream flood client.
// The NETWORK_TEST_LOCK std Mutex is held for the whole test by design
// (cross-binary serialization); nothing on this runtime contends for it.
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

    // 1. One TCP connection, correct HELLO handshake.
    let s = connect(port);
    let client_addr = s.local_addr().expect("client socket address");
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    s.set_write_timeout(Some(Duration::from_secs(5)))
        .expect("set write timeout");
    let mut w = BufWriter::new(s.try_clone().expect("clone for writing"));
    w.write_all(&protocol::encode_hello(
        strangecoin::consensus::CHAIN_ID_REGTEST,
    ))
    .expect("write native HELLO");
    w.flush().expect("flush native HELLO");

    // 2. Flood 200 junk messages on the SAME connection. `PING` is unknown
    //    text (class Other, per-class cap 30): the server ignores the
    //    content, but the per-message check bans the peer once the class
    //    bucket overflows — P13 «peer sends 200 msgs → ban mid-flood».
    //    Writes after the server closes surface as RST — tolerated, the
    //    ban has already landed by then.
    for _ in 0..200 {
        if protocol::write_length_prefixed(&mut w, b"PING").is_err() {
            break;
        }
        if w.flush().is_err() {
            break;
        }
    }

    // 3. The server closed the flooded connection (EOF, or RST when the
    //    socket drops with unread flood data still buffered — both mean
    //    the handler broke out of the request loop).
    let mut r = BufReader::new(s);
    let mut byte = [0u8; 1];
    let read = r.read(&mut byte);
    assert!(
        matches!(read, Ok(0)) || read.is_err(),
        "server must close the flooded connection (EOF or reset), got {:?}",
        read
    );

    // 4. The offender is banned at the rate limiter (the same handle the
    //    server used for the accept-time and per-message checks).
    assert!(
        node.rate_limiter.check(client_addr).is_err(),
        "per-message flood must ban the peer"
    );

    drop(node);
}

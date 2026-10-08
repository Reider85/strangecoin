//! S1-P19: network isolation — a HELLO from a foreign network_id is dropped
//! with an EOF and the peer is banned, while the correct network_id is served
//! (S1-P14 HELLO handshake; DoD: network_id/HELLO coverage).

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
// live while the test thread drives blocking raw-TcpStream clients.
// The NETWORK_TEST_LOCK std Mutex is held for the whole test by design
// (cross-binary serialization); nothing on this runtime contends for it.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn foreign_network_id_is_rejected_and_banned() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let port = random_port();
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();

    let dir = TestDir::new("netid_server");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let (mut node, _peers) = create_node_for_test(&bc, port);
    node.start_server(port, sync_tx).await;

    // 1. Foreign network: the server bans the peer and closes the connection
    //    without answering anything else.
    let s1 = connect(port);
    let client_addr = s1.local_addr().expect("client socket address");
    let mut w1 = BufWriter::new(s1.try_clone().expect("clone for writing"));
    w1.write_all(&protocol::encode_hello(
        strangecoin::consensus::CHAIN_ID_REGTEST + 1,
    ))
    .expect("write foreign HELLO");
    w1.flush().expect("flush foreign HELLO");

    s1.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let mut r1 = BufReader::new(s1);
    let mut byte = [0u8; 1];
    let read = r1
        .read(&mut byte)
        .expect("server must close the foreign connection promptly");
    assert_eq!(
        read, 0,
        "foreign HELLO must be answered with EOF, not with data"
    );

    // 2. The offender is banned at the rate limiter (the same handle the
    //    server used for the accept-time check).
    assert!(
        node.rate_limiter.check(client_addr).is_err(),
        "foreign network_id must ban the peer"
    );

    // 3. Control: the correct network_id gets served.
    let s2 = connect(port);
    let mut w2 = BufWriter::new(s2.try_clone().expect("clone for writing"));
    w2.write_all(&protocol::encode_hello(strangecoin::consensus::CHAIN_ID_REGTEST))
        .expect("write native HELLO");
    w2.flush().expect("flush native HELLO");
    protocol::write_length_prefixed(&mut w2, b"GET_BLOCKCHAIN")
        .expect("write GET_BLOCKCHAIN");

    s2.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let mut r2 = BufReader::new(s2);
    let response = protocol::read_length_prefixed(&mut r2)
        .expect("valid network_id must be served");
    assert!(!response.is_empty(), "chain payload must not be empty");

    drop(node);
}

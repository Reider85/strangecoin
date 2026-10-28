//! S1-P19 / S2-P19: network isolation — a HELLO from a foreign network_id
//! is dropped with an EOF and scored as a protocol violation against the
//! authenticated identity, while the correct network_id is served.
//!
//! S2-P19 moved the wire to Noise + the encrypted S2-P10 HELLO: the client
//! below completes a raw XX handshake (no PeerManager on the test side),
//! then sends the 20-byte HELLO payload inside the encrypted framing.

mod common;

use common::*;
use std::sync::Arc;
use std::time::Duration;
use strangecoin::{network::protocol, ChainSnapshot};
use strangecoin_net::protocol::hello::{write_hello, Capability, Hello};
use strangecoin_net::protocol::{decrypt_frame, encrypt_frame, read_length_prefixed_async};
use strangecoin_net::{NodeKeypair, NoiseHandshake, NoiseTransport};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn to_addr(port: u16) -> std::net::SocketAddr {
    format!("127.0.0.1:{}", port)
        .parse()
        .expect("valid socket addr")
}

/// Raw Noise XX initiator + encrypted HELLO exchange (S2-P19 client side).
async fn noise_session(
    stream: &mut TcpStream,
    identity: &NodeKeypair,
    network_id: u32,
) -> NoiseTransport {
    let mut handshake =
        NoiseHandshake::initiator(&identity.noise_static_private_key(), identity, None)
            .expect("initiator");
    let mut buf = [0u8; 512];
    let mut payload = [0u8; 512];

    let n = handshake.write_message(&[], &mut buf).expect("r1");
    write_length(stream, &buf[..n]).await;
    let msg = read_length(stream).await;
    handshake.read_message(&msg, &mut payload).expect("r2");
    let n = handshake.write_message(&[], &mut buf).expect("r3");
    write_length(stream, &buf[..n]).await;

    let (mut transport, _peer) = handshake.into_transport().expect("transport");
    // Noise-only capabilities: the control client reads exactly the HEADERS
    // reply, so the peer loop must not auto-send Erlay sketches first.
    let hello = Hello::new(network_id, Capability::Noise.bit(), 0);
    let mut out = Vec::new();
    write_hello(&mut transport, &hello, &mut out).expect("hello encode");
    stream.write_all(&out).await.expect("hello write");
    // Read (and discard) the responder's HELLO. A peer that rejects our
    // network_id drops the session without answering — that EOF is the
    // expected outcome for the foreign-network case, so failures here are
    // tolerated silently.
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let Ok(body) = read_length_prefixed_async(stream).await else {
            return;
        };
        let mut raw = Vec::with_capacity(4 + body.len());
        raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
        raw.extend_from_slice(&body);
        let _ = decrypt_frame(&mut transport, &raw);
    })
    .await;
    transport
}

async fn write_length(stream: &mut TcpStream, bytes: &[u8]) {
    let mut framed = Vec::with_capacity(4 + bytes.len());
    framed.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    framed.extend_from_slice(bytes);
    stream.write_all(&framed).await.expect("framed write");
}

async fn read_length(stream: &mut TcpStream) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(5), read_length_prefixed_async(stream))
        .await
        .expect("handshake frame arrives")
        .expect("handshake frame reads")
}

async fn encrypted_send(transport: &mut NoiseTransport, stream: &mut TcpStream, payload: &[u8]) {
    let mut frame = Vec::new();
    encrypt_frame(transport, payload, &mut frame).expect("encrypt");
    stream.write_all(&frame).await.expect("write frame");
}

async fn encrypted_recv(transport: &mut NoiseTransport, stream: &mut TcpStream) -> Vec<u8> {
    let body = tokio::time::timeout(Duration::from_secs(5), read_length_prefixed_async(stream))
        .await
        .expect("frame arrives")
        .expect("frame reads");
    let mut raw = Vec::with_capacity(4 + body.len());
    raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
    raw.extend_from_slice(&body);
    decrypt_frame(transport, &raw).expect("decrypt")
}

// ADR-0011: the server accepts on the test runtime; multi_thread keeps it
// live while the test drives the Noise client. The NETWORK_TEST_LOCK std
// Mutex is held for the whole test by design (cross-binary serialization).
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn foreign_network_id_is_rejected_and_scored() {
    let _net_lock = NETWORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let port = random_port();
    let (sync_tx, _sync_rx) = tokio::sync::mpsc::unbounded_channel::<ChainSnapshot>();

    let dir = TestDir::new("netid_server");
    let bc = Arc::new(create_test_blockchain(dir.path()));
    let (mut node, _peers) = create_node_for_test(&bc, port);
    node.start_server(port, sync_tx).await;

    // 1. Foreign network: the Noise handshake succeeds (it authenticates
    //    the identity), but the encrypted HELLO carries a foreign
    //    network_id (invariant #10) — the server drops the session without
    //    answering anything else and records a protocol violation against
    //    the authenticated node_id (ADR-0022; replaces the pre-S2-P19
    //    address-keyed ban — the identity is known now, so scoring it is
    //    strictly stronger).
    let foreign_identity = NodeKeypair::from_seed([0x57; 32]);
    let node_id = foreign_identity.node_id();
    let mut s1 = TcpStream::connect(to_addr(port))
        .await
        .expect("server must accept connections");
    let _ = noise_session(
        &mut s1,
        &foreign_identity,
        strangecoin::consensus::CHAIN_ID_REGTEST + 1,
    )
    .await;

    // The server closed the foreign connection: EOF or reset on the next
    // read (there is no response to a rejected HELLO).
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(5), s1.read(&mut byte))
        .await
        .expect("server must close the foreign connection promptly")
        .unwrap_or(0);
    assert_eq!(
        read, 0,
        "foreign HELLO must be answered with EOF, not with data"
    );

    // 2. The offender carries a protocol violation in the peer store.
    let record = node
        .peer_manager
        .get_peer(&node_id)
        .expect("handshake authenticated the peer into the store");
    assert!(
        record.violation_count >= 1,
        "foreign network_id must score a protocol violation, got {}",
        record.violation_count
    );

    // 3. Control: the correct network_id gets served over the same Noise
    //    wire — GET_HEADERS comes back as an encrypted HEADERS reply.
    let mut s2 = TcpStream::connect(to_addr(port))
        .await
        .expect("server must accept connections");
    let mut transport = noise_session(
        &mut s2,
        &NodeKeypair::from_seed([0x58; 32]),
        strangecoin::consensus::CHAIN_ID_REGTEST,
    )
    .await;
    encrypted_send(&mut transport, &mut s2, &protocol::encode_get_headers(0)).await;
    let response = encrypted_recv(&mut transport, &mut s2).await;
    assert!(!response.is_empty(), "headers payload must not be empty");
    assert_eq!(response[0], protocol::MSG_HEADERS, "served a HEADERS reply");

    drop(node);
}

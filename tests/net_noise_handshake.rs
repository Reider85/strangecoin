//! S2-P22 — Noise handshake integration: two real nodes, MITM detection
//! (ADR-0020 / threat V-24), and the unpinned first-contact path.
//!
//! Regtest (`network_id = 3`), ephemeral localhost sockets, no sleeps —
//! every wait is a channel receive or a bounded I/O timeout.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;

use common::net_helpers::{node_id_of, transport_pair};
use strangecoin_net::{
    NetError, NodeId, NodeKeypair, NoiseHandshake, PeerManager, PeerManagerConfig,
};
use tokio::net::TcpListener;

const HANDSHAKE_BUF: usize = 512;

fn manager_for(seed: [u8; 32]) -> Arc<PeerManager> {
    let config = PeerManagerConfig {
        network_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        ..PeerManagerConfig::default()
    };
    Arc::new(PeerManager::new(config, NodeKeypair::from_seed(seed)))
}

/// Accept loop on a fresh localhost port: every inbound session completes
/// `accept_inbound` (Noise XX responder + HELLO) under `manager`.
async fn spawn_acceptor(manager: Arc<PeerManager>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer_addr)) = listener.accept().await else {
                break;
            };
            let manager = Arc::clone(&manager);
            tokio::spawn(async move {
                let _ = manager.accept_inbound(stream, peer_addr).await;
            });
        }
    });
    addr
}

/// Two nodes complete the XX handshake (initiator + responder) and drive
/// encrypted traffic through both resulting transports.
#[test]
fn two_nodes_complete_handshake_and_traffic_roundtrips() {
    let (mut initiator_transport, mut responder_transport, initiator_peer, responder_peer) =
        transport_pair([0xA1; 32], [0xB2; 32]);

    // Each side authenticated the *other* side's Ed25519 identity.
    assert_eq!(initiator_peer, node_id_of([0xB2; 32]));
    assert_eq!(responder_peer, node_id_of([0xA1; 32]));

    // The pinned transport recorded who we expected — the session is
    // verified end-to-end.
    assert_eq!(
        initiator_transport.expected_node_id(),
        Some(*node_id_of([0xB2; 32]).as_bytes()),
        "initiator pinned the responder"
    );

    // Encrypted round-trip over the established transport (S2-P08 framing).
    let payload = b"stage2 integration traffic";
    let mut out = [0u8; HANDSHAKE_BUF];
    let n = initiator_transport
        .encrypt(payload, &mut out)
        .expect("encrypt");
    let mut scratch = [0u8; HANDSHAKE_BUF];
    let len = responder_transport
        .decrypt(&out[..n], &mut scratch)
        .expect("decrypt");
    assert_eq!(&scratch[..len], payload);

    // Reverse direction (independent nonce counters per direction).
    let n = responder_transport
        .encrypt(b"pong", &mut out)
        .expect("encrypt");
    let len = initiator_transport
        .decrypt(&out[..n], &mut scratch)
        .expect("decrypt");
    assert_eq!(&scratch[..len], b"pong");

    // A replayed frame is rejected (anti-replay invariant, S2-P08).
    let replay = out[..n].to_vec();
    assert!(
        initiator_transport.decrypt(&replay, &mut scratch).is_err(),
        "replayed frame must be rejected"
    );
}

/// MITM: an attacker completing the handshake under their own Noise + Ed25519
/// keys is dropped on the `expected_node_id` pin mismatch — before any
/// session is registered on the dialing side (threat V-24, ADR-0020).
#[tokio::test(flavor = "multi_thread")]
async fn mitm_with_own_keys_is_dropped_on_node_id_mismatch() {
    // The victim we *mean* to reach.
    let victim = node_id_of([0x77; 32]);
    // The attacker actually listening on the wire.
    let attacker = manager_for([0xEE; 32]);
    let attacker_id = attacker.local_node_id();
    assert_ne!(attacker_id, victim);

    let dial_addr = spawn_acceptor(Arc::clone(&attacker)).await;
    let dialer = manager_for([0xD1; 32]);

    let err = dialer
        .connect_to_peer(dial_addr, Some(victim))
        .await
        .expect_err("pinned dial must fail against the attacker's identity");
    assert!(matches!(err, NetError::Handshake(_)), "got {err:?}");

    // Nothing was registered on the dialing side — the session died in the
    // handshake, no transport ever existed.
    assert!(
        !dialer.connected_peers().contains(&attacker_id),
        "MITM session must never register"
    );
    assert!(!dialer.connected_peers().contains(&victim));
}

/// First contact with no pinned identity: the handshake completes and the
/// session is accepted *unverified* — `expected_node_id == None` on the
/// transport, and an ADDR-learned store record stays `verified = false`
/// until a successful dial flips it.
#[tokio::test(flavor = "multi_thread")]
async fn unpinned_first_contact_is_accepted_as_unverified() {
    // (a) In-memory: no pin → transport carries no expectation.
    let (initiator_transport, _responder_transport, initiator_peer, _) = transport_pair_unpinned();
    assert_eq!(
        initiator_transport.expected_node_id(),
        None,
        "unpinned session is unverified"
    );
    let _ = initiator_peer;

    // (b) Store: an ADDR-gossip learned peer is unverified.
    let manager = manager_for([0xC2; 32]);
    let learned = node_id_of([0x33; 32]);
    manager.learn_addrs(vec![strangecoin_net::discovery::addr_gossip::AddrEntry {
        node_id: learned,
        addr: "10.42.9.9:8081".parse().expect("addr"),
    }]);
    let record = manager.get_peer(&learned).expect("store record");
    assert!(!record.verified, "ADDR-learned peer starts unverified");

    // (c) An unpinned real dial is accepted — then the handshake
    // authentication marks the record verified (sticky, S2-P12).
    let responder = manager_for([0xE1; 32]);
    let responder_id = responder.local_node_id();
    let dial_addr = spawn_acceptor(responder).await;
    let peer = manager
        .connect_to_peer(dial_addr, None)
        .await
        .expect("unpinned dial completes");
    assert_eq!(peer, responder_id);
    let record = manager.get_peer(&peer).expect("store record");
    assert!(record.verified, "handshake-authenticated peer verified");
    assert!(!manager.is_banned(&peer), "first contact is not a ban");
}

/// In-memory handshake with no `expected_node_id` pin (the ADDR-first
/// flow) — `(initiator_transport, responder_transport, peer, _)`.
fn transport_pair_unpinned() -> (
    strangecoin_net::NoiseTransport,
    strangecoin_net::NoiseTransport,
    NodeId,
    NodeId,
) {
    let key_a = NodeKeypair::from_seed([0xF1; 32]);
    let key_b = NodeKeypair::from_seed([0xF2; 32]);
    let mut initiator = NoiseHandshake::initiator(&key_a.noise_static_private_key(), &key_a, None)
        .expect("initiator");
    let mut responder = NoiseHandshake::responder(&key_b.noise_static_private_key(), &key_b, None)
        .expect("responder");
    let mut msg = [0u8; HANDSHAKE_BUF];
    let mut payload = [0u8; HANDSHAKE_BUF];

    let n = initiator.write_message(&[], &mut msg).expect("round 1");
    responder
        .read_message(&msg[..n], &mut payload)
        .expect("round 1 read");
    let n = responder.write_message(&[], &mut msg).expect("round 2");
    initiator
        .read_message(&msg[..n], &mut payload)
        .expect("round 2 read");
    let n = initiator.write_message(&[], &mut msg).expect("round 3");
    responder
        .read_message(&msg[..n], &mut payload)
        .expect("round 3 read");

    let (transport_a, peer_a) = initiator.into_transport().expect("transport a");
    let (transport_b, peer_b) = responder.into_transport().expect("transport b");
    (transport_a, transport_b, peer_a, peer_b)
}

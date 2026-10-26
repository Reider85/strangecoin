//! S2-P10 — HELLO message + capabilities negotiation.
//!
//! Version / network_id rejection (invariant #10), the capability subset
//! negotiation and a full HELLO round-trip over the S2-P08 Noise encrypted
//! framing — the exact exchange the peer connection flow (S2-P12) will drive
//! after `into_transport()`.

use strangecoin_net::protocol::hello::{
    negotiate_capabilities, read_hello, write_hello, Capability, Hello, HELLO_WIRE_LEN,
    MIN_PROTO_VERSION, STAGE2_CAPABILITIES,
};
use strangecoin_net::{NetError, NodeKeypair, NoiseHandshake, NoiseTransport};

const BUF: usize = 512;

const INITIATOR_STATIC: [u8; 32] = [0x11; 32];
const RESPONDER_STATIC: [u8; 32] = [0x22; 32];

const NETWORK_ID: u32 = 3;

/// Run a full XX handshake (S2-P09 auth blobs included) and return both
/// peers' transports, pinned to each other's node_id.
fn handshake_pair() -> (NoiseTransport, NoiseTransport) {
    let init_id = NodeKeypair::from_seed([0xA1; 32]);
    let resp_id = NodeKeypair::from_seed([0xB2; 32]);
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC, &init_id, Some(resp_id.node_id()))
        .expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC, &resp_id, Some(init_id.node_id()))
        .expect("responder");

    let mut msg = [0u8; BUF];
    let mut payload = [0u8; BUF];

    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    resp.read_message(&msg[..n1], &mut payload)
        .expect("round 1 read");
    let n2 = resp.write_message(&[], &mut msg).expect("round 2 write");
    init.read_message(&msg[..n2], &mut payload)
        .expect("round 2 read");
    let n3 = init.write_message(&[], &mut msg).expect("round 3 write");
    resp.read_message(&msg[..n3], &mut payload)
        .expect("round 3 read");

    assert!(init.is_finished() && resp.is_finished());
    let (initiator, _) = init.into_transport().expect("init transport");
    let (responder, _) = resp.into_transport().expect("resp transport");
    (initiator, responder)
}

#[test]
fn wire_format_is_20_bytes_big_endian() {
    let hello = Hello {
        version: 2,
        network_id: 0x0102_0304,
        capabilities: 0x0A0B_0C0D,
        height: 0x1122_3344_5566_7788,
    };
    let wire = hello.encode();
    assert_eq!(wire.len(), HELLO_WIRE_LEN);
    assert_eq!(&wire[0..4], &2u32.to_be_bytes());
    assert_eq!(&wire[4..8], &0x0102_0304u32.to_be_bytes());
    assert_eq!(&wire[8..12], &0x0A0B_0C0Du32.to_be_bytes());
    assert_eq!(&wire[12..20], &0x1122_3344_5566_7788u64.to_be_bytes());

    assert_eq!(Hello::decode(&wire).expect("decode"), hello);
}

#[test]
fn decode_rejects_wrong_length_and_trailing_garbage() {
    let hello = Hello::new(NETWORK_ID, STAGE2_CAPABILITIES, 42);
    let wire = hello.encode();

    for len in [0, 1, 19, 21, 64] {
        let mut bad = wire.to_vec();
        bad.resize(len, 0);
        let err = Hello::decode(&bad).expect_err("non-20-byte payload must be rejected");
        assert!(
            matches!(err, NetError::Protocol(_)),
            "expected protocol failure for len {len}, got {err:?}"
        );
    }

    // Exact length but one flipped field byte still decodes — rejection of a
    // wrong version/network happens in validate_hello, not in the codec.
    let mut flipped = wire;
    flipped[4] ^= 0xFF;
    assert!(Hello::decode(&flipped).is_ok());
}

#[test]
fn compatible_peers_are_accepted() {
    let hello = Hello::new(NETWORK_ID, STAGE2_CAPABILITIES, 7);
    assert_eq!(hello.version, MIN_PROTO_VERSION);
    validate_ok(&hello);
}

#[test]
fn incompatible_version_is_rejected() {
    // Stage 1.5 spoke version 1 — must not pass the Stage 2 gate.
    let hello = Hello {
        version: MIN_PROTO_VERSION - 1,
        ..Hello::new(NETWORK_ID, STAGE2_CAPABILITIES, 0)
    };
    let err = validate(&hello).expect_err("version 1 must be rejected");
    assert!(
        matches!(err, NetError::Protocol(_)),
        "expected protocol failure, got {err:?}"
    );
    assert!(err.to_string().contains("version"));
}

#[test]
fn mismatched_network_id_is_rejected() {
    // Invariant #10 on the network layer: peer on network 1, we are on 3.
    let hello = Hello::new(1, STAGE2_CAPABILITIES, 0);
    let err = validate(&hello).expect_err("foreign network_id must be rejected");
    assert!(
        matches!(err, NetError::Protocol(_)),
        "expected protocol failure, got {err:?}"
    );
    assert!(err.to_string().contains("network_id mismatch"));
}

#[test]
fn capability_negotiation_returns_common_subset() {
    // Prompt example: peer 0x07 (NOISE+Compact+Erlay), local 0x05
    // (NOISE+Compact) -> 0x05.
    assert_eq!(negotiate_capabilities(0x05, 0x07), 0x05);
    // Shadow bit advertised by a future peer is not part of Stage 2 set.
    assert_eq!(negotiate_capabilities(STAGE2_CAPABILITIES, 0x0F), 0x07);
    assert_eq!(negotiate_capabilities(0x07, 0x00), 0x00);
    assert_eq!(
        negotiate_capabilities(STAGE2_CAPABILITIES, STAGE2_CAPABILITIES),
        STAGE2_CAPABILITIES,
        "identical advertisements negotiate to the full set"
    );
    // The advertised set really is NOISE | COMPACT_BLOCKS | ERLAY.
    assert_eq!(STAGE2_CAPABILITIES, 0x07);
    assert_eq!(Capability::Noise.bit(), 0x01);
    assert_eq!(Capability::CompactBlocks.bit(), 0x02);
    assert_eq!(Capability::Erlay.bit(), 0x04);
    assert_eq!(Capability::Shadow.bit(), 0x08);
}

#[test]
fn hello_roundtrip_over_noise_encrypted_framing() {
    let (mut initiator, mut responder) = handshake_pair();

    let alice = Hello::new(NETWORK_ID, 0x05, 100); // NOISE + COMPACT_BLOCKS
    let bob = Hello::new(NETWORK_ID, 0x07, 99); // + ERLAY

    // Alice -> Bob over the S2-P08 encrypted framing.
    let mut frame = Vec::new();
    write_hello(&mut initiator, &alice, &mut frame).expect("seal alice HELLO");
    let received = read_hello(&mut responder, &frame, NETWORK_ID).expect("accept alice HELLO");
    assert_eq!(received, alice, "fields survive the encrypted round-trip");

    // Bob -> Alice (both sides always greet).
    frame.clear();
    write_hello(&mut responder, &bob, &mut frame).expect("seal bob HELLO");
    let received = read_hello(&mut initiator, &frame, NETWORK_ID).expect("accept bob HELLO");
    assert_eq!(received, bob);

    // Both peers run the same negotiation over the same advertisements and
    // agree on the usable subset (alice 0x05 & bob 0x07 = 0x05).
    assert_eq!(
        negotiate_capabilities(alice.capabilities, received.capabilities),
        0x05,
        "both directions compute the same common subset"
    );
}

#[test]
fn incompatible_peer_is_dropped_during_hello_exchange() {
    let (mut initiator, mut responder) = handshake_pair();

    // A Stage 1.5 peer that somehow completed the Noise handshake still
    // announces version 1 — the exchange drops it before anything else.
    let old = Hello {
        version: 1,
        ..Hello::new(NETWORK_ID, STAGE2_CAPABILITIES, 5)
    };
    let mut frame = Vec::new();
    write_hello(&mut initiator, &old, &mut frame).expect("seal");
    let err =
        read_hello(&mut responder, &frame, NETWORK_ID).expect_err("version-1 peer must be dropped");
    assert!(matches!(err, NetError::Protocol(_)), "got {err:?}");

    // Same drop for a foreign network (invariant #10).
    let foreign = Hello::new(1, STAGE2_CAPABILITIES, 5);
    let mut frame = Vec::new();
    write_hello(&mut initiator, &foreign, &mut frame).expect("seal");
    let err = read_hello(&mut responder, &frame, NETWORK_ID)
        .expect_err("foreign-network peer must be dropped");
    assert!(matches!(err, NetError::Protocol(_)), "got {err:?}");
}

fn validate(hello: &Hello) -> Result<(), NetError> {
    strangecoin_net::protocol::hello::validate_hello(hello, NETWORK_ID)
}

fn validate_ok(hello: &Hello) {
    strangecoin_net::protocol::hello::validate_hello(hello, NETWORK_ID)
        .expect("compatible peer must be accepted");
}

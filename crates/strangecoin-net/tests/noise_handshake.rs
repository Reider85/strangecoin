//! S2-P07 — Noise XX handshake state machine.
//!
//! Three round trips, forward secrecy via ephemeral keys, tamper detection
//! and the application-level static-key binding (ADR-0020). S2-P09 added the
//! Ed25519 auth blobs: round 2 and round 3 each carry 96 extra payload bytes
//! (`[pubkey || signature]`), which is reflected in the message sizes below.

use strangecoin_net::noise::{NodeKeypair, NoiseError, NoiseHandshake, NoiseRole};
use strangecoin_net::NetError;

/// Handshake messages stay well under this: XX adds at most 96 bytes on top
/// of the payload, and the payload is the fixed 96-byte auth blob at most.
const BUF: usize = 512;

const INITIATOR_STATIC: [u8; 32] = [0x11; 32];
const RESPONDER_STATIC: [u8; 32] = [0x22; 32];
const INITIATOR_IDENTITY: [u8; 32] = [0xA1; 32];
const RESPONDER_IDENTITY: [u8; 32] = [0xB2; 32];

fn initiator() -> Result<NoiseHandshake, NoiseError> {
    NoiseHandshake::initiator(
        &INITIATOR_STATIC,
        &NodeKeypair::from_seed(INITIATOR_IDENTITY),
        None,
    )
}

fn responder() -> Result<NoiseHandshake, NoiseError> {
    NoiseHandshake::responder(
        &RESPONDER_STATIC,
        &NodeKeypair::from_seed(RESPONDER_IDENTITY),
        None,
    )
}

/// Drive a full XX handshake (auth-blob payloads) and return the three
/// message lengths, proving the exchange really took 3 round trips.
fn drive(init: &mut NoiseHandshake, resp: &mut NoiseHandshake) -> (usize, usize, usize) {
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

    (n1, n2, n3)
}

#[test]
fn handshake_completes_in_three_round_trips() {
    let mut init = initiator().expect("initiator");
    let mut resp = responder().expect("responder");

    assert_eq!(init.role(), NoiseRole::Initiator);
    assert_eq!(resp.role(), NoiseRole::Responder);
    assert!(!init.is_finished());
    assert!(!resp.is_finished());

    // Round 1: e (no auth blob yet — the initiator has not seen the peer)
    let mut msg = [0u8; BUF];
    let mut payload = [0u8; BUF];
    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    assert_eq!(n1, 32, "round 1 carries the initiator ephemeral key only");
    resp.read_message(&msg[..n1], &mut payload)
        .expect("round 1 read");
    assert!(!resp.is_finished(), "responder still needs rounds 2 and 3");

    // Round 2: e, ee, s, es + responder auth blob (96) + payload tag (16)
    let n2 = resp.write_message(&[], &mut msg).expect("round 2 write");
    assert_eq!(n2, 192, "round 2: e(32) + s(48) + auth blob ct(112)");
    init.read_message(&msg[..n2], &mut payload)
        .expect("round 2 read");
    assert_eq!(
        init.peer_node_id(),
        Some(NodeKeypair::from_seed(RESPONDER_IDENTITY).node_id()),
        "initiator learns the responder's node_id from round 2"
    );
    assert!(
        !init.is_finished(),
        "initiator finishes only after writing round 3"
    );

    // Round 3: s, se + initiator auth blob (96) + payload tag (16)
    let n3 = init.write_message(&[], &mut msg).expect("round 3 write");
    assert_eq!(n3, 160, "round 3: s(48) + auth blob ct(112)");
    assert!(
        init.is_finished(),
        "initiator finished after writing round 3"
    );
    resp.read_message(&msg[..n3], &mut payload)
        .expect("round 3 read");
    assert!(
        resp.is_finished(),
        "responder finished after reading round 3"
    );
    assert_eq!(
        resp.peer_node_id(),
        Some(NodeKeypair::from_seed(INITIATOR_IDENTITY).node_id()),
        "responder learns the initiator's node_id from round 3"
    );

    init.into_transport()
        .expect("initiator enters transport mode");
    resp.into_transport()
        .expect("responder enters transport mode");
}

#[test]
fn same_static_keys_yield_same_shared_secret() {
    // Both peers run the handshake with one and the same static key *and*
    // the same identity (the shared Arc keeps one ephemeral-counter sequence).
    let static_key = [0x42; 32];
    let identity = NodeKeypair::from_seed([0x42; 32]);
    let mut init = NoiseHandshake::initiator(&static_key, &identity, None).expect("initiator");
    let mut resp = NoiseHandshake::responder(&static_key, &identity, None).expect("responder");

    drive(&mut init, &mut resp);
    assert!(init.is_finished() && resp.is_finished());

    // Both sides mix the identical transcript — the handshake hash both
    // transport key halves are derived from — so they agree on the shared
    // secret.
    assert_eq!(init.handshake_hash().len(), 32, "BLAKE2s handshake hash");
    assert_eq!(
        init.handshake_hash(),
        resp.handshake_hash(),
        "both peers must derive the same shared secret"
    );
}

#[test]
fn distinct_static_keys_still_agree_on_handshake_hash() {
    let mut init = initiator().expect("initiator");
    let mut resp = responder().expect("responder");

    drive(&mut init, &mut resp);

    assert_eq!(init.handshake_hash(), resp.handshake_hash());
    assert!(
        init.remote_static().is_some() && resp.remote_static().is_some(),
        "XX transports both static keys"
    );
    assert_ne!(
        init.remote_static(),
        resp.remote_static(),
        "each peer learns the *other* peer's static key"
    );
}

#[test]
fn corrupted_handshake_message_returns_decrypt_error() {
    let mut init = initiator().expect("initiator");
    let mut resp = responder().expect("responder");

    let mut msg = [0u8; BUF];
    let mut payload = [0u8; BUF];
    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    resp.read_message(&msg[..n1], &mut payload)
        .expect("round 1 read");

    let n2 = resp.write_message(&[], &mut msg).expect("round 2 write");

    // Flip one byte inside the encrypted `s` section (bytes 32..80 of msg2).
    msg[40] ^= 0xFF;

    let err = init
        .read_message(&msg[..n2], &mut payload)
        .expect_err("tampered message must not authenticate");
    assert!(
        matches!(err, NoiseError::Decrypt),
        "expected decrypt failure, got {err:?}"
    );
}

#[test]
fn wrong_static_key_fails_at_se_step() {
    let mut init = initiator().expect("initiator");
    let mut resp = responder()
        .expect("responder")
        // Responder expects a different peer than the one connecting.
        .expect_remote_static([0xEE; 32]);

    let mut msg = [0u8; BUF];
    let mut payload = [0u8; BUF];

    // Rounds 1 and 2 do not reveal the initiator's static key to the
    // responder, so they still pass.
    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    resp.read_message(&msg[..n1], &mut payload)
        .expect("round 1 read");
    let n2 = resp.write_message(&[], &mut msg).expect("round 2 write");
    init.read_message(&msg[..n2], &mut payload)
        .expect("round 2 read");
    assert!(
        !resp.is_finished(),
        "failure must happen on round 3, not earlier"
    );

    // Round 3 is the `se` step: the responder decrypts the initiator's static
    // key and the auth check rejects it.
    let n3 = init.write_message(&[], &mut msg).expect("round 3 write");
    assert!(init.is_finished());
    let err = resp
        .read_message(&msg[..n3], &mut payload)
        .expect_err("mismatched static key must be rejected at the se step");
    assert!(
        matches!(err, NoiseError::Auth),
        "expected auth failure, got {err:?}"
    );
}

#[test]
fn initiator_rejects_unexpected_responder_static_at_es_step() {
    let mut init = initiator()
        .expect("initiator")
        .expect_remote_static([0xEE; 32]);
    let mut resp = responder().expect("responder");

    let mut msg = [0u8; BUF];
    let mut payload = [0u8; BUF];

    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    resp.read_message(&msg[..n1], &mut payload)
        .expect("round 1 read");

    // Round 2 (`es`) is where the initiator learns the responder's static key.
    let n2 = resp.write_message(&[], &mut msg).expect("round 2 write");
    let err = init
        .read_message(&msg[..n2], &mut payload)
        .expect_err("mismatched static key must be rejected at the es step");
    assert!(
        matches!(err, NoiseError::Auth),
        "expected auth failure, got {err:?}"
    );
}

#[test]
fn correct_expected_remote_static_is_accepted() {
    // First pass: learn the initiator's Noise static public key.
    let mut init = initiator().expect("initiator");
    let mut resp = responder().expect("responder");
    drive(&mut init, &mut resp);
    let initiator_static_pub = resp
        .remote_static()
        .expect("responder learns initiator static");

    // Second pass: pin exactly that key — the check must not fire.
    let mut init2 = initiator().expect("initiator");
    let mut resp2 = responder()
        .expect("responder")
        .expect_remote_static(initiator_static_pub);
    drive(&mut init2, &mut resp2);

    assert!(init2.is_finished() && resp2.is_finished());
    assert_eq!(
        resp2.remote_static().expect("remote static"),
        initiator_static_pub
    );
}

#[test]
fn into_transport_before_handshake_finish_fails() {
    let resp = responder().expect("responder");
    // `expect_err` would need `(NoiseTransport, NodeId): Debug`, which the
    // opaque snow transport state is not — match the error out directly.
    let err = match resp.into_transport() {
        Ok(_) => panic!("handshake is not finished yet"),
        Err(err) => err,
    };
    assert!(
        matches!(err, NetError::Handshake(_)),
        "expected handshake failure, got {err:?}"
    );
}

#[test]
fn small_output_buffer_returns_error_instead_of_panicking() {
    let mut init = initiator().expect("initiator");
    let mut tiny = [0u8; 8];

    let err = init
        .write_message(&[], &mut tiny)
        .expect_err("buffer below the XX overhead must be rejected");
    assert!(
        matches!(err, NoiseError::Input(_)),
        "expected input failure, got {err:?}"
    );

    // The handshake is still usable with a correctly sized buffer.
    let mut msg = [0u8; BUF];
    assert!(init.write_message(&[], &mut msg).is_ok());
}

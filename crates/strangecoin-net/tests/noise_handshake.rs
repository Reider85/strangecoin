//! S2-P07 — Noise XX handshake state machine.
//!
//! Three round trips, forward secrecy via ephemeral keys, tamper detection
//! and the application-level static-key binding (ADR-0020).

use strangecoin_net::noise::{NoiseError, NoiseHandshake, NoiseRole};

/// Handshake messages stay well under this: XX adds at most 96 bytes on top
/// of the payload, and S2-P07 exchanges empty payloads only.
const BUF: usize = 512;

const INITIATOR_STATIC: [u8; 32] = [0x11; 32];
const RESPONDER_STATIC: [u8; 32] = [0x22; 32];

/// Drive a full XX handshake (empty payloads) and return the three message
/// lengths, proving the exchange really took 3 round trips.
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
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC).expect("responder");

    assert_eq!(init.role(), NoiseRole::Initiator);
    assert_eq!(resp.role(), NoiseRole::Responder);
    assert!(!init.is_finished());
    assert!(!resp.is_finished());

    // Round 1: e
    let mut msg = [0u8; BUF];
    let mut payload = [0u8; BUF];
    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    assert_eq!(n1, 32, "round 1 carries the initiator ephemeral key only");
    resp.read_message(&msg[..n1], &mut payload)
        .expect("round 1 read");
    assert!(!resp.is_finished(), "responder still needs rounds 2 and 3");

    // Round 2: e, ee, s, es
    let n2 = resp.write_message(&[], &mut msg).expect("round 2 write");
    assert_eq!(n2, 96, "round 2: e(32) + s(32) + tags(32)");
    init.read_message(&msg[..n2], &mut payload)
        .expect("round 2 read");
    assert!(
        !init.is_finished(),
        "initiator finishes only after writing round 3"
    );

    // Round 3: s, se
    let n3 = init.write_message(&[], &mut msg).expect("round 3 write");
    assert_eq!(n3, 64, "round 3: s(32) + tags(32)");
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

    init.into_transport()
        .expect("initiator enters transport mode");
    resp.into_transport()
        .expect("responder enters transport mode");
}

#[test]
fn same_static_keys_yield_same_shared_secret() {
    // Both peers run the handshake with one and the same static key.
    let static_key = [0x42; 32];
    let mut init = NoiseHandshake::initiator(&static_key).expect("initiator");
    let mut resp = NoiseHandshake::responder(&static_key).expect("responder");

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
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC).expect("responder");

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
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC).expect("responder");

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
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC)
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
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC)
        .expect("initiator")
        .expect_remote_static([0xEE; 32]);
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC).expect("responder");

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
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC).expect("responder");
    drive(&mut init, &mut resp);
    let initiator_static_pub = resp
        .remote_static()
        .expect("responder learns initiator static");

    // Second pass: pin exactly that key — the check must not fire.
    let mut init2 = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
    let mut resp2 = NoiseHandshake::responder(&RESPONDER_STATIC)
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
    let resp = NoiseHandshake::responder(&RESPONDER_STATIC).expect("responder");
    let err = resp
        .into_transport()
        .expect_err("handshake is not finished yet");
    assert!(
        matches!(err, NoiseError::State(_)),
        "expected state failure, got {err:?}"
    );
}

#[test]
fn small_output_buffer_returns_error_instead_of_panicking() {
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
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

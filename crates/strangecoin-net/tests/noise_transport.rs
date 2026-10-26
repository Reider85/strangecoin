//! S2-P08 — Noise transport encryption (ChaCha20-Poly1305 framing).
//!
//! Round-trip delivery, AEAD tamper detection, replay / out-of-order nonce
//! rejection, rekey behaviour, and the `protocol::encrypt_frame` /
//! `decrypt_frame` wire format including the pre-allocation length checks
//! (invariant #16).

use strangecoin_net::noise::transport::{
    MAX_ENCRYPTED_PART, MAX_TRANSPORT_PAYLOAD, NOISE_NONCE_LEN,
};
use strangecoin_net::protocol::{decrypt_frame, encrypt_frame, ENCRYPTED_FRAME_OVERHEAD};
use strangecoin_net::{NetError, NoiseError, NoiseHandshake, NoiseTransport};

const BUF: usize = 512;

const INITIATOR_STATIC: [u8; 32] = [0x11; 32];
const RESPONDER_STATIC: [u8; 32] = [0x22; 32];

/// Run a full XX handshake and return both peers' transports.
fn handshake_pair() -> (NoiseTransport, NoiseTransport) {
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC).expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC).expect("responder");

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
    let initiator = NoiseTransport::new(init.into_transport().expect("init transport"), None);
    let responder = NoiseTransport::new(resp.into_transport().expect("resp transport"), None);
    (initiator, responder)
}

/// Encrypt one frame from `transport` and return the bytes.
fn frame(transport: &mut NoiseTransport, plaintext: &[u8]) -> Vec<u8> {
    let mut buf = vec![0u8; NOISE_NONCE_LEN + plaintext.len() + 16];
    let n = transport.encrypt(plaintext, &mut buf).expect("encrypt");
    buf.truncate(n);
    buf
}

#[test]
fn roundtrip_delivers_plaintext_and_advances_nonces() {
    let (mut a, mut b) = handshake_pair();
    assert_eq!(a.send_nonce(), 0);
    assert_eq!(b.recv_nonce(), 0);

    let mut out = [0u8; BUF];
    let f1 = frame(&mut a, b"hello transport");
    let n = b.decrypt(&f1, &mut out).expect("decrypt");
    assert_eq!(&out[..n], b"hello transport");
    assert_eq!(a.send_nonce(), 1);
    assert_eq!(b.recv_nonce(), 1);

    // Empty payload is a legal frame too (nonce header + AEAD tag only).
    let f2 = frame(&mut a, b"");
    assert_eq!(f2.len(), NOISE_NONCE_LEN + 16);
    let n = b.decrypt(&f2, &mut out).expect("decrypt empty");
    assert_eq!(n, 0);
    assert_eq!(b.recv_nonce(), 2);

    // And the reverse direction shares the session with independent counters.
    let f3 = frame(&mut b, b"reply");
    let n = a.decrypt(&f3, &mut out).expect("decrypt reply");
    assert_eq!(&out[..n], b"reply");
    assert_eq!(a.recv_nonce(), 1);
}

#[test]
fn tampered_ciphertext_returns_decrypt_error() {
    let (mut a, mut b) = handshake_pair();
    let mut f = frame(&mut a, b"attack the ciphertext, not the nonce");
    let mut out = [0u8; BUF];

    // One flipped byte inside the AEAD-protected region (past the nonce).
    let flip_at = NOISE_NONCE_LEN + 4;
    f[flip_at] ^= 0xFF;
    let err = b
        .decrypt(&f, &mut out)
        .expect_err("tampered ciphertext must not authenticate");
    assert!(
        matches!(err, NoiseError::Decrypt),
        "expected decrypt failure, got {err:?}"
    );
}

#[test]
fn tampered_nonce_header_returns_replay_error() {
    let (mut a, mut b) = handshake_pair();
    let mut f = frame(&mut a, b"corrupt the nonce header");
    let mut out = [0u8; BUF];

    f[0] ^= 0xFF; // nonce 0 -> some other value
    let err = b
        .decrypt(&f, &mut out)
        .expect_err("nonce mismatch must be caught before the AEAD tag");
    assert!(
        matches!(err, NoiseError::Replay),
        "expected replay failure, got {err:?}"
    );
    // The rejected frame leaves the session untouched.
    assert_eq!(b.recv_nonce(), 0);
}

#[test]
fn replayed_frame_is_rejected() {
    let (mut a, mut b) = handshake_pair();
    let mut out = [0u8; BUF];

    let f = frame(&mut a, b"pay 10 coins");
    b.decrypt(&f, &mut out).expect("first delivery");
    let err = b
        .decrypt(&f, &mut out)
        .expect_err("the very same frame again is a replay");
    assert!(
        matches!(err, NoiseError::Replay),
        "expected replay failure, got {err:?}"
    );
    assert_eq!(b.recv_nonce(), 1, "failed decrypt must not advance state");
}

#[test]
fn out_of_order_frame_is_rejected_then_in_order_recovers() {
    let (mut a, mut b) = handshake_pair();
    let mut out = [0u8; BUF];

    let f0 = frame(&mut a, b"first");
    let f1 = frame(&mut a, b"second");
    assert_eq!(a.send_nonce(), 2);

    // TCP cannot reorder frames, so a future-nonce frame is dropped…
    let err = b
        .decrypt(&f1, &mut out)
        .expect_err("skipping a nonce must be rejected");
    assert!(
        matches!(err, NoiseError::Replay),
        "expected replay failure, got {err:?}"
    );

    // …and because the rejection did not move recv_nonce, the in-order
    // delivery still succeeds afterwards.
    let n = b.decrypt(&f0, &mut out).expect("first frame recovers");
    assert_eq!(&out[..n], b"first");
    let n = b.decrypt(&f1, &mut out).expect("second frame follows");
    assert_eq!(&out[..n], b"second");
    assert_eq!(b.recv_nonce(), 2);
}

#[test]
fn rekey_rotates_keys_and_old_frames_are_rejected() {
    let (mut a, mut b) = handshake_pair();
    let mut out = [0u8; BUF];

    let pre = frame(&mut a, b"before the rotation");
    b.decrypt(&pre, &mut out).expect("pre-rekey delivery");

    a.rekey();
    b.rekey();

    // Both peers rotated — new frames still flow (nonce keeps counting).
    let post = frame(&mut a, b"after the rotation");
    let n = b.decrypt(&post, &mut out).expect("post-rekey delivery");
    assert_eq!(&out[..n], b"after the rotation");

    // The old ciphertext is dead: replaying it trips the nonce check.
    let err = b
        .decrypt(&pre, &mut out)
        .expect_err("stale frame after rekey must be rejected");
    assert!(
        matches!(err, NoiseError::Replay),
        "expected replay failure, got {err:?}"
    );
}

#[test]
fn old_ciphertext_fails_aead_after_key_rotation() {
    // Isolate the *key* change from the nonce check: build a frame under the
    // old key, rotate both peers before it is ever delivered, then feed it in
    // — the nonce still matches (0), so only the AEAD tag under the new key
    // can reject it.
    let (mut a, mut b) = handshake_pair();
    let mut out = [0u8; BUF];

    let stale = frame(&mut a, b"encrypted under the old key");
    a.rekey();
    b.rekey();

    let err = b
        .decrypt(&stale, &mut out)
        .expect_err("ciphertext from before the rekey must fail the tag check");
    assert!(
        matches!(err, NoiseError::Decrypt),
        "expected decrypt failure, got {err:?}"
    );
}

#[test]
fn encrypt_rejects_oversized_payload_and_undersized_buffer() {
    let (mut a, _) = handshake_pair();

    let oversized = vec![0u8; MAX_TRANSPORT_PAYLOAD + 1];
    let mut out = vec![0u8; MAX_TRANSPORT_PAYLOAD + 16];
    let err = a
        .encrypt(&oversized, &mut out)
        .expect_err("payload above the noise message limit must be rejected");
    assert!(
        matches!(err, NoiseError::Input(_)),
        "expected input failure, got {err:?}"
    );

    let mut tiny = [0u8; 8];
    let err = a
        .encrypt(b"short frame", &mut tiny)
        .expect_err("buffer below nonce + payload + tag must be rejected");
    assert!(
        matches!(err, NoiseError::Input(_)),
        "expected input failure, got {err:?}"
    );
}

#[test]
fn decrypt_rejects_truncated_frame_before_touching_snow() {
    let (_, mut b) = handshake_pair();
    let mut out = [0u8; BUF];
    let err = b
        .decrypt(&[0u8; ENCRYPTED_FRAME_OVERHEAD - 1], &mut out)
        .expect_err("frame below nonce + tag must be rejected");
    assert!(
        matches!(err, NoiseError::Input(_)),
        "expected input failure, got {err:?}"
    );
}

// ------------------------------------------------- protocol frame layer

#[test]
fn frame_roundtrip_matches_spec_wire_format() {
    let (mut a, mut b) = handshake_pair();
    let payload = b"GET_HEADERS 42";

    let mut raw = Vec::new();
    encrypt_frame(&mut a, payload, &mut raw).expect("encrypt_frame");

    // [length][nonce][ciphertext + tag: length - 8]
    let total = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    assert_eq!(total, raw.len() - 4, "length covers everything after it");
    assert_eq!(total, ENCRYPTED_FRAME_OVERHEAD + payload.len());
    assert_eq!(
        u64::from_be_bytes(raw[4..12].try_into().unwrap()),
        0,
        "first frame carries nonce 0"
    );

    let plain = decrypt_frame(&mut b, &raw).expect("decrypt_frame");
    assert_eq!(plain, payload);
}

#[test]
fn frame_roundtrip_chunks_payloads_above_the_noise_limit() {
    let (mut a, mut b) = handshake_pair();
    // 200 KB needs ceil(200000 / 65519) = 4 Noise parts.
    let payload: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();

    let mut raw = Vec::new();
    encrypt_frame(&mut a, &payload, &mut raw).expect("encrypt_frame");
    let total = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    assert_eq!(total, raw.len() - 4);
    assert!(
        total > MAX_ENCRYPTED_PART,
        "a chunked payload spans more than one part"
    );

    let plain = decrypt_frame(&mut b, &raw).expect("decrypt_frame");
    assert_eq!(plain, payload, "chunks reassemble byte-identically");
    assert_eq!(a.send_nonce(), 4, "one nonce per chunk");
    assert_eq!(b.recv_nonce(), 4);
}

#[test]
fn frame_roundtrip_empty_message() {
    let (mut a, mut b) = handshake_pair();
    let mut raw = Vec::new();
    encrypt_frame(&mut a, b"", &mut raw).expect("encrypt_frame");
    let plain = decrypt_frame(&mut b, &raw).expect("decrypt_frame");
    assert!(plain.is_empty());
}

#[test]
fn decrypt_frame_rejects_oversized_length_before_allocation() {
    let (mut a, mut b) = handshake_pair();

    // Claimed length far beyond MAX_MESSAGE_SIZE — must fail on the prefix
    // check, never reaching an allocation or a decrypt.
    let mut raw = Vec::with_capacity(64);
    raw.extend_from_slice(&u32::MAX.to_be_bytes());
    raw.extend_from_slice(&[0u8; 60]);
    let err = decrypt_frame(&mut b, &raw).expect_err("huge length must be rejected");
    assert!(
        matches!(err, NetError::Protocol(_)),
        "expected protocol failure, got {err:?}"
    );
    assert!(err.to_string().contains("size limit exceeded"));

    // A plausible length that still exceeds what is in the buffer (truncation).
    let mut good = Vec::new();
    encrypt_frame(&mut a, b"truncate me", &mut good).expect("encrypt_frame");
    good.truncate(good.len() - 3);
    let err = decrypt_frame(&mut b, &good).expect_err("truncated frame must be rejected");
    assert!(
        matches!(err, NetError::Protocol(_)),
        "expected protocol failure, got {err:?}"
    );
}

#[test]
fn decrypt_frame_propagates_tamper_and_replay_as_transport_errors() {
    let (mut a, mut b) = handshake_pair();

    let mut raw = Vec::new();
    encrypt_frame(&mut a, b"critical consensus data", &mut raw).expect("encrypt_frame");

    // Tampered ciphertext (past the length prefix and the nonce header).
    let mut tampered = raw.clone();
    tampered[4 + NOISE_NONCE_LEN + 2] ^= 0xFF;
    let err = decrypt_frame(&mut b, &tampered).expect_err("tamper must fail");
    assert!(
        matches!(err, NetError::Transport(_)),
        "expected transport failure, got {err:?}"
    );

    // Honest delivery, then the identical frame again = replay.
    let plain = decrypt_frame(&mut b, &raw).expect("first delivery");
    assert_eq!(plain, b"critical consensus data");
    let err = decrypt_frame(&mut b, &raw).expect_err("replay must fail");
    assert!(
        matches!(err, NetError::Transport(_)),
        "expected transport failure, got {err:?}"
    );
}

#[test]
fn encrypt_frame_rejects_messages_above_max_message_size() {
    let (mut a, _) = handshake_pair();
    // wire_len = 4 + len + parts * overhead — one byte past the cap trips it.
    let oversized = vec![0u8; strangecoin_net::protocol::MAX_MESSAGE_SIZE + 1];
    let mut raw = Vec::new();
    let err = encrypt_frame(&mut a, &oversized, &mut raw)
        .expect_err("message above MAX_MESSAGE_SIZE must be rejected");
    assert!(
        matches!(err, NetError::Protocol(_)),
        "expected protocol failure, got {err:?}"
    );
}

//! S2-P09 — Ed25519 node_id auth (internal P2P identity, not EOA).
//!
//! Full-path identity checks on top of the S2-P07 handshake: valid auth,
//! forged signatures, MITM with the attacker's own keys, unpinned (new) peers,
//! plus the Stage 2 determinism guarantee for derived ephemeral keys.

use strangecoin_net::noise::{NodeId, NodeKeypair, NoiseError, NoiseHandshake, NOISE_PARAMS};
use strangecoin_net::NetError;

const BUF: usize = 1024;

const INITIATOR_STATIC: [u8; 32] = [0x11; 32];
const RESPONDER_STATIC: [u8; 32] = [0x22; 32];

#[test]
fn node_id_is_blake3_of_the_ed25519_public_key() {
    let keypair = NodeKeypair::from_seed([0x01; 32]);
    let expected = NodeId::from_public_bytes(keypair.public_key());
    assert_eq!(keypair.node_id(), expected);

    // Distinct seeds must never collide into the same handle.
    let other = NodeKeypair::from_seed([0x02; 32]);
    assert_ne!(keypair.node_id(), other.node_id());
}

#[test]
fn verify_node_id_accepts_valid_signature_and_rejects_forged_ones() {
    let keypair = NodeKeypair::from_seed([0x33; 32]);
    let message = b"handshake transcript";
    let signature = keypair.sign(message);

    let verifying =
        ed25519_dalek::VerifyingKey::from_bytes(&keypair.public_key()).expect("public key parses");
    strangecoin_net::verify_node_id(&signature, message, &verifying, &keypair.node_id())
        .expect("valid signature under the expected node_id");

    // Signature over different content.
    let err = strangecoin_net::verify_node_id(
        &signature,
        b"other transcript",
        &verifying,
        &keypair.node_id(),
    )
    .expect_err("wrong message must fail");
    assert!(matches!(err, NetError::Handshake(_)), "got {err:?}");

    // Signature by a different key, presented as `keypair`.
    let impostor = NodeKeypair::from_seed([0x44; 32]);
    let forged = impostor.sign(message);
    let err = strangecoin_net::verify_node_id(&forged, message, &verifying, &keypair.node_id())
        .expect_err("foreign signature must fail");
    assert!(matches!(err, NetError::Handshake(_)), "got {err:?}");

    // Valid signature, but the caller pinned a different node_id.
    let err = strangecoin_net::verify_node_id(
        &signature,
        message,
        &verifying,
        &NodeKeypair::from_seed([0x55; 32]).node_id(),
    )
    .expect_err("node_id pin mismatch must fail");
    assert!(matches!(err, NetError::Handshake(_)), "got {err:?}");
}

#[test]
fn valid_peer_auth_is_accepted_and_returns_the_peer_node_id() {
    let init_id = NodeKeypair::from_seed([0xA1; 32]);
    let resp_id = NodeKeypair::from_seed([0xB2; 32]);

    // Both sides pin each other — the "known peer" path (peer_store / seed).
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC, &init_id, Some(resp_id.node_id()))
        .expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC, &resp_id, Some(init_id.node_id()))
        .expect("responder");

    drive(&mut init, &mut resp).expect("handshake with valid auth");

    let (mut initiator_transport, peer) = init.into_transport().expect("pinned peer authenticates");
    assert_eq!(peer, resp_id.node_id(), "responder proves its node_id");
    assert_eq!(
        initiator_transport.expected_node_id(),
        Some(*resp_id.node_id().as_bytes()),
        "verified session keeps the pinned expectation"
    );

    let (mut responder_transport, peer) = resp.into_transport().expect("pinned peer authenticates");
    assert_eq!(peer, init_id.node_id(), "initiator proves its node_id");
    assert_eq!(
        responder_transport.expected_node_id(),
        Some(*init_id.node_id().as_bytes())
    );

    // The authenticated session actually carries traffic.
    let mut buf = [0u8; 128];
    let mut out = [0u8; 128];
    let n = initiator_transport
        .encrypt(b"over an authenticated channel", &mut buf)
        .expect("encrypt");
    let n = responder_transport
        .decrypt(&buf[..n], &mut out)
        .expect("decrypt");
    assert_eq!(&out[..n], b"over an authenticated channel");
}

#[test]
fn invalid_signature_fails_at_the_auth_step() {
    // The initiator does not pin anyone (new peer), so only the *signature*
    // itself keeps the forger out — craft round 2 manually with raw snow:
    // valid AEAD, blob signed by a key that does not match the pubkey it
    // claims.
    let init_id = NodeKeypair::from_seed([0xC1; 32]);
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC, &init_id, None).expect("initiator");

    let params = NOISE_PARAMS
        .parse::<snow::params::NoiseParams>()
        .expect("params");
    let mut raw_responder = snow::Builder::new(params)
        .local_private_key(&RESPONDER_STATIC)
        .expect("local static key")
        .build_responder()
        .expect("raw responder");

    let mut msg = [0u8; BUF];
    let mut out = [0u8; BUF];

    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    raw_responder
        .read_message(&msg[..n1], &mut out)
        .expect("round 1 read on the raw side");

    // Sign the exact transcript both sides have at this point — with the
    // *wrong* key.
    let claimed_identity = NodeKeypair::from_seed([0xD4; 32]);
    let actual_signer = NodeKeypair::from_seed([0xE5; 32]);
    let transcript = init.handshake_hash().to_vec();
    let signature = actual_signer.sign(&transcript);
    let mut blob = [0u8; 96];
    blob[..32].copy_from_slice(&claimed_identity.public_key());
    blob[32..].copy_from_slice(&signature.to_bytes());

    let n2 = raw_responder
        .write_message(&blob, &mut msg)
        .expect("forged round 2 write");
    let err = init
        .read_message(&msg[..n2], &mut out)
        .expect_err("a signature must verify under the key it claims");
    assert!(
        matches!(err, NoiseError::Auth),
        "expected auth failure, got {err:?}"
    );
    assert!(!init.is_finished(), "forged identity never completes");
}

#[test]
fn mitm_with_own_noise_keys_is_dropped_on_node_id_mismatch() {
    let victim = NodeKeypair::from_seed([0x31; 32]);
    let attacker = NodeKeypair::from_seed([0x66; 32]);

    // The initiator dials expecting the victim…
    let mut init = NoiseHandshake::initiator(
        &INITIATOR_STATIC,
        &NodeKeypair::from_seed([0x11; 32]),
        Some(victim.node_id()),
    )
    .expect("initiator");
    // …and an attacker completes the handshake under their own Noise keys
    // and their own perfectly valid Ed25519 identity.
    let mut mitm = NoiseHandshake::responder(&RESPONDER_STATIC, &attacker, None).expect("mitm");

    let mut msg = [0u8; BUF];
    let mut out = [0u8; BUF];

    let n1 = init.write_message(&[], &mut msg).expect("round 1 write");
    mitm.read_message(&msg[..n1], &mut out)
        .expect("round 1 read");

    let n2 = mitm.write_message(&[], &mut msg).expect("round 2 write");
    let err = init
        .read_message(&msg[..n2], &mut out)
        .expect_err("valid signature for the wrong node_id must be dropped");
    assert!(
        matches!(err, NoiseError::Auth),
        "expected auth failure, got {err:?}"
    );
    assert!(
        !init.is_finished(),
        "MITM is dropped before any transport exists"
    );
}

#[test]
fn unknown_peer_without_expectation_is_accepted_as_unverified() {
    let init_id = NodeKeypair::from_seed([0x71; 32]);
    let resp_id = NodeKeypair::from_seed([0x72; 32]);

    // Neither side pins anybody — the ADDR-gossip "first contact" path.
    let mut init = NoiseHandshake::initiator(&INITIATOR_STATIC, &init_id, None).expect("initiator");
    let mut resp = NoiseHandshake::responder(&RESPONDER_STATIC, &resp_id, None).expect("responder");

    drive(&mut init, &mut resp).expect("handshake with unpinned peers");

    let recorded = init.peer_node_id();
    let (initiator_transport, peer) = init.into_transport().expect("accepted");
    assert_eq!(
        peer,
        resp_id.node_id(),
        "the signature still authenticates the key"
    );
    assert_eq!(
        peer,
        recorded.expect("peer id was recorded"),
        "the handshake remembers what it learned"
    );
    assert!(
        initiator_transport.expected_node_id().is_none(),
        "nothing was pinned -> the session is marked unverified"
    );

    let (responder_transport, peer) = resp.into_transport().expect("accepted");
    assert_eq!(peer, init_id.node_id());
    assert!(responder_transport.expected_node_id().is_none());
}

#[test]
fn ephemeral_keys_are_reproducible_per_seed_and_fresh_per_handshake() {
    // Stage 2 determinism checklist #3: ephemeral keys derive from the
    // long-term seed — same seed at the same handshake position ⇒ same bytes…
    let seed = [0x99; 32];
    let mut first =
        NoiseHandshake::initiator(&INITIATOR_STATIC, &NodeKeypair::from_seed(seed), None)
            .expect("initiator a");
    let mut twin =
        NoiseHandshake::initiator(&INITIATOR_STATIC, &NodeKeypair::from_seed(seed), None)
            .expect("initiator b");

    let mut msg_a = [0u8; BUF];
    let mut msg_b = [0u8; BUF];
    let n_a = first.write_message(&[], &mut msg_a).expect("write a");
    let n_b = twin.write_message(&[], &mut msg_b).expect("write b");
    assert_eq!(
        &msg_a[..n_a],
        &msg_b[..n_b],
        "the same seed must reproduce the same ephemeral key"
    );

    // …while the *same* keypair's next handshake draws a fresh one (the
    // shared counter advanced).
    let identity = NodeKeypair::from_seed(seed);
    let mut once =
        NoiseHandshake::initiator(&INITIATOR_STATIC, &identity, None).expect("first use");
    let mut twice =
        NoiseHandshake::initiator(&INITIATOR_STATIC, &identity, None).expect("second use");
    let mut msg_1 = [0u8; BUF];
    let mut msg_2 = [0u8; BUF];
    let n_1 = once.write_message(&[], &mut msg_1).expect("write 1");
    let n_2 = twice.write_message(&[], &mut msg_2).expect("write 2");
    assert_eq!(
        &msg_1[..n_1],
        &msg_a[..n_a],
        "a fresh keypair reproduces the first handshake's ephemeral"
    );
    assert_ne!(
        &msg_1[..n_1],
        &msg_2[..n_2],
        "each handshake must use a fresh ephemeral key"
    );
}

/// Drive a full XX handshake; returns the round-2 and round-3 lengths.
fn drive(
    init: &mut NoiseHandshake,
    resp: &mut NoiseHandshake,
) -> Result<(usize, usize), NoiseError> {
    let mut msg = [0u8; BUF];
    let mut payload = [0u8; BUF];

    let n1 = init.write_message(&[], &mut msg)?;
    resp.read_message(&msg[..n1], &mut payload)?;
    let n2 = resp.write_message(&[], &mut msg)?;
    init.read_message(&msg[..n2], &mut payload)?;
    let n3 = init.write_message(&[], &mut msg)?;
    resp.read_message(&msg[..n3], &mut payload)?;
    debug_assert!(init.is_finished() && resp.is_finished());
    Ok((n2, n3))
}

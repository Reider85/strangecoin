//! S2-P21 — Noise XX handshake fuzzing (ADR-0020, threat V-24).
//!
//! Contract: arbitrary bytes fed to the handshake state machine never panic
//! (every failure surfaces as a typed [`NoiseError`]); an honest handshake
//! driven with fuzzer-chosen payloads always completes on both sides with
//! matching authenticated `NodeId`s and a working encrypted transport; and
//! the initial state is deterministic — two fresh handshakes from the same
//! identity seed emit a byte-identical round-1 message (Stage 2 determinism
//! checklist #2/#3, S2-P09: ephemeral keys HKDF-derived, no OS CSPRNG).

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use strangecoin_net::{NodeKeypair, NoiseHandshake};

/// Handshake messages stay tiny (XX rounds ≤ ~250 B with the auth blob);
/// anything larger is rejected by the frame layer long before the state
/// machine sees it.
const MAX_PAYLOAD: usize = 64;
/// Scratch buffer: comfortably above the longest honest XX message.
const BUF_LEN: usize = 512;
/// Raw bytes fed as if they arrived off the wire.
const MAX_WIRE: usize = 256;

#[derive(Arbitrary, Debug)]
struct HandshakeInput {
    /// Initiator identity seed.
    a_seed: [u8; 32],
    /// Responder identity seed.
    b_seed: [u8; 32],
    /// Application payload carried on every honest round.
    payload: Vec<u8>,
    /// Garbage fed to a fresh responder's first `read_message`.
    wire: Vec<u8>,
}

fuzz_target!(|input: HandshakeInput| {
    let payload: Vec<u8> = input.payload.into_iter().take(MAX_PAYLOAD).collect();
    let wire: Vec<u8> = input.wire.into_iter().take(MAX_WIRE).collect();

    let a = NodeKeypair::from_seed(input.a_seed);
    let b = NodeKeypair::from_seed(input.b_seed);

    // --- garbage path: a fresh responder consuming arbitrary bytes -------
    let mut responder = NoiseHandshake::responder(&b.noise_static_private_key(), &b, None)
        .expect("NOISE_PARAMS is a compile-time constant");
    let mut scratch = [0u8; BUF_LEN];
    let _ = responder.read_message(&wire, &mut scratch); // typed error, never a panic

    // --- determinism: same seed ⇒ same round-1 message -------------------
    let a1 = NodeKeypair::from_seed(input.a_seed);
    let a2 = NodeKeypair::from_seed(input.a_seed);
    let mut i1 =
        NoiseHandshake::initiator(&a1.noise_static_private_key(), &a1, None).expect("fixed params");
    let mut i2 =
        NoiseHandshake::initiator(&a2.noise_static_private_key(), &a2, None).expect("fixed params");
    let mut out1 = [0u8; BUF_LEN];
    let mut out2 = [0u8; BUF_LEN];
    let n1 = i1.write_message(&[], &mut out1).expect("round 1");
    let n2 = i2.write_message(&[], &mut out2).expect("round 1");
    assert_eq!(
        &out1[..n1],
        &out2[..n2],
        "round-1 handshake message must be deterministic for the same seed"
    );

    // --- honest handshake with the fuzz payload, MITM pin on the initiator
    let mut initiator =
        NoiseHandshake::initiator(&a.noise_static_private_key(), &a, Some(b.node_id()))
            .expect("fixed params");
    let mut responder =
        NoiseHandshake::responder(&b.noise_static_private_key(), &b, None).expect("fixed params");
    let mut out = [0u8; BUF_LEN];

    let n = initiator
        .write_message(&payload, &mut out)
        .expect("round 1");
    responder
        .read_message(&out[..n], &mut scratch)
        .expect("honest round 1 read");
    let n = responder
        .write_message(&payload, &mut out)
        .expect("round 2");
    initiator
        .read_message(&out[..n], &mut scratch)
        .expect("honest round 2 read");
    let n = initiator
        .write_message(&payload, &mut out)
        .expect("round 3");
    responder
        .read_message(&out[..n], &mut scratch)
        .expect("honest round 3 read");

    assert!(initiator.is_finished() && responder.is_finished());
    let (mut initiator_transport, initiator_peer) = initiator.into_transport().unwrap();
    let (mut responder_transport, responder_peer) = responder.into_transport().unwrap();
    assert_eq!(initiator_peer, b.node_id());
    assert_eq!(responder_peer, a.node_id());

    // Encrypted traffic round-trips once the handshake completed.
    let n = initiator_transport
        .encrypt(&payload, &mut out)
        .expect("encrypt");
    let len = responder_transport
        .decrypt(&out[..n], &mut scratch)
        .expect("decrypt");
    assert_eq!(&scratch[..len], payload.as_slice());
});

//! S2-P18 — deprecated v0.8.6 messages are gone from the wire.
//!
//! The old text protocol (`GET_BLOCKCHAIN` request, `UPDATE_BLOCKCHAIN:`
//! gossip push and the JSON `Blockchain` reply) was removed together with
//! the `ChainGossip` rate-limit class. A node receiving those exact bytes
//! today must treat them as an unknown **future** message type: log + ignore,
//! never crash, never ban the peer — that is the forward-compat contract of
//! [`InboundMessage::UnknownMessage`].
//!
//! Malformed payloads of *known* requests stay protocol violations (`Err`),
//! and every valid message of the current wire format keeps decoding.

use std::io::Cursor;
use strangecoin_net::protocol::{self, InboundMessage};

/// The exact v0.8.6 request wire shape: 4-byte BE length prefix + ASCII
/// payload — bytes a pre-Stage-2 peer can still send today.
#[test]
fn legacy_get_blockchain_wire_decodes_as_unknown_message() {
    let mut wire = Vec::new();
    protocol::write_length_prefixed(&mut wire, b"GET_BLOCKCHAIN").expect("legacy frame");
    let payload = protocol::read_length_prefixed(&mut Cursor::new(wire)).expect("legacy payload");

    assert_eq!(
        protocol::decode_message(&payload).expect("never fails on unknown types"),
        InboundMessage::UnknownMessage(b"GET_BLOCKCHAIN".to_vec())
    );
}

/// The v0.8.6 gossip push (the `BLOCKCHAIN`-side of the legacy pair).
#[test]
fn legacy_update_blockchain_push_decodes_as_unknown_message() {
    let push = br#"UPDATE_BLOCKCHAIN:{"chain":[],"balances":{}}"#;
    assert_eq!(
        protocol::decode_message(push).expect("never fails on unknown types"),
        InboundMessage::UnknownMessage(push.to_vec())
    );
}

#[test]
fn future_tags_and_empty_payloads_decode_as_unknown_message() {
    assert_eq!(
        protocol::decode_message(&[0x7f, 0xde, 0xad]).expect("decode"),
        InboundMessage::UnknownMessage(vec![0x7f, 0xde, 0xad])
    );
    assert_eq!(
        protocol::decode_message(&[]).expect("decode"),
        InboundMessage::UnknownMessage(Vec::new())
    );
    // No known tag starts with an ASCII letter (the legacy protocol did).
    for byte in b'A'..=b'Z' {
        assert!(!protocol::is_known_tag(byte));
    }
}

#[test]
fn valid_sync_requests_still_decode() {
    assert_eq!(
        protocol::decode_message(&protocol::encode_get_headers(7)).expect("decode"),
        InboundMessage::GetHeaders(7)
    );
    let hashes = [[0xAAu8; 32], [0xBBu8; 32]];
    assert_eq!(
        protocol::decode_message(&protocol::encode_get_blocks(&hashes).expect("encode"))
            .expect("decode"),
        InboundMessage::GetBlocks(hashes.to_vec())
    );
}

#[test]
fn known_gossip_and_response_tags_decode_as_known_tag() {
    for tag in protocol::KNOWN_TAGS {
        if tag == protocol::MSG_GET_HEADERS || tag == protocol::MSG_GET_BLOCKS {
            continue; // fully decoded (or Err) above / below
        }
        assert_eq!(
            protocol::decode_message(&[tag]).expect("decode"),
            InboundMessage::KnownTag(tag),
            "tag {tag:#04x} must stay known"
        );
    }
}

#[test]
fn malformed_known_request_is_an_error_not_unknown() {
    // A broken GET_HEADERS body is a protocol violation (ADR-0022),
    // not an unknown type — the serving side closes the connection.
    assert!(protocol::decode_message(&[protocol::MSG_GET_HEADERS, 0]).is_err());
    // Claims one hash, carries a truncated body.
    assert!(protocol::decode_message(&[protocol::MSG_GET_BLOCKS, 0, 0, 0, 1, 0xAA]).is_err());
}

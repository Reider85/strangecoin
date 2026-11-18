//! Ed25519 response signing (V-43 MCP tool poisoning, ADR-0029).
//!
//! The payload is canonical JSON: `serde_json::Value` serializes its map as
//! a `BTreeMap` (serde_json default, no `preserve_order`), so keys are
//! sorted and the byte string is reproducible on every node (#23). The
//! signature itself is deterministic (RFC 8032 Ed25519) — the same key over
//! the same payload always yields the same bytes.

use serde_json::Value;
use strangecoin_net::NodeKeypair;

use crate::McpError;

/// Canonical JSON bytes of `value` (sorted keys, compact separators).
pub fn canonical_json(value: &Value) -> Vec<u8> {
    // `Value` is a tree of BTreeMaps — serialization cannot fail for any
    // value we construct; map the error anyway instead of panicking.
    serde_json::to_vec(value).unwrap_or_else(|_| Vec::new())
}

/// `signature` hex (128 chars) over `canonical_json(payload)`.
pub fn sign(signer: &NodeKeypair, payload: &Value) -> String {
    let bytes = canonical_json(payload);
    hex::encode(signer.sign(&bytes).to_bytes())
}

/// Verify `signature_hex` over `payload` against `signer_pub_hex`
/// (32-byte Ed25519 public key, hex). `node_id = blake3(signer_pub)` is the
/// Stage 2 binding — clients that know the peer's `node_id` (Noise auth)
/// should additionally check it against the `signer` field.
pub fn verify(signer_pub_hex: &str, payload: &Value, signature_hex: &str) -> Result<(), McpError> {
    use ed25519_dalek::{Signature, VerifyingKey};

    let pub_bytes = decode_32(signer_pub_hex)
        .map_err(|err| McpError::InvalidParams(format!("bad `signer` public key: {err}")))?;
    let verifying = VerifyingKey::from_bytes(&pub_bytes)
        .map_err(|err| McpError::InvalidParams(format!("bad `signer` public key: {err}")))?;
    let sig_bytes = decode_sig(signature_hex)?;
    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|err| McpError::InvalidParams(format!("bad signature: {err}")))?;
    verifying
        .verify_strict(&canonical_json(payload), &signature)
        .map_err(|err| McpError::Rejected(format!("signature verification failed: {err}")))
}

/// Verify a full signed envelope produced by
/// [`McpServer::signed_response`](crate::McpServer::signed_response) /
/// `signed_error`: the top-level `signature` + `signer` must authenticate
/// the `result` (or `error`) member.
pub fn verify_envelope(envelope: &Value) -> Result<(), McpError> {
    let payload = envelope
        .get("result")
        .cloned()
        .or_else(|| envelope.get("error").cloned())
        .ok_or_else(|| McpError::InvalidParams("envelope has neither result nor error".into()))?;
    let signer = envelope
        .get("signer")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::InvalidParams("envelope missing `signer`".into()))?;
    let signature = envelope
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::InvalidParams("envelope missing `signature`".into()))?;
    verify(signer, &payload, signature)
}

fn decode_32(hex_str: &str) -> Result<[u8; 32], String> {
    let cleaned = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(cleaned).map_err(|err| err.to_string())?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| format!("expected 32 bytes, got {}", bytes.len()))
}

fn decode_sig(hex_str: &str) -> Result<[u8; 64], McpError> {
    let cleaned = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes = hex::decode(cleaned)
        .map_err(|err| McpError::InvalidParams(format!("bad signature hex: {err}")))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| McpError::InvalidParams("signature must be 64 bytes".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use strangecoin_net::NodeKeypair;

    #[test]
    fn canonical_json_sorts_keys_byte_identically() {
        let a = json!({"height": 1, "hash": "aa", "nested": {"b": 2, "a": 1}});
        let b = json!({"nested": {"a": 1, "b": 2}, "hash": "aa", "height": 1});
        assert_eq!(canonical_json(&a), canonical_json(&b));
        let text = String::from_utf8(canonical_json(&a)).unwrap();
        assert!(text.find("\"hash\"").unwrap() < text.find("\"height\"").unwrap());
        assert!(text.find("\"a\"").unwrap() < text.find("\"b\"").unwrap());
    }

    #[test]
    fn sign_and_verify_roundtrip_and_tamper_detection() {
        let key = NodeKeypair::from_seed([7u8; 32]);
        let payload = json!({"value": "deterministic", "n": 3});
        let sig = sign(&key, &payload);
        let signer_hex = hex::encode(key.public_key());

        verify(&signer_hex, &payload, &sig).expect("valid signature verifies");

        let tampered = json!({"value": "deterministic", "n": 4});
        assert!(
            verify(&signer_hex, &tampered, &sig).is_err(),
            "tamper must fail"
        );

        let other = NodeKeypair::from_seed([8u8; 32]);
        assert!(
            verify(&hex::encode(other.public_key()), &payload, &sig).is_err(),
            "wrong key must fail"
        );
    }

    #[test]
    fn verify_envelope_accepts_signed_response_and_rejects_unsigned() {
        let key = NodeKeypair::from_seed([9u8; 32]);
        let payload = json!({"content": [], "isError": false});
        let envelope = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": payload,
            "signature": sign(&key, &payload),
            "signer": hex::encode(key.public_key()),
        });
        verify_envelope(&envelope).expect("signed envelope verifies");

        let unsigned = json!({"jsonrpc": "2.0", "id": 1, "result": payload});
        assert!(
            verify_envelope(&unsigned).is_err(),
            "unsigned must be rejected"
        );
    }

    #[test]
    fn signatures_are_deterministic() {
        let key = NodeKeypair::from_seed([1u8; 32]);
        let payload = json!({"x": 1});
        assert_eq!(sign(&key, &payload), sign(&key, &payload));
    }
}

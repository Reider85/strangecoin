//! ACP (Agent Communication Protocol) wire messages — AI-P03, ADR-0030.
//!
//! Four application-layer message types ride the existing Noise-authenticated
//! peer connection (no new handshake, no new transport):
//!
//! | Message | Tag | Purpose |
//! |---------|-----|---------|
//! | `ACP_NEGOTIATE` | `0x0E` | request a quote / respond with a quote |
//! | `ACP_TASK` | `0x0F` | assign a task, references on-chain escrow txid |
//! | `ACP_SETTLE` | `0x10` | release escrow to the performer (task done) |
//! | `ACP_REVOKE` | `0x11` | cancel, refund escrow (timeout or mutual) |
//!
//! **Wire shape** (spec §2): `[tag: u8][canonical JSON envelope]`, where the
//! envelope is an [`AcpEnvelope`] — the message plus the sender's agent
//! identity and a secp256k1 signature over the canonical JSON of
//! `(message, sender_agent_id)` (blake3 digest → compact 64-byte ECDSA).
//! Agent identity is a 33-byte compressed secp256k1 pubkey (ADR-0001: agents
//! are EOAs, same curve as wallets) — **not** the Noise `node_id`.
//!
//! **Canonical JSON**: `serde_json::Value` serializes its map as a `BTreeMap`
//! (serde_json default, no `preserve_order` anywhere in the workspace — the
//! AI-P02 `mcp/signing.rs` precedent), so keys are sorted and the encoding is
//! whitespace-free byte-stable. Every hex field (agent ids, txids,
//! signatures) uses lowercase hex via the `serde_hex*` helpers.
//!
//! **Determinism** (Stage 2 checklist, applied to ACP as a network-layer
//! function): no `SystemTime`/`Instant`, no RNG — signing/verification is
//! pure blake3 + secp256k1 over fixed bytes; test keys are fixed byte
//! arrays, never `OsRng`. Nonces come from the message itself (replay
//! protection lives in [`crate::acp::nonce`]).

use crate::error::NetError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `ACP_NEGOTIATE` — quote request (A → B) or quote response (B → A).
pub const MSG_ACP_NEGOTIATE: u8 = 0x0E;
/// `ACP_TASK` — task assignment referencing an on-chain escrow txid (A → B).
pub const MSG_ACP_TASK: u8 = 0x0F;
/// `ACP_SETTLE` — release escrow to the performer (A → B, or mutual).
pub const MSG_ACP_SETTLE: u8 = 0x10;
/// `ACP_REVOKE` — cancel task, refund escrow (A or B).
pub const MSG_ACP_REVOKE: u8 = 0x11;

/// Every ACP first-byte tag (kept in sync with `KNOWN_TAGS` in
/// [`super`](crate::protocol)).
pub const ACP_TAGS: [u8; 4] = [
    MSG_ACP_NEGOTIATE,
    MSG_ACP_TASK,
    MSG_ACP_SETTLE,
    MSG_ACP_REVOKE,
];

/// Wire cap for one ACP frame: envelopes are small (task_spec JSON is the
/// only variable part) — 64 KiB bounds parser memory ahead of the 32 MiB
/// global frame cap (DoS hardening, spec §2).
pub const MAX_ACP_ENVELOPE: usize = 64 * 1024;

/// Compressed secp256k1 agent identity (33 bytes, hex on the wire —
/// ADR-0028: agent identity = wallet identity, no on-chain registry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(pub [u8; 33]);

impl AgentId {
    /// Parse from 33-byte compressed secp256k1 encoding.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, NetError> {
        let arr: [u8; 33] = bytes
            .try_into()
            .map_err(|_| NetError::Protocol("agent id must be 33 bytes".into()))?;
        Ok(Self(arr))
    }

    /// Lowercase hex (66 chars) — the JSON/wire representation.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Validate that the bytes are a parseable compressed pubkey.
    pub fn verify_parseable(&self) -> Result<(), NetError> {
        secp256k1::PublicKey::from_slice(&self.0)
            .map(|_| ())
            .map_err(|err| NetError::Protocol(format!("bad agent pubkey: {err}")))
    }
}

impl Serialize for AgentId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for AgentId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex_str = String::deserialize(deserializer)?;
        let bytes = hex::decode(hex_str.trim_start_matches("0x"))
            .map_err(|err| serde::de::Error::custom(format!("agent id hex: {err}")))?;
        AgentId::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
}

/// `serde` helper: `[u8; N]` as lowercase hex.
mod serde_hex {
    use serde::Deserialize;

    pub fn serialize<const N: usize, S: serde::Serializer>(
        bytes: &[u8; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, const N: usize, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; N], D::Error> {
        let hex_str = String::deserialize(deserializer)?;
        let raw = hex::decode(hex_str.trim_start_matches("0x"))
            .map_err(|err| serde::de::Error::custom(format!("hex: {err}")))?;
        raw.as_slice()
            .try_into()
            .map_err(|_| serde::de::Error::custom(format!("expected {N} bytes")))
    }

    /// `Option<[u8; N]>` as `null` / lowercase hex (`skip_serializing_if`
    /// keeps `None` off the wire entirely).
    pub mod option {
        use serde::Deserialize;

        pub fn serialize<S: serde::Serializer>(
            bytes: &Option<[u8; 64]>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match bytes {
                Some(b) => serializer.serialize_str(&hex::encode(b)),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<[u8; 64]>, D::Error> {
            let opt = Option::<String>::deserialize(deserializer)?;
            match opt {
                None => Ok(None),
                Some(hex_str) => {
                    let raw = hex::decode(hex_str.trim_start_matches("0x"))
                        .map_err(|err| serde::de::Error::custom(format!("hex: {err}")))?;
                    let arr: [u8; 64] = raw
                        .as_slice()
                        .try_into()
                        .map_err(|_| serde::de::Error::custom("expected 64 bytes".to_string()))?;
                    Ok(Some(arr))
                }
            }
        }
    }
}

/// Bilateral quote (spec §3.1): present only in the B → A response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpQuote {
    /// Quoted price in SC (integer units — deviation from the spec §3.1
    /// example that showed a quoted string; u64 keeps the amount canonical).
    pub amount_sc: u64,
    /// Deadline the quote promises, in blocks from assignment.
    pub deadline_blocks: u64,
}

/// `ACP_NEGOTIATE` (spec §3.1): quote request (`quote: null`) or response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpNegotiate {
    /// Assigner (task owner).
    pub agent_id_a: AgentId,
    /// Performer.
    pub agent_id_b: AgentId,
    /// Monotonic per `(sender, counterparty)` — replay protection #25.
    pub agent_nonce: u64,
    /// Bilaterally-interpreted task description (no canonical schema).
    pub task_spec: Value,
    /// Quote response payload; absent on the request direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<AcpQuote>,
}

/// `ACP_TASK` (spec §3.2): assignment + on-chain escrow reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpTask {
    /// Assigner (must equal the escrow depositor — verified #24).
    pub agent_id_a: AgentId,
    /// Performer (must equal the escrow counterparty — verified #24).
    pub agent_id_b: AgentId,
    /// Monotonic per `(sender, counterparty)` — #25.
    pub agent_nonce: u64,
    /// The negotiated task.
    pub task_spec: Value,
    /// txid of the on-chain `EscrowVault.deposit` call (verified #24).
    #[serde(with = "serde_hex")]
    pub escrow_txid: [u8; 32],
    /// Block height after which A may `ACP_REVOKE` unilaterally.
    pub deadline_block: u64,
}

/// `ACP_SETTLE` (spec §3.3): release escrow to the performer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSettle {
    /// Assigner.
    pub agent_id_a: AgentId,
    /// Performer (receives the escrowed amount).
    pub agent_id_b: AgentId,
    /// Monotonic per `(sender, counterparty)` — #25.
    pub agent_nonce: u64,
    /// txid of the escrow deposit (verified #24).
    #[serde(with = "serde_hex")]
    pub escrow_txid: [u8; 32],
    /// Agent A's secp256k1 signature over `blake3(escrow_id_bytes)`.
    #[serde(with = "serde_hex")]
    pub settle_signature_a: [u8; 64],
    /// Agent B's matching signature (mutual consent).
    #[serde(with = "serde_hex")]
    pub settle_signature_b: [u8; 64],
}

/// `ACP_REVOKE` (spec §3.4): cancel + refund to A.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpRevoke {
    /// Assigner (receives the refund).
    pub agent_id_a: AgentId,
    /// Performer.
    pub agent_id_b: AgentId,
    /// Monotonic per `(sender, counterparty)` — #25.
    pub agent_nonce: u64,
    /// txid of the escrow deposit (verified #24).
    #[serde(with = "serde_hex")]
    pub escrow_txid: [u8; 32],
    /// Agent A's signature over `blake3(escrow_id_bytes)`.
    #[serde(with = "serde_hex")]
    pub revoke_signature_a: [u8; 64],
    /// Agent B's signature — present only for the mutual-cancel case.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "serde_hex::option"
    )]
    pub revoke_signature_b: Option<[u8; 64]>,
    /// `true` = unilateral timeout refund (handler checks current height >
    /// deposit deadline); `false` = mutual cancel (both signatures).
    pub deadline_passed: bool,
}

/// The four-message ACP family (internally tagged `type` on the wire —
/// spec §2.2/§3: `"type": "ACP_NEGOTIATE" | …`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AcpMessage {
    /// Quote request / response.
    #[serde(rename = "ACP_NEGOTIATE")]
    Negotiate(AcpNegotiate),
    /// Task assignment with escrow reference.
    #[serde(rename = "ACP_TASK")]
    Task(AcpTask),
    /// Escrow release.
    #[serde(rename = "ACP_SETTLE")]
    Settle(AcpSettle),
    /// Escrow refund (timeout or mutual).
    #[serde(rename = "ACP_REVOKE")]
    Revoke(AcpRevoke),
}

/// Runtime kind of an [`AcpMessage`] (dispatch + wire-tag mapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcpKind {
    /// `ACP_NEGOTIATE`.
    Negotiate,
    /// `ACP_TASK`.
    Task,
    /// `ACP_SETTLE`.
    Settle,
    /// `ACP_REVOKE`.
    Revoke,
}

impl AcpKind {
    /// The wire tag for this kind.
    pub fn tag(self) -> u8 {
        match self {
            AcpKind::Negotiate => MSG_ACP_NEGOTIATE,
            AcpKind::Task => MSG_ACP_TASK,
            AcpKind::Settle => MSG_ACP_SETTLE,
            AcpKind::Revoke => MSG_ACP_REVOKE,
        }
    }

    /// Inverse of [`tag`](Self::tag).
    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            MSG_ACP_NEGOTIATE => Some(AcpKind::Negotiate),
            MSG_ACP_TASK => Some(AcpKind::Task),
            MSG_ACP_SETTLE => Some(AcpKind::Settle),
            MSG_ACP_REVOKE => Some(AcpKind::Revoke),
            _ => None,
        }
    }

    /// Wire-name string (`"ACP_NEGOTIATE"` …) for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            AcpKind::Negotiate => "ACP_NEGOTIATE",
            AcpKind::Task => "ACP_TASK",
            AcpKind::Settle => "ACP_SETTLE",
            AcpKind::Revoke => "ACP_REVOKE",
        }
    }
}

impl AcpMessage {
    /// The family kind of this message.
    pub fn kind(&self) -> AcpKind {
        match self {
            AcpMessage::Negotiate(_) => AcpKind::Negotiate,
            AcpMessage::Task(_) => AcpKind::Task,
            AcpMessage::Settle(_) => AcpKind::Settle,
            AcpMessage::Revoke(_) => AcpKind::Revoke,
        }
    }

    /// The replay-protection nonce (spec §6: monotonic per direction).
    pub fn nonce(&self) -> u64 {
        match self {
            AcpMessage::Negotiate(m) => m.agent_nonce,
            AcpMessage::Task(m) => m.agent_nonce,
            AcpMessage::Settle(m) => m.agent_nonce,
            AcpMessage::Revoke(m) => m.agent_nonce,
        }
    }

    /// `(agent_id_a, agent_id_b)` — the task pair the message belongs to.
    pub fn pair(&self) -> (AgentId, AgentId) {
        match self {
            AcpMessage::Negotiate(m) => (m.agent_id_a, m.agent_id_b),
            AcpMessage::Task(m) => (m.agent_id_a, m.agent_id_b),
            AcpMessage::Settle(m) => (m.agent_id_a, m.agent_id_b),
            AcpMessage::Revoke(m) => (m.agent_id_a, m.agent_id_b),
        }
    }

    /// The escrow txid carried by TASK/SETTLE/REVOKE (`None` for
    /// NEGOTIATE).
    pub fn escrow_txid(&self) -> Option<[u8; 32]> {
        match self {
            AcpMessage::Negotiate(_) => None,
            AcpMessage::Task(m) => Some(m.escrow_txid),
            AcpMessage::Settle(m) => Some(m.escrow_txid),
            AcpMessage::Revoke(m) => Some(m.escrow_txid),
        }
    }
}

/// Signed ACP envelope (spec §2.1): message + sender identity + signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpEnvelope {
    /// The ACP message body.
    pub message: AcpMessage,
    /// Sender's compressed secp256k1 agent pubkey.
    pub sender_agent_id: AgentId,
    /// secp256k1 compact signature (64 bytes) over
    /// `blake3(canonical_json({message, sender_agent_id}))`.
    #[serde(with = "serde_hex")]
    pub signature: [u8; 64],
}

/// Canonical JSON bytes: `serde_json::Value` maps are `BTreeMap`s (sorted
/// keys, compact output) — the AI-P02 `mcp/signing.rs` precedent.
pub fn canonical_json(value: &Value) -> Vec<u8> {
    // Serialization of a `Value` tree cannot fail (no custom impls, no
    // non-string keys) — same justification as mcp/signing.rs.
    serde_json::to_vec(value).expect("serde_json::Value serialization cannot fail")
}

/// The exact bytes an ACP signature covers (spec §2.1 / ADR-0030): the
/// canonical JSON object of `{message, sender_agent_id}` — everything except
/// the signature field itself.
pub fn signing_bytes(message: &AcpMessage, sender: &AgentId) -> Result<Vec<u8>, NetError> {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "message".to_string(),
        serde_json::to_value(message)
            .map_err(|err| NetError::Protocol(format!("acp message encode: {err}")))?,
    );
    obj.insert(
        "sender_agent_id".to_string(),
        serde_json::to_value(sender)
            .map_err(|err| NetError::Protocol(format!("acp sender encode: {err}")))?,
    );
    Ok(canonical_json(&Value::Object(obj)))
}

/// blake3 digest of the signing payload — the secp256k1 `Message` input.
pub fn signing_digest(message: &AcpMessage, sender: &AgentId) -> Result<[u8; 32], NetError> {
    let bytes = signing_bytes(message, sender)?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

impl AcpEnvelope {
    /// Sign `message` as `sender` (deterministic ECDSA — RFC 6979 inside
    /// libsecp256k1; no RNG). Used by agent runtimes/tests — production
    /// signing keys live in the agent keystore (AI-P04/P05).
    pub fn sign(
        message: AcpMessage,
        sender: AgentId,
        secret: &secp256k1::SecretKey,
    ) -> Result<Self, NetError> {
        sender.verify_parseable()?;
        let digest = signing_digest(&message, &sender)?;
        let secp = secp256k1::Secp256k1::signing_only();
        let msg = secp256k1::Message::from_digest(digest);
        let signature = secp.sign_ecdsa(&msg, secret);
        Ok(AcpEnvelope {
            message,
            sender_agent_id: sender,
            signature: signature.serialize_compact(),
        })
    }

    /// Verify the envelope signature against `sender_agent_id` (invariant
    /// chain step 1 of the handler: authenticity before any state effect).
    pub fn verify(&self) -> Result<(), NetError> {
        self.sender_agent_id.verify_parseable()?;
        let pk = secp256k1::PublicKey::from_slice(&self.sender_agent_id.0)
            .map_err(|err| NetError::Protocol(format!("bad agent pubkey: {err}")))?;
        let digest = signing_digest(&self.message, &self.sender_agent_id)?;
        let msg = secp256k1::Message::from_digest(digest);
        let signature = secp256k1::ecdsa::Signature::from_compact(&self.signature)
            .map_err(|err| NetError::Protocol(format!("bad acp signature: {err}")))?;
        let secp = secp256k1::Secp256k1::verification_only();
        secp.verify_ecdsa(&msg, &signature, &pk)
            .map_err(|err| NetError::Protocol(format!("acp signature verify: {err}")))
    }

    /// Encode `[tag][canonical JSON]` for the wire.
    pub fn encode_wire(&self) -> Vec<u8> {
        let json = serde_json::to_vec(self).expect("AcpEnvelope serialization cannot fail");
        let mut out = Vec::with_capacity(1 + json.len());
        out.push(self.message.kind().tag());
        out.extend_from_slice(&json);
        out
    }

    /// Decode `[tag][json]`; rejects oversize frames, unknown tags and a
    /// tag that disagrees with the embedded message kind.
    pub fn decode_wire(bytes: &[u8]) -> Result<Self, NetError> {
        if bytes.len() > MAX_ACP_ENVELOPE {
            return Err(NetError::Protocol(format!(
                "acp envelope too large: {} > {MAX_ACP_ENVELOPE}",
                bytes.len()
            )));
        }
        let Some((&tag, rest)) = bytes.split_first() else {
            return Err(NetError::Protocol("empty acp frame".into()));
        };
        let kind = AcpKind::from_tag(tag)
            .ok_or_else(|| NetError::Protocol(format!("unknown acp tag 0x{tag:02x}")))?;
        let envelope: AcpEnvelope = serde_json::from_slice(rest)
            .map_err(|err| NetError::Protocol(format!("acp envelope json: {err}")))?;
        if envelope.message.kind() != kind {
            return Err(NetError::Protocol(format!(
                "acp tag/kind mismatch: tag={:?} body={:?}",
                kind,
                envelope.message.kind()
            )));
        }
        Ok(envelope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SK_A: [u8; 32] = [0x11; 32];
    const SK_B: [u8; 32] = [0x22; 32];

    fn keypair(seed: [u8; 32]) -> (secp256k1::SecretKey, AgentId) {
        let sk = secp256k1::SecretKey::from_slice(&seed).expect("fixed seed");
        let pk = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &sk);
        (sk, AgentId(pk.serialize()))
    }

    fn negotiate_msg(nonce: u64) -> AcpMessage {
        let (_, a) = keypair(SK_A);
        let (_, b) = keypair(SK_B);
        AcpMessage::Negotiate(AcpNegotiate {
            agent_id_a: a,
            agent_id_b: b,
            agent_nonce: nonce,
            task_spec: serde_json::json!({"description": "summarize X"}),
            quote: None,
        })
    }

    #[test]
    fn wire_roundtrip_all_four_kinds() {
        let (sk_a, a) = keypair(SK_A);
        let (_, b) = keypair(SK_B);
        let cases = vec![
            negotiate_msg(1),
            AcpMessage::Task(AcpTask {
                agent_id_a: a,
                agent_id_b: b,
                agent_nonce: 2,
                task_spec: serde_json::json!({"description": "t"}),
                escrow_txid: [0xAB; 32],
                deadline_block: 5000,
            }),
            AcpMessage::Settle(AcpSettle {
                agent_id_a: a,
                agent_id_b: b,
                agent_nonce: 3,
                escrow_txid: [0xAB; 32],
                settle_signature_a: [0x01; 64],
                settle_signature_b: [0x02; 64],
            }),
            AcpMessage::Revoke(AcpRevoke {
                agent_id_a: a,
                agent_id_b: b,
                agent_nonce: 4,
                escrow_txid: [0xAB; 32],
                revoke_signature_a: [0x01; 64],
                revoke_signature_b: None,
                deadline_passed: true,
            }),
        ];
        for message in cases {
            let env = AcpEnvelope::sign(message, a, &sk_a).expect("sign");
            env.verify().expect("verify");
            let wire = env.encode_wire();
            let decoded = AcpEnvelope::decode_wire(&wire).expect("decode");
            assert_eq!(decoded, env, "roundtrip must be byte-identical");
            assert_eq!(
                decoded.message.kind().tag(),
                wire[0],
                "tag byte must match kind"
            );
        }
    }

    #[test]
    fn signing_bytes_are_canonical_and_order_independent() {
        let (_, a) = keypair(SK_A);
        let msg = negotiate_msg(7);
        let direct = signing_bytes(&msg, &a).expect("bytes");
        // Same content through a reordered JSON object must hash the same
        // (BTreeMap canonicalization).
        let mut obj = serde_json::Map::new();
        obj.insert("sender_agent_id".into(), serde_json::to_value(a).unwrap());
        obj.insert("message".into(), serde_json::to_value(&msg).unwrap());
        let reordered = canonical_json(&Value::Object(obj));
        assert_eq!(direct, reordered, "key order must not change the digest");
        // Compact: serde_json emits no separator whitespace (`": "`, `", "`)
        // — spaces *inside* string values are content, not formatting.
        let text = String::from_utf8(direct).expect("utf8");
        assert!(
            !text.contains(": ") && !text.contains(", "),
            "compact: {text}"
        );
    }

    #[test]
    fn signature_detects_tampering() {
        let (sk_a, a) = keypair(SK_A);
        let mut env = AcpEnvelope::sign(negotiate_msg(9), a, &sk_a).expect("sign");
        env.verify().expect("fresh signature verifies");

        // Tampered nonce.
        let mut bad = env.clone();
        if let AcpMessage::Negotiate(m) = &mut bad.message {
            m.agent_nonce = 10;
        }
        assert!(bad.verify().is_err(), "tampered nonce must fail");

        // Tampered sender.
        let (_, b) = keypair(SK_B);
        let mut bad_sender = env.clone();
        bad_sender.sender_agent_id = b;
        assert!(bad_sender.verify().is_err(), "swapped sender must fail");

        // Flipped signature byte.
        env.signature[0] ^= 0x01;
        assert!(env.verify().is_err(), "flipped signature byte must fail");
    }

    #[test]
    fn tag_kind_mismatch_and_unknown_tag_rejected() {
        let (sk_a, a) = keypair(SK_A);
        let env = AcpEnvelope::sign(negotiate_msg(1), a, &sk_a).expect("sign");
        let mut wire = env.encode_wire();
        wire[0] = MSG_ACP_SETTLE; // negotiate body under a settle tag
        assert!(AcpEnvelope::decode_wire(&wire).is_err(), "tag mismatch");

        assert!(
            AcpEnvelope::decode_wire(&[0x7F]).is_err(),
            "unknown tag rejected"
        );
        assert!(AcpEnvelope::decode_wire(&[]).is_err(), "empty rejected");
    }

    #[test]
    fn oversize_envelope_rejected() {
        let big = vec![0u8; MAX_ACP_ENVELOPE + 1];
        assert!(AcpEnvelope::decode_wire(&big).is_err(), "oversize");
    }

    #[test]
    fn nonce_and_pair_accessors() {
        let (_, a) = keypair(SK_A);
        let (_, b) = keypair(SK_B);
        let msg = AcpMessage::Task(AcpTask {
            agent_id_a: a,
            agent_id_b: b,
            agent_nonce: 42,
            task_spec: Value::Null,
            escrow_txid: [7; 32],
            deadline_block: 1,
        });
        assert_eq!(msg.nonce(), 42);
        assert_eq!(msg.pair(), (a, b));
        assert_eq!(msg.escrow_txid(), Some([7; 32]));
        assert_eq!(msg.kind(), AcpKind::Task);
        assert_eq!(negotiate_msg(1).escrow_txid(), None);
    }
}

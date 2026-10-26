//! S2-P10 — HELLO message + capabilities negotiation.
//!
//! First application message after a finished Noise XX handshake
//! ([`super::super::noise::NoiseHandshake::into_transport`]): both peers
//! exchange one HELLO over the S2-P08 encrypted framing and agree on the
//! protocol version, the network and the capability subset they will speak.
//!
//! Wire format — one fixed-size 20-byte payload inside an encrypted frame
//! (the length prefix the prompt attributes to the wire format is the
//! [`encrypt_frame`](crate::protocol::encrypt_frame) prefix; adding another
//! one inside the ciphertext would be redundant):
//!
//! ```text
//! [version: u32 BE][network_id: u32 BE][capabilities: u32 BE][height: u64 BE]
//! ```
//!
//! Rejection rules (ROADMAP3 Этап 2 DoD: «Handshake works (test: incompatible
//! peers отвергаются)»):
//!
//! * `version < MIN_PROTO_VERSION` — Stage 2 starts at protocol version 2
//!   (Stage 1.5 spoke version 1 plaintext HELLO); an older peer cannot be
//!   expected to understand the Noise-encrypted Stage 2 transport at all;
//! * `network_id != local_network_id` — invariant #10 replay protection on
//!   the network layer: a mainnet peer must never be able to talk a testnet
//!   node into adopting its chain.
//!
//! Capabilities are a bitfield both sides advertise; the usable set is the
//! bitwise AND ([`negotiate_capabilities`]). A peer that advertises a
//! capability this node does not implement simply never sees that feature
//! requested — the negotiation result is what later prompts (S2-P14 compact
//! blocks, S2-P15 Erlay) gate on.
//!
//! **Determinism** (Stage 2 checklist, threat V-21 extension): no clock, no
//! RNG, no I/O — `height` is supplied by the caller from the chain view, the
//! codec is pure byte shuffling and negotiation is a bitwise AND. Same input
//! -> same output on every node and every run.

use crate::error::NetError;
use crate::noise::NoiseTransport;
use crate::protocol::{decrypt_frame, encrypt_frame};

/// Stage 2 protocol version. Stage 1.5 spoke version 1 (plaintext
/// `HELLO:<network_id>`); everything from Stage 2 on speaks the binary HELLO
/// over Noise and rejects anything older.
pub const MIN_PROTO_VERSION: u32 = 2;

/// Fixed wire size of one HELLO payload: `u32 + u32 + u32 + u64`.
pub const HELLO_WIRE_LEN: usize = 20;

/// Capability bits advertised in [`Hello::capabilities`] (first 8 bits of the
/// u32 field; the rest are reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Capability {
    /// Noise transport encryption (ADR-0020) — set by every Stage 2 peer.
    Noise = 0x01,
    /// BIP 152-style compact blocks (lands in S2-P14).
    CompactBlocks = 0x02,
    /// Erlay reconciliation (lands in S2-P15).
    Erlay = 0x04,
    /// Shadow-network tooling (S2-P20, Stage 6 preparation).
    Shadow = 0x08,
}

impl Capability {
    /// The bit this capability sets inside the `capabilities` bitfield.
    pub const fn bit(self) -> u32 {
        self as u32
    }
}

/// Capability set this node advertises as of Stage 2: Noise, CompactBlocks
/// and Erlay. The latter two only start gating their features when the
/// corresponding prompts land (S2-P14/S2-P15) — until then the bits are
/// forward-compatible promises, not active features. `Shadow` (0x08) stays
/// unadvertised until S2-P20.
pub const STAGE2_CAPABILITIES: u32 =
    Capability::Noise as u32 | Capability::CompactBlocks as u32 | Capability::Erlay as u32;

/// The post-handshake greeting: what protocol version this peer speaks,
/// which network it is on, which capabilities it supports and how tall its
/// best chain is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hello {
    /// Protocol version (see [`MIN_PROTO_VERSION`]).
    pub version: u32,
    /// Network id — mainnet / testnet / regtest discriminator (invariant #10).
    pub network_id: u32,
    /// Capability bitfield (see [`Capability`]).
    pub capabilities: u32,
    /// The sender's current chain height (best tip), 0-based index.
    pub height: u64,
}

impl Hello {
    /// A HELLO speaking this node's current protocol version.
    pub fn new(network_id: u32, capabilities: u32, height: u64) -> Self {
        Self {
            version: MIN_PROTO_VERSION,
            network_id,
            capabilities,
            height,
        }
    }

    /// Encode to the fixed 20-byte wire form
    /// `[version][network_id][capabilities][height]`, all big-endian.
    pub fn encode(&self) -> [u8; HELLO_WIRE_LEN] {
        let mut out = [0u8; HELLO_WIRE_LEN];
        out[0..4].copy_from_slice(&self.version.to_be_bytes());
        out[4..8].copy_from_slice(&self.network_id.to_be_bytes());
        out[8..12].copy_from_slice(&self.capabilities.to_be_bytes());
        out[12..20].copy_from_slice(&self.height.to_be_bytes());
        out
    }

    /// Decode the fixed 20-byte wire form. Anything that is not exactly
    /// [`HELLO_WIRE_LEN`] bytes is a protocol violation — the length is
    /// checked before any field is read (invariant #16 discipline).
    pub fn decode(bytes: &[u8]) -> Result<Self, NetError> {
        if bytes.len() != HELLO_WIRE_LEN {
            return Err(NetError::Protocol(format!(
                "hello: length {} != {HELLO_WIRE_LEN}",
                bytes.len()
            )));
        }
        let u32_at =
            |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let mut height_bytes = [0u8; 8];
        height_bytes.copy_from_slice(&bytes[12..20]);
        Ok(Self {
            version: u32_at(0),
            network_id: u32_at(4),
            capabilities: u32_at(8),
            height: u64::from_be_bytes(height_bytes),
        })
    }
}

/// Capabilities both peers support: the common subset of the two bitfields.
pub fn negotiate_capabilities(local: u32, remote: u32) -> u32 {
    local & remote
}

/// Accept a peer's HELLO for `local_network_id` or reject the connection:
/// protocol version below [`MIN_PROTO_VERSION`], or a foreign network
/// (invariant #10). Callers drop the connection on `Err`.
pub fn validate_hello(hello: &Hello, local_network_id: u32) -> Result<(), NetError> {
    if hello.version < MIN_PROTO_VERSION {
        return Err(NetError::Protocol(format!(
            "hello: protocol version {} below minimum {MIN_PROTO_VERSION}",
            hello.version
        )));
    }
    if hello.network_id != local_network_id {
        return Err(NetError::Protocol(format!(
            "hello: network_id mismatch: peer {}, local {local_network_id}",
            hello.network_id
        )));
    }
    Ok(())
}

/// Encode `hello` and seal it into a self-contained encrypted wire frame via
/// [`encrypt_frame`](crate::protocol::encrypt_frame) — write the returned
/// `out` to the socket with a plain write (the frame carries its own length
/// prefix).
pub fn write_hello(
    transport: &mut NoiseTransport,
    hello: &Hello,
    out: &mut Vec<u8>,
) -> Result<(), NetError> {
    let payload = hello.encode();
    encrypt_frame(transport, &payload, out)
}

/// Decrypt one complete wire frame read off the socket (including its 4-byte
/// length prefix), decode the 20-byte HELLO payload and validate it against
/// `local_network_id`. On `Err` the caller drops the connection — the peer
/// is incompatible (old version, foreign network) or the frame was corrupt.
pub fn read_hello(
    transport: &mut NoiseTransport,
    frame: &[u8],
    local_network_id: u32,
) -> Result<Hello, NetError> {
    let payload = decrypt_frame(transport, frame)?;
    let hello = Hello::decode(&payload)?;
    validate_hello(&hello, local_network_id)?;
    Ok(hello)
}

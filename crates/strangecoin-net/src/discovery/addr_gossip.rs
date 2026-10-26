//! S2-P11 — ADDR gossip: peers exchange the addresses they know.
//!
//! Wire message `ADDR [0x05]` (continues the binary tag space of
//! `protocol.rs`: 0x01..0x04 are sync messages), decoded before any
//! allocation so an oversized count cannot drive a Vec reservation
//! (invariant #16 extension, checked in `parse_addr`).
//!
//! [`AddrBook`] is the discovery-local address cache. It is deliberately
//! *not* the `PeerStore` of S2-P12: at S2-P11 the peer subsystem does not
//! exist yet (dependency order S2-P10 → S2-P11 → S2-P12). S2-P12 wraps this
//! book inside `PeerStore` and `impl AddrSender for PeerManager` — the
//! signatures below are the ones that module will call (deviation from the
//! prompt's literal `&mut PeerStore` / `&PeerManager` parameters, recorded
//! in the S2-P11 commit message).

use crate::error::NetError;
use crate::noise::NodeId;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// First payload byte of an ADDR message.
pub const MSG_ADDR: u8 = 0x05;

/// Upper bound of entries per ADDR message — enforced on decode *before*
/// any allocation and on encode (invariant #16).
pub const MAX_ADDR_ENTRIES: usize = 1000;

/// Smallest possible entry: `family(1) + IPv4(4) + port(2) + node_id(32)`.
const MIN_ENTRY_WIRE_LEN: usize = 1 + 4 + 2 + 32;

/// One advertised address: the peer's authenticated [`NodeId`] plus where
/// to dial it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AddrEntry {
    pub node_id: NodeId,
    pub addr: SocketAddr,
}

/// `ADDR [0x05] [count: u32 BE] [entry]…` with
/// `entry = [family: u8 (4|6)][addr: 4|16][port: u16 BE][node_id: 32]`.
pub fn encode_addr(entries: &[AddrEntry]) -> Result<Vec<u8>, NetError> {
    if entries.len() > MAX_ADDR_ENTRIES {
        return Err(NetError::Protocol(format!(
            "addr batch: {} entries > {MAX_ADDR_ENTRIES}",
            entries.len()
        )));
    }
    let mut out = Vec::with_capacity(5 + entries.len() * MIN_ENTRY_WIRE_LEN);
    out.push(MSG_ADDR);
    out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for entry in entries {
        match entry.addr.ip() {
            IpAddr::V4(ip) => {
                out.push(4);
                out.extend_from_slice(&ip.octets());
            }
            IpAddr::V6(ip) => {
                out.push(6);
                out.extend_from_slice(&ip.octets());
            }
        }
        out.extend_from_slice(&entry.addr.port().to_be_bytes());
        out.extend_from_slice(entry.node_id.as_bytes());
    }
    Ok(out)
}

/// Inverse of [`encode_addr`]. Rejects wrong tags, oversized counts (before
/// allocating), truncated/garbage entries and trailing bytes.
pub fn parse_addr(bytes: &[u8]) -> Result<Vec<AddrEntry>, NetError> {
    let truncated = |what: &'static str| NetError::Protocol(format!("addr message: {what}"));
    if bytes.len() < 5 || bytes[0] != MSG_ADDR {
        return Err(truncated("bad tag or missing count"));
    }
    let count = u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
    // Invariant #16: bound the count BEFORE any per-entry allocation; the
    // minimum-size pre-check below catches gross truncation in O(1).
    if count > MAX_ADDR_ENTRIES {
        return Err(NetError::Protocol(format!(
            "addr batch: count {count} > {MAX_ADDR_ENTRIES}"
        )));
    }
    if bytes.len() < 5 + count * MIN_ENTRY_WIRE_LEN {
        return Err(truncated("count exceeds payload length"));
    }
    let mut entries = Vec::with_capacity(count);
    let mut offset = 5;
    for _ in 0..count {
        let family = bytes[offset];
        offset += 1;
        let ip = match family {
            4 => {
                if offset + 4 > bytes.len() {
                    return Err(truncated("ipv4 truncation"));
                }
                let mut octets = [0u8; 4];
                octets.copy_from_slice(&bytes[offset..offset + 4]);
                offset += 4;
                IpAddr::V4(Ipv4Addr::from(octets))
            }
            6 => {
                if offset + 16 > bytes.len() {
                    return Err(truncated("ipv6 truncation"));
                }
                let mut octets = [0u8; 16];
                octets.copy_from_slice(&bytes[offset..offset + 16]);
                offset += 16;
                IpAddr::V6(Ipv6Addr::from(octets))
            }
            _ => return Err(truncated("unknown address family")),
        };
        if offset + 2 > bytes.len() {
            return Err(truncated("port truncation"));
        }
        let port = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;
        if offset + 32 > bytes.len() {
            return Err(truncated("node_id truncation"));
        }
        let mut node_id = [0u8; 32];
        node_id.copy_from_slice(&bytes[offset..offset + 32]);
        offset += 32;
        // The wire carries the node_id itself (blake3 of the Ed25519 pubkey,
        // auth.rs) — wrap, do NOT re-hash through `from_public_bytes`.
        entries.push(AddrEntry {
            node_id: NodeId(node_id),
            addr: SocketAddr::new(ip, port),
        });
    }
    if offset != bytes.len() {
        return Err(truncated("trailing bytes"));
    }
    Ok(entries)
}

/// One cache entry: [`verified`] is `true` once we completed a Noise
/// handshake with that `node_id` (S2-P12); everything learned from ADDR
/// gossip starts `false` («unverified» per the prompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddrRecord {
    pub addr: SocketAddr,
    pub verified: bool,
}

/// Discovery-local cache of known peer addresses, keyed by [`NodeId`].
#[derive(Debug, Clone, Default)]
pub struct AddrBook {
    entries: HashMap<NodeId, AddrRecord>,
}

impl AddrBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, node_id: &NodeId) -> Option<&AddrRecord> {
        self.entries.get(node_id)
    }

    pub fn contains(&self, node_id: &NodeId) -> bool {
        self.entries.contains_key(node_id)
    }

    /// Insert an address. Returns `true` when the `node_id` was unknown.
    ///
    /// Existing records are kept as-is: a known address is not demoted back
    /// to `verified = false` by gossip, and a stale gossip address never
    /// overwrites one we already track for that identity.
    pub fn insert(&mut self, node_id: NodeId, addr: SocketAddr, verified: bool) -> bool {
        match self.entries.get(&node_id) {
            Some(_) => false,
            None => {
                self.entries.insert(node_id, AddrRecord { addr, verified });
                true
            }
        }
    }

    /// Promote a known address to `verified` (S2-P12: the Noise handshake
    /// with that `node_id` completed). Returns `false` when the identity is
    /// unknown — you can only verify what the book already tracks.
    pub fn mark_verified(&mut self, node_id: &NodeId) -> bool {
        match self.entries.get_mut(node_id) {
            Some(record) => {
                let was = record.verified;
                record.verified = true;
                !was
            }
            None => false,
        }
    }

    /// Up to `max_count` entries ordered by `node_id` (Stage 2 determinism
    /// checklist #4: the same book contents produce the same payload on
    /// every node — no time, no RNG).
    pub fn top_k_sorted(&self, max_count: usize) -> Vec<AddrEntry> {
        let mut out: Vec<AddrEntry> = self
            .entries
            .iter()
            .map(|(node_id, record)| AddrEntry {
                node_id: *node_id,
                addr: record.addr,
            })
            .collect();
        out.sort();
        out.truncate(max_count);
        out
    }
}

/// A dialable, non-degenerate address worth gossiping/dialing: non-zero
/// port, no unspecified/multicast destination (those are config noise, not
/// peers).
pub fn is_dialable(addr: SocketAddr) -> bool {
    addr.port() != 0 && !addr.ip().is_unspecified() && !addr.ip().is_multicast()
}

/// Fold an ADDR message into the local book: valid entries go in as
/// «unverified», degenerate ones are skipped. Returns how many `node_id`s
/// were newly added.
pub fn handle_addr_message(entries: Vec<AddrEntry>, book: &mut AddrBook) -> usize {
    let mut added = 0;
    for entry in entries {
        if !is_dialable(entry.addr) {
            continue;
        }
        if book.insert(entry.node_id, entry.addr, false) {
            added += 1;
        }
    }
    added
}

/// How a caller delivers one encoded ADDR payload to a connected peer.
///
/// S2-P12 implements this for `PeerManager` (connection pool → encrypted
/// framing); tests use an in-memory recorder.
pub trait AddrSender {
    fn send_addr(&self, peer: &NodeId, payload: &[u8]) -> Result<(), NetError>;
}

/// Send the top-`max_count` known addresses (deterministically ordered) to
/// every recipient — one identical payload for all, recipients sorted by
/// [`NodeId`] and deduped first (determinism checklist #4; no RNG on the
/// network path).
///
/// Returns how many peers received the message; a single recipient failure
/// is logged and skipped rather than aborting the whole broadcast.
pub fn broadcast_known_addrs(
    book: &AddrBook,
    recipients: &[NodeId],
    sender: &dyn AddrSender,
    max_count: usize,
) -> Result<usize, NetError> {
    if recipients.is_empty() || max_count == 0 || book.is_empty() {
        return Ok(0);
    }
    let payload = encode_addr(&book.top_k_sorted(max_count))?;
    let mut targets: Vec<NodeId> = recipients.to_vec();
    targets.sort();
    targets.dedup();
    let mut sent = 0usize;
    for peer in targets {
        match sender.send_addr(&peer, &payload) {
            Ok(()) => sent += 1,
            Err(err) => {
                tracing::warn!(
                    target = "net::discovery",
                    peer = %peer,
                    error = %err,
                    "ADDR broadcast to peer failed, continuing"
                );
            }
        }
    }
    Ok(sent)
}

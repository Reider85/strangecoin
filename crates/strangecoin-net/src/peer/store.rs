//! S2-P12 — `PeerStore`: the persistent peer database + scoring/ban state
//! (ARCHITECT3 §3.6, ARCHITECT.md §11.6: «PeerStore — данные»).
//!
//! One map keyed by [`NodeId`] (ADR-0020 identity — bans and scores never go
//! stale when a peer rotates IPs) holds every record; a second map holds the
//! active [`BanRecord`]s; unbanned records move to an in-memory
//! `historical_bans` audit table rather than being deleted (ADR-0022).
//!
//! Persistence is the `network.json` extension of ADR-0022 with one S2-P12
//! deviation (recorded in the commit): the legacy Stage 1.5 `peers`
//! string array is left untouched (it is still read by the monolith's
//! `Node::discover_peers`), and the rich records live under the new
//! `known_peers` key alongside `banned_peers`. Load/save only ever touch
//! those two keys — `peers` and `seed_config` survive a read-modify-write.
//!
//! **Determinism** (Stage 2 checklist, threat V-21 extension): no clock, no
//! RNG, no I/O on any scoring path — [`PeerStore::update_score`] is a pure
//! function of the record's counters. Wall time appears only at the edges
//! (ban-expiry comparison in `PeerManager::tick_at`, and the age/remaining
//! stamps written to or read from `network.json`), exactly as ADR-0022
//! §Determinism prescribes. Persistence uses second-granularity ages and
//! remainings instead of wall-clock timestamps so an `Instant` (monotonic,
//! non-serializable) round-trips without pulling a datetime dependency in.

use crate::discovery::addr_gossip::{is_dialable, AddrEntry};
use crate::error::NetError;
use crate::noise::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

/// Skeleton latency penalty: `latency_norm = clamp(ms / 100, 0, 1)` costs up
/// to this much score — ≥100 ms floors the total at `1 − 0.667 ≈ 0.333`,
/// the ADR-0022 false-positive bound (a responsive peer with zero violations
/// sits in Warn and is mathematically unbannable, whatever its latency).
const SKELETON_LATENCY_FULL_PENALTY: f32 = 0.667;
/// Skeleton invalid-blocks penalty: 12 invalid blocks reach score 0.
const SKELETON_INVALID_FULL_PENALTY: f32 = 0.083;
/// Skeleton protocol-violation penalty: 20 violations reach score 0.
const SKELETON_VIOLATION_FULL_PENALTY: f32 = 0.05;

/// Score a freshly-seen peer starts at (no observations yet).
pub const INITIAL_PEER_SCORE: f32 = 1.0;

/// Why a peer was banned (prompt S2-P12 task 2; persisted as `snake_case`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BanReason {
    InvalidBlocks,
    ProtocolViolation,
    RateLimit,
    Manual,
}

/// An active ban. `until`/`banned_at` are monotonic instants — expiry is
/// compared in `tick_at`, and only the remaining/age seconds ever hit disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanRecord {
    pub until: Instant,
    pub reason: BanReason,
    pub banned_at: Instant,
    pub persistent: bool,
}

/// Everything the node knows about one peer identity.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerRecord {
    pub node_id: NodeId,
    pub addr: SocketAddr,
    pub last_seen: Instant,
    /// Recomputed by [`PeerStore::update_score`] (skeleton formula; the
    /// full ADR-0022 weighted sum lands in `scoring.rs`, S2-P17).
    pub score: f32,
    pub latency_ewma: Duration,
    pub invalid_count: u32,
    pub violation_count: u32,
    pub uptime_seconds: u64,
    /// `true` once a Noise handshake with this `node_id` completed (S2-P09);
    /// ADDR-gossip-learned peers start `false`.
    pub verified: bool,
}

/// `network.json` wire shape of one rich peer record (`known_peers` entry).
#[derive(Debug, Serialize, Deserialize)]
struct PersistedPeer {
    node_id: String,
    addr: String,
    score: f32,
    latency_ewma_ms: u64,
    invalid_count: u32,
    violation_count: u32,
    uptime_seconds: u64,
    verified: bool,
    last_seen_age_secs: u64,
}

/// `network.json` wire shape of one ban (`banned_peers` entry, ADR-0022).
#[derive(Debug, Serialize, Deserialize)]
struct PersistedBan {
    node_id: String,
    ban_remaining_secs: u64,
    banned_at_age_secs: u64,
    reason: BanReason,
    persistent: bool,
}

/// The peer database: records, active bans, ban audit trail.
#[derive(Debug, Clone, Default)]
pub struct PeerStore {
    peers: HashMap<NodeId, PeerRecord>,
    ban_state: HashMap<NodeId, BanRecord>,
    historical_bans: Vec<(NodeId, BanRecord)>,
}

impl PeerStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Insert a peer, or refresh a known one. Returns `true` when the
    /// `node_id` was unknown.
    ///
    /// Refresh semantics: the address we just proved dialable (direct
    /// connect) or gossip advertised wins; `verified` is sticky — a known
    /// record is never demoted back to unverified.
    pub fn add_peer(
        &mut self,
        node_id: NodeId,
        addr: SocketAddr,
        verified: bool,
        now: Instant,
    ) -> bool {
        match self.peers.get_mut(&node_id) {
            Some(record) => {
                record.addr = addr;
                record.verified |= verified;
                record.last_seen = now;
                false
            }
            None => {
                self.peers.insert(
                    node_id,
                    PeerRecord {
                        node_id,
                        addr,
                        last_seen: now,
                        score: INITIAL_PEER_SCORE,
                        latency_ewma: Duration::ZERO,
                        invalid_count: 0,
                        violation_count: 0,
                        uptime_seconds: 0,
                        verified,
                    },
                );
                true
            }
        }
    }

    pub fn remove_peer(&mut self, node_id: &NodeId) -> Option<PeerRecord> {
        self.peers.remove(node_id)
    }

    pub fn get_peer(&self, node_id: &NodeId) -> Option<&PeerRecord> {
        self.peers.get(node_id)
    }

    pub fn get_peer_mut(&mut self, node_id: &NodeId) -> Option<&mut PeerRecord> {
        self.peers.get_mut(node_id)
    }

    /// All known identities ordered by `node_id` bytes (Stage 2 determinism
    /// checklist #4: same store contents → same order on every node).
    pub fn peer_ids_sorted(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.peers.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Recompute `record.score` from its factors and return it.
    ///
    /// **S2-P12 skeleton** — a provisional penalty model shaped after
    /// ADR-0022 (latency floor 0.333, milestone counts 12 invalid / 20
    /// violations → 0) so the ban state machine and its tests are honest
    /// today; the full weighted quality/abuse formula, EWMA/uptime tracking
    /// and `ScoringConfig` replace this in `peer/scoring.rs` (S2-P17) per
    /// ADR-0022. Pure function — determinism checklist #1/#2.
    pub fn update_score(record: &mut PeerRecord) -> f32 {
        let latency_ms = record.latency_ewma.as_secs_f32() * 1000.0;
        let latency_norm = (latency_ms / 100.0).clamp(0.0, 1.0);
        let penalty = SKELETON_LATENCY_FULL_PENALTY * latency_norm
            + SKELETON_INVALID_FULL_PENALTY * record.invalid_count as f32
            + SKELETON_VIOLATION_FULL_PENALTY * record.violation_count as f32;
        record.score = (1.0 - penalty).clamp(0.0, 1.0);
        record.score
    }

    /// Record a ban, replacing any existing one.
    ///
    /// Replacement is what makes score-band *escalation* work: a peer that
    /// keeps misbehaving crosses TempBan first and PersistentBan later —
    /// the second call upgrades the record in place. Returns `true` when a
    /// record was written.
    pub fn ban_peer(
        &mut self,
        node_id: NodeId,
        reason: BanReason,
        until: Instant,
        banned_at: Instant,
        persistent: bool,
    ) -> bool {
        self.ban_state.insert(
            node_id,
            BanRecord {
                until,
                reason,
                banned_at,
                persistent,
            },
        );
        true
    }

    /// Lift a ban, moving the record into the `historical_bans` audit table
    /// (ADR-0022: unbanned records are not deleted).
    pub fn unban_peer(&mut self, node_id: &NodeId) -> Option<BanRecord> {
        let record = self.ban_state.remove(node_id)?;
        self.historical_bans.push((*node_id, record.clone()));
        Some(record)
    }

    pub fn is_banned(&self, node_id: &NodeId) -> bool {
        self.ban_state.contains_key(node_id)
    }

    /// Lift every ban whose `until` has passed, moving the records into the
    /// `historical_bans` audit table (ADR-0022 — expiry via `tick`). Returns
    /// the unbanned identities ordered by `node_id` (determinism #4).
    pub fn expire_bans(&mut self, now: Instant) -> Vec<NodeId> {
        let expired: Vec<NodeId> = self
            .ban_state
            .iter()
            .filter(|(_, ban)| ban.until < now)
            .map(|(node_id, _)| *node_id)
            .collect();
        for node_id in &expired {
            self.unban_peer(node_id);
        }
        expired
    }

    pub fn ban(&self, node_id: &NodeId) -> Option<&BanRecord> {
        self.ban_state.get(node_id)
    }

    pub fn historical_bans(&self) -> &[(NodeId, BanRecord)] {
        &self.historical_bans
    }

    /// Fold an ADDR-gossip batch into the store as *unverified* peers (the
    /// S2-P11 `handle_addr_message` semantics, over the single `peers` map —
    /// this store does not keep a second `AddrBook` copy). Returns how many
    /// identities were newly added.
    pub fn ingest_addr_entries(&mut self, entries: Vec<AddrEntry>, now: Instant) -> usize {
        let mut added = 0;
        for entry in entries {
            if !is_dialable(entry.addr) {
                continue;
            }
            if self.add_peer(entry.node_id, entry.addr, false, now) {
                added += 1;
            }
        }
        added
    }

    /// Up to `max_count` gossippable entries ordered by `node_id`
    /// (determinism checklist #4).
    pub fn known_addr_entries(&self, max_count: usize) -> Vec<AddrEntry> {
        let mut out: Vec<AddrEntry> = self
            .peers
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

    /// Load `known_peers` + `banned_peers` from a `network.json`-style file.
    ///
    /// * Missing file → no-op (fresh node).
    /// * Malformed top-level JSON → [`NetError::Peer`] (the operator cannot
    ///   fix a broken file we silently ignore — their bans would vanish).
    /// * Individual unparsable entries are skipped with a warning so one bad
    ///   record does not brick the whole store.
    /// * Bans whose remaining time is zero (expired while the node was down)
    ///   are dropped — an expired ban does not resurrect (ADR-0022).
    pub fn load_from_network_json(&mut self, path: &Path) -> Result<(), NetError> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(NetError::Peer(format!("reading {}: {err}", path.display()))),
        };
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|err| NetError::Peer(format!("parsing {}: {err}", path.display())))?;
        let now = Instant::now();

        if let Some(array) = value.get("known_peers").and_then(|v| v.as_array()) {
            for entry in array {
                match serde_json::from_value::<PersistedPeer>(entry.clone()) {
                    Ok(persisted) => {
                        if let Some(record) = persisted.into_record(now) {
                            self.peers.insert(record.node_id, record);
                        } else {
                            tracing::warn!(
                                target = "net::peer",
                                entry = %entry,
                                "skipping malformed known_peers entry"
                            );
                        }
                    }
                    Err(err) => tracing::warn!(
                        target = "net::peer",
                        error = %err,
                        "skipping malformed known_peers entry"
                    ),
                }
            }
        }

        if let Some(array) = value.get("banned_peers").and_then(|v| v.as_array()) {
            for entry in array {
                match serde_json::from_value::<PersistedBan>(entry.clone()) {
                    Ok(persisted) => match persisted.into_record(now) {
                        Some((node_id, record)) => {
                            self.ban_state.insert(node_id, record);
                        }
                        None => {
                            tracing::debug!(
                                target = "net::peer",
                                "dropping expired banned_peers entry at load"
                            );
                        }
                    },
                    Err(err) => tracing::warn!(
                        target = "net::peer",
                        error = %err,
                        "skipping malformed banned_peers entry"
                    ),
                }
            }
        }
        Ok(())
    }

    /// Write `known_peers` + `banned_peers` back to `network.json`, preserving
    /// every other key (`peers` legacy strings, `seed_config`, …).
    ///
    /// Only *active* bans are persisted; `historical_bans` is a runtime audit
    /// table (ADR-0022). Entries are sorted by `node_id` (checklist #4).
    pub fn save_to_network_json(&self, path: &Path) -> Result<(), NetError> {
        let now = Instant::now();
        let mut doc = match fs::read_to_string(path) {
            Ok(content) => serde_json::from_str::<serde_json::Value>(&content)
                .map_err(|err| NetError::Peer(format!("parsing {}: {err}", path.display())))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
            Err(err) => return Err(NetError::Peer(format!("reading {}: {err}", path.display()))),
        };
        if !doc.is_object() {
            doc = serde_json::json!({});
        }

        let peers: Vec<PersistedPeer> = self
            .peer_ids_sorted()
            .into_iter()
            .filter_map(|node_id| {
                self.peers
                    .get(&node_id)
                    .map(|r| PersistedPeer::from_record(r, now))
            })
            .collect();
        doc["known_peers"] = serde_json::to_value(&peers)
            .map_err(|err| NetError::Peer(format!("serializing known_peers: {err}")))?;

        let mut bans: Vec<PersistedBan> = self
            .peer_ids_sorted()
            .into_iter()
            .filter_map(|node_id| {
                self.ban_state
                    .get(&node_id)
                    .map(|b| PersistedBan::from_record(node_id, b, now))
            })
            .collect();
        // `peer_ids_sorted` covers peers only; banned identities without a
        // record (possible after a partial load) still must persist.
        let mut extra: Vec<NodeId> = self
            .ban_state
            .keys()
            .copied()
            .filter(|id| !self.peers.contains_key(id))
            .collect();
        extra.sort();
        bans.extend(extra.into_iter().filter_map(|node_id| {
            self.ban_state
                .get(&node_id)
                .map(|b| PersistedBan::from_record(node_id, b, now))
        }));
        doc["banned_peers"] = serde_json::to_value(&bans)
            .map_err(|err| NetError::Peer(format!("serializing banned_peers: {err}")))?;

        let content = serde_json::to_string_pretty(&doc)
            .map_err(|err| NetError::Peer(format!("serializing network.json: {err}")))?;
        fs::write(path, content)
            .map_err(|err| NetError::Peer(format!("writing {}: {err}", path.display())))?;
        Ok(())
    }
}

impl PersistedPeer {
    fn from_record(record: &PeerRecord, now: Instant) -> Self {
        PersistedPeer {
            node_id: hex::encode(record.node_id.as_bytes()),
            addr: record.addr.to_string(),
            score: record.score,
            latency_ewma_ms: record.latency_ewma.as_millis() as u64,
            invalid_count: record.invalid_count,
            violation_count: record.violation_count,
            uptime_seconds: record.uptime_seconds,
            verified: record.verified,
            last_seen_age_secs: now.saturating_duration_since(record.last_seen).as_secs(),
        }
    }

    fn into_record(self, now: Instant) -> Option<PeerRecord> {
        let bytes: [u8; 32] = hex::decode(&self.node_id).ok()?.try_into().ok()?;
        let addr: SocketAddr = self.addr.parse().ok()?;
        Some(PeerRecord {
            node_id: NodeId(bytes),
            addr,
            last_seen: now
                .checked_sub(Duration::from_secs(self.last_seen_age_secs))
                .unwrap_or(now),
            score: self.score,
            latency_ewma: Duration::from_millis(self.latency_ewma_ms),
            invalid_count: self.invalid_count,
            violation_count: self.violation_count,
            uptime_seconds: self.uptime_seconds,
            verified: self.verified,
        })
    }
}

impl PersistedBan {
    fn from_record(node_id: NodeId, ban: &BanRecord, now: Instant) -> Self {
        PersistedBan {
            node_id: hex::encode(node_id.as_bytes()),
            ban_remaining_secs: ban.until.saturating_duration_since(now).as_secs(),
            banned_at_age_secs: now.saturating_duration_since(ban.banned_at).as_secs(),
            reason: ban.reason,
            persistent: ban.persistent,
        }
    }

    /// `None` = the ban expired while the node was down (dropped at load).
    fn into_record(self, now: Instant) -> Option<(NodeId, BanRecord)> {
        if self.ban_remaining_secs == 0 {
            return None;
        }
        let bytes: [u8; 32] = hex::decode(&self.node_id).ok()?.try_into().ok()?;
        Some((
            NodeId(bytes),
            BanRecord {
                until: now + Duration::from_secs(self.ban_remaining_secs),
                reason: self.reason,
                banned_at: now
                    .checked_sub(Duration::from_secs(self.banned_at_age_secs))
                    .unwrap_or(now),
                persistent: self.persistent,
            },
        ))
    }
}

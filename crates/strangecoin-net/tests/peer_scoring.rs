//! S2-P17 — peer scoring (ADR-0022): the weighted formula's acceptance
//! scenarios, the ban ladder, warn recovery and ban persistence.
//!
//! All time-based assertions run on the injected-clock `tick_at` edge (the
//! `tick_at` determinism precedent) — no `sleep`, no real-time races.

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use strangecoin_net::discovery::addr_gossip::AddrEntry;
use strangecoin_net::noise::NodeKeypair;
use strangecoin_net::{BanReason, PeerManager, PeerManagerConfig};

const NETWORK_ID: u32 = 3;

fn addr(last: u8) -> SocketAddr {
    format!("10.11.0.{last}:8081").parse().unwrap()
}

/// Fresh per-test directory in the system temp dir (not `target/`).
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("strangecoin-s2p17-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn manager_with(network_json: Option<PathBuf>) -> PeerManager {
    let config = PeerManagerConfig {
        network_id: NETWORK_ID,
        network_json_path: network_json,
        ..PeerManagerConfig::default()
    };
    PeerManager::new(config, NodeKeypair::from_seed([0xC1; 32]))
}

fn node_id(seed: u8) -> strangecoin_net::NodeId {
    NodeKeypair::from_seed([seed; 32]).node_id()
}

fn register(manager: &PeerManager, seed: u8, last: u8) {
    manager.learn_addrs(vec![AddrEntry {
        node_id: node_id(seed),
        addr: addr(last),
    }]);
}

// --------------------------------------------------- false-positive bound

#[test]
fn high_latency_but_valid_peer_is_never_banned() {
    let manager = manager_with(None);
    register(&manager, 1, 1);

    // 200 ms on every message — the worst-case grading factor. The ADR-0022
    // structural bound floors the score at ~0.333 (Warn band), above the
    // 0.2 temp threshold: slow ≠ malicious.
    for _ in 0..30 {
        manager.record_latency(node_id(1), Duration::from_millis(200));
    }
    let report = manager.tick();
    assert_eq!(report.newly_banned, 0);
    assert!(!manager.is_banned(&node_id(1)), "never banned");
    let record = manager.get_peer(&node_id(1)).expect("record");
    assert!(
        record.score > 0.3 && record.score < 0.4,
        "score in the warn band, got {}",
        record.score
    );

    // Even hours of the same slow-but-valid traffic keep it unbanned.
    let later = Instant::now() + Duration::from_secs(7200);
    let report = manager.tick_at(later);
    assert_eq!(report.newly_banned, 0);
    assert!(!manager.is_banned(&node_id(1)));
}

// ----------------------------------------------------------- invalid blocks

#[test]
fn invalid_blocks_reach_temp_ban_for_one_hour() {
    let manager = manager_with(None);
    register(&manager, 2, 2);

    // ADR-0022 milestones: 7 invalids → 0.417 NoAction, 8 → 0.333 Warn,
    // 10 → 0.167 TempBan. The ladder is walked by the event-driven path.
    for _ in 0..7 {
        manager.record_invalid_block(node_id(2));
        assert!(!manager.is_banned(&node_id(2)), "not banned at <= 7");
    }
    manager.record_invalid_block(node_id(2));
    assert!(!manager.is_banned(&node_id(2)), "warn band is not a ban");
    for _ in 0..2 {
        manager.record_invalid_block(node_id(2));
    }
    assert!(
        manager.is_banned(&node_id(2)),
        "temp banned at invalid = 10"
    );

    let record = manager.get_peer(&node_id(2)).expect("record");
    assert_eq!(record.invalid_count, 10);
    assert!(
        (record.score - 0.1667).abs() < 1e-3,
        "score {}, expected ~0.167",
        record.score
    );

    let ban = manager.get_ban(&node_id(2)).expect("ban record");
    assert_eq!(ban.reason, BanReason::InvalidBlocks);
    assert!(!ban.persistent, "temp bans stay in-memory");
    let ban_len = ban.until.saturating_duration_since(ban.banned_at);
    assert!(
        ban_len >= Duration::from_secs(3599) && ban_len <= Duration::from_secs(3601),
        "temp ban lasts 1 h, got {ban_len:?}"
    );
}

// ------------------------------------------------------- protocol violations

#[test]
fn protocol_violations_reach_persistent_ban_for_thirty_days() {
    let manager = manager_with(None);
    register(&manager, 3, 3);

    // ADR-0022 milestones: 16 → 0.200 Warn boundary, 17 → 0.150 TempBan,
    // 20 → 0.000 PersistentBan.
    for _ in 0..16 {
        manager.record_protocol_violation(node_id(3));
        assert!(!manager.is_banned(&node_id(3)), "not banned at <= 16");
    }
    manager.record_protocol_violation(node_id(3));
    let ban = manager.get_ban(&node_id(3)).expect("temp ban at 17");
    assert!(!ban.persistent);
    for _ in 0..3 {
        manager.record_protocol_violation(node_id(3));
    }
    assert!(manager.is_banned(&node_id(3)));

    let record = manager.get_peer(&node_id(3)).expect("record");
    assert_eq!(record.violation_count, 20);
    assert_eq!(record.score, 0.0, "abuse reaches the full weight");

    let ban = manager.get_ban(&node_id(3)).expect("persistent ban");
    assert!(ban.persistent, "terminal state is durable");
    assert_eq!(ban.reason, BanReason::ProtocolViolation);
    let ban_len = ban.until.saturating_duration_since(ban.banned_at);
    assert!(
        ban_len >= Duration::from_secs(30 * 24 * 3600 - 60)
            && ban_len <= Duration::from_secs(30 * 24 * 3600 + 60),
        "persistent ban lasts 30 d, got {ban_len:?}"
    );
}

// ------------------------------------------------------------ warn recovery

#[test]
fn warn_state_recovers_when_quality_improves() {
    let manager = manager_with(None);
    register(&manager, 4, 4);

    // Degraded latency pulls the score into the Warn band.
    for _ in 0..30 {
        manager.record_latency(node_id(4), Duration::from_millis(200));
    }
    let report = manager.tick();
    assert_eq!(report.warned, 1, "score in the warn band");
    assert_eq!(report.newly_banned, 0);

    // No new violations + improved samples: the EWMA converges toward
    // 10 ms and the score climbs back above the warn threshold (the ADR
    // recovery clause — «improved quality, no new violations»).
    for _ in 0..60 {
        manager.record_latency(node_id(4), Duration::from_millis(10));
    }
    let record = manager.get_peer(&node_id(4)).expect("record");
    assert!(record.score > 0.9, "score recovered, got {}", record.score);

    let report = manager.tick();
    assert_eq!(report.warned, 0, "warn cleared");
    assert_eq!(report.newly_banned, 0);
    assert!(!manager.is_banned(&node_id(4)));
}

#[test]
fn expired_temp_ban_gets_one_tick_grace_before_rearm() {
    // ADR-0022 counters are cumulative in memory: after a temp ban expires
    // the peer is free for one full tick window (grace), then the ladder
    // re-arms on the next pass if the score is still below the threshold.
    let manager = manager_with(None);
    register(&manager, 5, 5);
    for _ in 0..10 {
        manager.record_invalid_block(node_id(5));
    }
    assert!(manager.is_banned(&node_id(5)));

    let after = Instant::now() + Duration::from_secs(3601);
    let report = manager.tick_at(after);
    assert_eq!(report.expired_bans, 1);
    assert!(!manager.is_banned(&node_id(5)), "grace window after expiry");

    let next = after + Duration::from_secs(60);
    let report = manager.tick_at(next);
    assert_eq!(report.newly_banned, 1, "ladder re-arms on the next tick");
    assert!(manager.is_banned(&node_id(5)));
}

// ------------------------------------------------------- restart durability

#[test]
fn persistent_ban_survives_restart() {
    let dir = temp_dir("persist-restart");
    let path = dir.join("network.json");
    let manager = manager_with(Some(path.clone()));
    register(&manager, 6, 6);
    for _ in 0..20 {
        manager.record_protocol_violation(node_id(6));
    }
    assert!(manager.is_banned(&node_id(6)));

    // "Restart": a new manager on the same network.json loads the ban.
    let reloaded = manager_with(Some(path));
    assert!(
        reloaded.is_banned(&node_id(6)),
        "persistent ban reloaded from network.json"
    );
    let ban = reloaded.get_ban(&node_id(6)).expect("ban after reload");
    assert!(ban.persistent);
    assert_eq!(ban.reason, BanReason::ProtocolViolation);
    assert!(ban.until > Instant::now(), "remaining time preserved");
}

// ---------------------------------------------------- rate-limiter feed

#[test]
fn rate_limit_violations_feed_the_protocol_violation_channel() {
    // The S2-P16 seam: refused messages accumulate as protocol_violations
    // and eventually cross the persistent threshold through scoring.
    let manager = manager_with(None);
    register(&manager, 7, 7);
    for _ in 0..20 {
        manager.record_rate_violation(node_id(7));
    }
    let record = manager.get_peer(&node_id(7)).expect("record");
    assert_eq!(record.violation_count, 20);
    assert!(manager.is_banned(&node_id(7)));
    let ban = manager.get_ban(&node_id(7)).expect("ban");
    assert_eq!(ban.reason, BanReason::RateLimit);
    assert!(ban.persistent);
}

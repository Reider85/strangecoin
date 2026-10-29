//! S2-P22 — peer scoring integration (DoD #6: malicious peers banned after
//! N violations, false positives survive, bans persist across restart).
//!
//! Thresholds come from `config.toml` via `Config::peer_manager_config()`
//! (`peer_scoring_*` keys, ADR-0022); all time assertions run on the
//! injected-clock `tick_at` edge — no sleeps.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::*;
use strangecoin::config::Config;
use strangecoin_net::discovery::addr_gossip::AddrEntry;
use strangecoin_net::{BanReason, NodeKeypair, PeerManager};

/// Manager built from the operator surface (`config.toml` defaults):
/// scoring weights/thresholds/durations exactly as `Config` maps them.
fn manager_from_config(network_json: Option<PathBuf>) -> PeerManager {
    let mut config = Config::default().peer_manager_config();
    config.network_json_path = network_json;
    PeerManager::new(config, NodeKeypair::from_seed([0xC1; 32]))
}

fn node_id(seed: [u8; 32]) -> strangecoin_net::NodeId {
    NodeKeypair::from_seed(seed).node_id()
}

fn register(manager: &PeerManager, peer: strangecoin_net::NodeId, last: u8) {
    manager.learn_addrs(vec![AddrEntry {
        node_id: peer,
        addr: format!("10.13.0.{last}:8081").parse().expect("addr"),
    }]);
}

/// Malicious peer: invalid blocks drive the ADR-0022 ladder past the
/// `config.toml` temp-ban threshold — banned for exactly the configured
/// duration, with the configured reason.
#[test]
fn malicious_peer_banned_after_configured_invalid_block_threshold() {
    // The threshold genuinely comes from the config surface (S2-P17 keys).
    let config = Config::default().peer_manager_config();
    assert_eq!(
        config.scoring.invalid_full_weight, 6,
        "invalid_full_weight from peer_scoring_invalid_full_weight"
    );
    assert!(
        (config.scoring.temp_ban_threshold - 0.2).abs() < 1e-6,
        "temp threshold from peer_ban_threshold, got {}",
        config.scoring.temp_ban_threshold
    );
    assert_eq!(
        config.scoring.temp_ban_duration,
        Duration::from_secs(3600),
        "temp duration from peer_scoring_temp_ban_duration_sec"
    );

    let manager = manager_from_config(None);
    let malicious = node_id([0x21; 32]);
    register(&manager, malicious, 1);

    // Walk the ladder event by event: warn band is not a ban, the threshold
    // crossing is (10 invalids with the default weights → score ~0.167).
    for i in 1..=9 {
        manager.record_invalid_block(malicious);
        if i < 10 {
            assert!(
                !manager.is_banned(&malicious),
                "no ban before the threshold (at {i})"
            );
        }
    }
    manager.record_invalid_block(malicious);
    assert!(
        manager.is_banned(&malicious),
        "temp banned at the configured threshold"
    );

    let ban = manager.get_ban(&malicious).expect("ban record");
    assert_eq!(ban.reason, BanReason::InvalidBlocks);
    assert!(!ban.persistent, "invalid blocks start as a temp ban");
    let ban_len = ban.until.saturating_duration_since(ban.banned_at);
    assert!(
        ban_len >= Duration::from_secs(3599) && ban_len <= Duration::from_secs(3601),
        "configured 1 h temp ban, got {ban_len:?}"
    );
}

/// False-positive bound: a slow-but-valid peer floors in the warn band and
/// is never banned — not now, not hours later on the injected clock.
#[test]
fn high_latency_but_valid_peer_is_never_banned() {
    let manager = manager_from_config(None);
    let slow = node_id([0x31; 32]);
    register(&manager, slow, 2);

    for _ in 0..30 {
        manager.record_latency(slow, Duration::from_millis(200));
    }
    let report = manager.tick();
    assert_eq!(report.newly_banned, 0, "slow is not malicious");
    assert!(!manager.is_banned(&slow), "false-positive guard: no ban");

    let record = manager.get_peer(&slow).expect("record");
    assert!(
        record.score > 0.3 && record.score < 0.4,
        "warn band, got {}",
        record.score
    );

    // Hours of the same slow-but-valid traffic still ban nothing.
    let later = Instant::now() + Duration::from_secs(7200);
    let report = manager.tick_at(later);
    assert_eq!(report.newly_banned, 0);
    assert!(!manager.is_banned(&slow));
}

/// A persistent ban is durable: a fresh manager on the same `network.json`
/// (a restart) reloads it with reason and remaining time intact.
#[test]
fn persistent_ban_survives_restart_via_network_json() {
    let dir = TestDir::new("net_scoring_restart");
    std::fs::create_dir_all(dir.path()).expect("create temp dir");
    let path = dir.path().join("network.json");

    let manager = manager_from_config(Some(path.clone()));
    let abuser = node_id([0x41; 32]);
    register(&manager, abuser, 3);
    for _ in 0..20 {
        manager.record_protocol_violation(abuser);
    }
    assert!(manager.is_banned(&abuser), "persistent ban reached");
    let ban = manager.get_ban(&abuser).expect("ban record");
    assert!(ban.persistent, "terminal state is durable");
    assert_eq!(ban.reason, BanReason::ProtocolViolation);

    // Restart: a new manager loads network.json from disk.
    let reloaded = manager_from_config(Some(path));
    assert!(
        reloaded.is_banned(&abuser),
        "persistent ban reloaded from network.json"
    );
    let ban = reloaded.get_ban(&abuser).expect("ban after reload");
    assert!(ban.persistent);
    assert_eq!(ban.reason, BanReason::ProtocolViolation);
    assert!(ban.until > Instant::now(), "remaining time preserved");
}

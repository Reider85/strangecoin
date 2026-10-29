//! S2-P22 — rate-limiting integration (DoD #8: a 1000 INV/s spammer is
//! banned after the threshold; a legitimate peer under the budget never is).
//!
//! Bucket tunables come from `config.toml` via `Config::rate_limit_config()`
//! (`rate_limit_*` keys, ADR-0023) through the monolith's `network` shim.
//! All bucket arithmetic runs on the clock-injected `_at` API — no sleeps,
//! no real-time races.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::*;
use strangecoin::config::Config;
use strangecoin::network::{MessageType, RateLimitError, RateLimiter};
use strangecoin_net::discovery::addr_gossip::AddrEntry;
use strangecoin_net::{BanReason, NodeKeypair, PeerManager, PeerManagerConfig};

fn addr(last: u8) -> std::net::SocketAddr {
    format!("10.14.0.{last}:8081").parse().expect("addr")
}

fn node_id(seed: [u8; 32]) -> strangecoin_net::NodeId {
    NodeKeypair::from_seed(seed).node_id()
}

fn manager_with(network_json: Option<PathBuf>) -> PeerManager {
    let mut config = PeerManagerConfig {
        network_id: strangecoin::consensus::CHAIN_ID_REGTEST,
        ..PeerManagerConfig::default()
    };
    config.network_json_path = network_json;
    PeerManager::new(config, NodeKeypair::from_seed([0x51; 32]))
}

fn register(manager: &PeerManager, peer: strangecoin_net::NodeId, peer_addr: std::net::SocketAddr) {
    manager.learn_addrs(vec![AddrEntry {
        node_id: peer,
        addr: peer_addr,
    }]);
}

/// Spammer pushing 1000 INV at one instant: the config's INV bucket drains,
/// refusals count violations, the 5th violation inside the 60 s window is
/// the ban — forwarded through `apply_rate_verdict` into the peer manager
/// and persisted in `network.json`.
#[test]
fn spammer_1000_inv_per_second_banned_after_five_violations_and_persists() {
    let rl_config = Config::default().rate_limit_config();
    // The threshold really is the config surface (S2-P16 keys).
    assert_eq!(rl_config.ban_threshold, 5, "rate_limit_ban_threshold");
    assert_eq!(rl_config.ban_window, Duration::from_secs(60), "ban window");
    assert_eq!(
        rl_config.inv,
        strangecoin_net::rate_limiter::BucketSpec::new(100, 10),
        "INV bucket 100/10 from config"
    );

    let dir = TestDir::new("net_rate_spammer");
    std::fs::create_dir_all(dir.path()).expect("create temp dir");
    let path = dir.path().join("network.json");
    let manager = manager_with(Some(path.clone()));
    let spammer = node_id([0x61; 32]);
    let spam_addr = addr(1);
    register(&manager, spammer, spam_addr);

    let limiter = RateLimiter::new(rl_config.clone());
    let now = Instant::now();
    let ban_duration = rl_config.ban_duration;

    // Burst flood: verdicts flow to the peer manager on every message
    // (the production `apply_rate_verdict` seam, S2-P16 → S2-P12).
    let mut banned_at = None;
    for i in 1..=1000u32 {
        let verdict = limiter.check_rate_at(now, spam_addr, MessageType::Inv);
        manager.apply_rate_verdict(spammer, &verdict, ban_duration);
        match verdict {
            Ok(()) => {}
            Err(RateLimitError::Exceeded) => {
                assert_eq!(
                    limiter.violation_count(spam_addr),
                    i - 100,
                    "each refused INV is exactly one violation"
                );
            }
            Err(RateLimitError::Ban) => {
                banned_at = Some(i);
                break;
            }
        }
    }
    assert_eq!(banned_at, Some(105), "ban exactly at violation 5");

    // Fast path (limiter) and slow path (peer manager) agree.
    assert!(limiter.is_banned_at(now, spam_addr));
    assert_eq!(limiter.violation_count(spam_addr), 5);
    assert!(
        manager.is_banned(&spammer),
        "peer manager banned the spammer"
    );
    let ban = manager.get_ban(&spammer).expect("ban record");
    assert_eq!(ban.reason, BanReason::RateLimit);
    assert!(ban.until > now, "ban_duration applied");

    // Ban state persists: a fresh manager reloads network.json.
    drop(manager);
    let reloaded = manager_with(Some(path));
    assert!(
        reloaded.is_banned(&spammer),
        "rate-limit ban reloaded from network.json"
    );
}

/// Legitimate peer staying inside the configured budget (100 INV burst +
/// 100 INV per 10 s refill): zero refusals over six windows, never banned,
/// and the peer manager's violation counter stays untouched.
#[test]
fn legitimate_peer_under_100_inv_per_10s_never_banned() {
    let rl_config = Config::default().rate_limit_config();
    let manager = manager_with(None);
    let honest = node_id([0x71; 32]);
    let honest_addr = addr(2);
    register(&manager, honest, honest_addr);

    let limiter = RateLimiter::new(rl_config.clone());
    let t0 = Instant::now();

    let mut now = t0;
    for round in 0..6 {
        for _ in 0..100 {
            let verdict = limiter.check_rate_at(now, honest_addr, MessageType::Inv);
            assert!(
                verdict.is_ok(),
                "round {round}: honest peer inside budget refused: {verdict:?}"
            );
            manager.apply_rate_verdict(honest, &verdict, rl_config.ban_duration);
        }
        now += Duration::from_secs(10);
    }

    assert_eq!(limiter.violation_count(honest_addr), 0, "zero refusals");
    assert!(!limiter.is_banned_at(now, honest_addr), "never banned");
    assert!(!manager.is_banned(&honest), "peer manager untouched");
    let record = manager.get_peer(&honest).expect("peer record");
    assert_eq!(
        record.violation_count, 0,
        "no violations recorded against a well-behaved peer"
    );
}

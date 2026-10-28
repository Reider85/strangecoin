//! S2-P16 — rate limiting (ADR-0023): token buckets per peer, violation
//! window, ban on threshold, and the peer_manager slow-path integration.
//!
//! All bucket arithmetic is driven through the clock-injected `_at` API (the
//! `tick_at` determinism precedent) — no `sleep`, no real-time races.

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use strangecoin_net::discovery::addr_gossip::AddrEntry;
use strangecoin_net::noise::NodeKeypair;
use strangecoin_net::rate_limiter::{MessageType, RateLimitError, RateLimiter};
use strangecoin_net::{BanReason, PeerManager, PeerManagerConfig};

const NETWORK_ID: u32 = 3;

fn addr(last: u8) -> SocketAddr {
    format!("10.9.0.{last}:8081").parse().unwrap()
}

/// Fresh per-test directory in the system temp dir (not `target/`).
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("strangecoin-s2p16-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn manager_with(network_json: Option<PathBuf>) -> PeerManager {
    let mut config = PeerManagerConfig {
        network_id: NETWORK_ID,
        ..PeerManagerConfig::default()
    };
    config.network_json_path = network_json;
    PeerManager::new(config, NodeKeypair::from_seed([0x51; 32]))
}

fn node_id(seed: u8) -> strangecoin_net::NodeId {
    NodeKeypair::from_seed([seed; 32]).node_id()
}

// ------------------------------------------------------------- spammer ban

#[test]
fn spammer_1000_inv_per_second_banned_after_five_violations() {
    let limiter = RateLimiter::default(); // ADR-0023 defaults: INV 100/10
    let now = Instant::now();
    let spammer = addr(1);

    // "1000 INV/s" simulated at one instant: the first 100 drain the INV
    // sub-bucket, every later message is refused and counts a violation.
    let mut banned_at = None;
    for i in 1..=1000 {
        match limiter.check_rate_at(now, spammer, MessageType::Inv) {
            Ok(()) => {}
            Err(RateLimitError::Exceeded) => {
                assert_eq!(
                    limiter.violation_count(spammer),
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

    // Threshold is 5 violations: 100 accepted + 4 Exceeded + the 5th = Ban.
    assert_eq!(banned_at, Some(105), "ban exactly at violation 5");
    assert!(limiter.is_banned_at(now, spammer));
    assert_eq!(limiter.violation_count(spammer), 5);

    // While banned every message is Ban and the counter does not grow.
    assert_eq!(
        limiter.check_rate_at(now, spammer, MessageType::Inv),
        Err(RateLimitError::Ban)
    );
    assert_eq!(limiter.violation_count(spammer), 5);

    // Other types on the same address are blocked too (ban is per-address).
    assert_eq!(
        limiter.check_rate_at(now, spammer, MessageType::Tx),
        Err(RateLimitError::Ban)
    );
}

// ------------------------------------------------------ legitimate peer OK

#[test]
fn legitimate_peer_under_100_inv_per_10s_never_banned() {
    let limiter = RateLimiter::default();
    let t0 = Instant::now();
    let honest = addr(2);

    // 100 INV burst + 5 rounds of 100 INV every 10 s — exactly the allowed
    // budget (INV: 100 burst, 10 tokens/s refill => 100 per 10 s).
    let mut now = t0;
    for round in 0..6 {
        for _ in 0..100 {
            limiter
                .check_rate_at(now, honest, MessageType::Inv)
                .unwrap_or_else(|e| panic!("round {round}: honest peer refused: {e}"));
        }
        now += Duration::from_secs(10);
    }

    assert_eq!(limiter.violation_count(honest), 0, "zero refusals");
    assert!(!limiter.is_banned_at(now, honest), "never banned");
}

// ------------------------------------------------------------ burst support

#[test]
fn burst_within_main_capacity_is_ok_and_over_capacity_bans() {
    let limiter = RateLimiter::default();
    let now = Instant::now();

    // 200 messages inside the main bucket capacity (1000) — all accepted.
    // Type without a configured sub-bucket (Hello) charges the main bucket
    // only; the literal "200 INV" of the prompt contradicts INV=100 burst
    // (ADR-0023 worked example), see the INV cap test below.
    let burst_ok = addr(3);
    for _ in 0..200 {
        limiter
            .check_rate_at(now, burst_ok, MessageType::Hello)
            .expect("200 msgs < main capacity 1000 must pass");
    }
    assert_eq!(limiter.violation_count(burst_ok), 0);

    // 1500 at once: 1000 pass, then refusals escalate to the ban.
    let burst_over = addr(4);
    let mut banned_at = None;
    for i in 1..=1500 {
        match limiter.check_rate_at(now, burst_over, MessageType::Hello) {
            Ok(()) => {}
            Err(RateLimitError::Exceeded) => {}
            Err(RateLimitError::Ban) => {
                banned_at = Some(i);
                break;
            }
        }
    }
    // 1000 accepted, refusals 1001..=1005 -> the 5th refusal is the Ban.
    assert_eq!(banned_at, Some(1005), "capacity 1000 + 5 violations");
    assert!(limiter.is_banned_at(now, burst_over));
    assert_eq!(limiter.violation_count(burst_over), 5);

    // INV sub-bucket cap (ADR-0023 worked example): 100 OK, 101 Exceeded.
    let inv_cap = addr(5);
    for _ in 0..100 {
        limiter
            .check_rate_at(now, inv_cap, MessageType::Inv)
            .expect("INV burst 100 must pass");
    }
    assert_eq!(
        limiter.check_rate_at(now, inv_cap, MessageType::Inv),
        Err(RateLimitError::Exceeded),
        "INV sub-bucket caps at 100 burst (ADR-0023)"
    );
    assert_eq!(limiter.violation_count(inv_cap), 1, "one refusal, no ban");
    assert!(!limiter.is_banned_at(now, inv_cap));
}

// ------------------------------------------------------- per-type caps

#[test]
fn per_type_block_cap_thirty_per_window() {
    let limiter = RateLimiter::default();
    let now = Instant::now();
    let peer = addr(6);

    // BLOCK: 30 burst — 30 pass, the 31st is refused.
    for i in 1..=30 {
        limiter
            .check_rate_at(now, peer, MessageType::Block)
            .unwrap_or_else(|e| panic!("BLOCK {i} within cap must pass: {e}"));
    }
    assert_eq!(
        limiter.check_rate_at(now, peer, MessageType::Block),
        Err(RateLimitError::Exceeded),
        "31 BLOCK/10s refused (window not yet elapsed, no refill)"
    );
    assert_eq!(limiter.violation_count(peer), 1);
    assert!(
        !limiter.is_banned_at(now, peer),
        "1 violation < threshold 5"
    );

    // Per-type isolation: TX has its own bucket — still fully open.
    for _ in 0..100 {
        limiter
            .check_rate_at(now, peer, MessageType::Tx)
            .expect("TX unaffected by BLOCK exhaustion");
    }

    // Refill: 3 tokens/s for BLOCK — after 10 simulated seconds 30 tokens
    // are back and the bucket accepts again.
    let later = now + Duration::from_secs(10);
    for _ in 0..30 {
        limiter
            .check_rate_at(later, peer, MessageType::Block)
            .expect("BLOCK bucket refilled after 10s");
    }
}

// ------------------------------------------------- peer_manager integration

#[test]
fn rate_limit_ban_feeds_peer_manager_and_persists_in_network_json() {
    let dir = temp_dir("persist");
    let path = dir.join("network.json");
    let manager = manager_with(Some(path.clone()));

    // record_rate_violation ignores unknown peers — register first.
    let peer_id = node_id(0x61);
    let peer_addr = addr(7);
    manager.learn_addrs(vec![AddrEntry {
        node_id: peer_id,
        addr: peer_addr,
    }]);

    let limiter = RateLimiter::default();
    let now = Instant::now();
    let ban_duration = Duration::from_secs(300);

    // Spam and forward every verdict to the slow path (ADR-0023 diagram).
    let mut banned = false;
    for _ in 0..1000 {
        let verdict = limiter.check_rate_at(now, peer_addr, MessageType::Inv);
        manager.apply_rate_verdict(peer_id, &verdict, ban_duration);
        if matches!(verdict, Err(RateLimitError::Ban)) {
            banned = true;
            break;
        }
    }
    assert!(banned, "fast path must reach the ban threshold");

    // Slow path: violations recorded + explicit RateLimit ban.
    let record = manager.get_peer(&peer_id).expect("peer record");
    assert_eq!(record.violation_count, 5, "4 Exceeded + 1 Ban = 5 refusals");
    assert!(manager.is_banned(&peer_id));
    let ban = manager.get_ban(&peer_id).expect("ban record");
    assert_eq!(ban.reason, BanReason::RateLimit);
    assert!(ban.until > Instant::now(), "ban still active");

    // Ban state survives a restart: a fresh manager loads network.json.
    drop(manager);
    let reloaded = manager_with(Some(path));
    assert!(
        reloaded.is_banned(&peer_id),
        "rate-limit ban must be reloaded from network.json"
    );
    let ban = reloaded.get_ban(&peer_id).expect("reloaded ban");
    assert_eq!(ban.reason, BanReason::RateLimit);
}

// ------------------------------------------------------- honest verdict no-op

#[test]
fn ok_verdict_leaves_peer_manager_untouched() {
    let manager = manager_with(None);
    let peer_id = node_id(0x62);
    manager.learn_addrs(vec![AddrEntry {
        node_id: peer_id,
        addr: addr(8),
    }]);

    manager.apply_rate_verdict(peer_id, &Ok(()), Duration::from_secs(300));

    let record = manager.get_peer(&peer_id).expect("peer record");
    assert_eq!(record.violation_count, 0);
    assert!(!manager.is_banned(&peer_id));
}

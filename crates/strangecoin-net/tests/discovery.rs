//! S2-P11 — peer discovery: seed loading, DNSSEC gate, ADDR gossip and the
//! multi-source bootstrap.
//!
//! Everything here is simulated — mock DNS closures, in-memory address
//! books, `#[tokio::test(start_paused = true)]` for the 30s budget — no
//! real network, no `network.json` next to the test binary (the
//! `NETWORK_TEST_LOCK` file of the top-level suite is a different binary
//! and untouched).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;
use strangecoin_net::discovery::addr_gossip::{
    broadcast_known_addrs, encode_addr, handle_addr_message, parse_addr, AddrBook, AddrEntry,
    AddrSender, MAX_ADDR_ENTRIES, MSG_ADDR,
};
use strangecoin_net::discovery::dnssec::{apply_dnssec_policy, DnsAnswer};
use strangecoin_net::discovery::seeds::{load_seeds, SeedConfig, DEFAULT_SEED_PORT};
use strangecoin_net::discovery::{bootstrap, PeerSource, BOOTSTRAP_BUDGET, MIN_BOOTSTRAP_PEERS};
use strangecoin_net::{NetError, NodeId, NodeKeypair};

// ------------------------------------------------------------------ helpers

fn node_id(seed: u8) -> NodeId {
    NodeKeypair::from_seed([seed; 32]).node_id()
}

fn ip4(last: u8) -> IpAddr {
    IpAddr::from([10, 0, 0, last])
}

fn entry(seed: u8, last: u8, port: u16) -> AddrEntry {
    AddrEntry {
        node_id: node_id(seed),
        addr: SocketAddr::new(ip4(last), port),
    }
}

/// Fresh per-test directory in the system temp dir (not `target/`).
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("strangecoin-s2p11-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Future output type of the DNS resolver seam: `bootstrap`'s
/// `Fn(String) -> Future` bound is instantiated with this in tests.
type BoxFuture = std::pin::Pin<Box<dyn Future<Output = Result<DnsAnswer, NetError>> + Send>>;

// ------------------------------------------------------------ seeds loading

#[test]
fn seed_config_loads_from_network_json() {
    let dir = temp_dir("seed-config");
    let path = dir.join("network.json");
    fs::write(
        &path,
        r#"{
            "peers": ["127.0.0.1:8081"],
            "seed_config": {
                "hard_coded_ips": ["203.0.113.10", "2001:db8::10"],
                "dns_seeds": ["seeds.example.org", "  ", "backup.example.org"],
                "dnssec_required": false,
                "default_port": 9999
            }
        }"#,
    )
    .unwrap();

    let cfg = load_seeds(&path).expect("load");
    assert_eq!(cfg.hard_coded_ips.len(), 2);
    assert_eq!(cfg.dns_seeds.len(), 3);
    assert!(!cfg.dnssec_required);
    assert_eq!(cfg.default_port, 9999);
    // IP without port + default_port → dialable socket, sorted.
    let sockets = cfg.seed_sockets();
    assert_eq!(sockets.len(), 2);
    assert!(sockets.contains(&SocketAddr::new(
        "203.0.113.10".parse::<IpAddr>().unwrap(),
        9999
    )));
    // Empty DNS entries dropped by the domain filter.
    assert_eq!(cfg.dns_seed_domains().len(), 2);

    // Missing `seed_config` section (Stage 1.5 back-compat) → secure default.
    fs::write(&path, r#"{ "peers": ["127.0.0.1:8081"] }"#).unwrap();
    let cfg = load_seeds(&path).expect("load back-compat");
    assert_eq!(cfg, SeedConfig::default());
    assert!(cfg.dnssec_required, "secure default: DNSSEC required");
    assert_eq!(cfg.default_port, DEFAULT_SEED_PORT);
    assert!(cfg.is_empty());

    // Missing file → default (fresh node has no seeds yet).
    let cfg = load_seeds(&dir.join("nope.json")).expect("missing file");
    assert_eq!(cfg, SeedConfig::default());

    // Malformed seed_config fails loudly — seeds sit on the V-16 path.
    fs::write(&path, r#"{ "seed_config": { "hard_coded_ips": "oops" } }"#).unwrap();
    let err = load_seeds(&path).expect_err("malformed seed_config must fail");
    assert!(matches!(err, NetError::Discovery(_)), "got {err:?}");
}

// -------------------------------------------------------------- DNSSEC gate

#[test]
fn dnssec_poisoned_answer_is_rejected() {
    let poisoned = DnsAnswer {
        addrs: vec!["203.0.113.66".parse().unwrap()],
        validated: false,
    };
    // dnssec_required = true (the default): the V-16 gate drops it.
    let err = apply_dnssec_policy(&poisoned, true).expect_err("poisoned must be rejected");
    assert!(matches!(err, NetError::Discovery(_)), "got {err:?}");
    assert!(err.to_string().contains("DNSSEC"));

    // Opting out of DNSSEC keeps the addresses (operator's choice).
    let addrs = apply_dnssec_policy(&poisoned, false).expect("policy off accepts");
    assert_eq!(addrs, poisoned.addrs);

    // Empty answers never contribute peers.
    let err = apply_dnssec_policy(&DnsAnswer::empty(), false).expect_err("empty rejected");
    assert!(matches!(err, NetError::Discovery(_)));
}

#[test]
fn dnssec_validated_answer_is_accepted() {
    let secure = DnsAnswer {
        addrs: vec![
            "203.0.113.10".parse().unwrap(),
            "2001:db8::10".parse().unwrap(),
        ],
        validated: true,
    };
    let addrs = apply_dnssec_policy(&secure, true).expect("Secure proof passes the gate");
    assert_eq!(addrs, secure.addrs);
}

// ----------------------------------------------------------- ADDR wire codec

#[test]
fn addr_message_roundtrip_and_rejections() {
    let entries = vec![
        entry(1, 10, 8081),
        AddrEntry {
            node_id: node_id(2),
            addr: "[2001:db8::5]:30303".parse().unwrap(),
        },
    ];
    let wire = encode_addr(&entries).expect("encode");
    assert_eq!(wire[0], MSG_ADDR);
    assert_eq!(
        wire.len(),
        5 + (1 + 4 + 2 + 32) + (1 + 16 + 2 + 32),
        "tag + count + v4 entry + v6 entry"
    );
    let back = parse_addr(&wire).expect("parse");
    assert_eq!(back, entries, "encode/parse round-trip");

    // Empty batch.
    let empty = encode_addr(&[]).expect("encode empty");
    assert_eq!(empty, vec![MSG_ADDR, 0, 0, 0, 0]);
    assert!(parse_addr(&empty).expect("parse empty").is_empty());

    // Oversized count rejected BEFORE any allocation (invariant #16).
    let mut oversized = vec![MSG_ADDR];
    oversized.extend_from_slice(&((MAX_ADDR_ENTRIES + 1) as u32).to_be_bytes());
    oversized.extend_from_slice(&[0u8; 64]);
    let err = parse_addr(&oversized).expect_err("count over cap");
    assert!(matches!(err, NetError::Protocol(_)), "got {err:?}");

    // Same cap on the encode side.
    let bulk = vec![entry(1, 1, 8081); MAX_ADDR_ENTRIES + 1];
    assert!(encode_addr(&bulk).is_err());

    // Count claims more entries than the payload carries.
    let mut short = vec![MSG_ADDR];
    short.extend_from_slice(&4u32.to_be_bytes());
    short.extend_from_slice(&[0u8; 40]);
    assert!(parse_addr(&short).is_err());

    // Wrong tag.
    let err = parse_addr(&[0x01, 0, 0, 0, 0]).expect_err("wrong tag");
    assert!(matches!(err, NetError::Protocol(_)));

    // Trailing garbage after the last entry.
    let mut trailing = wire.clone();
    trailing.push(0xAA);
    assert!(parse_addr(&trailing).is_err());

    // Unknown address family.
    let mut bad_family = vec![MSG_ADDR];
    bad_family.extend_from_slice(&1u32.to_be_bytes());
    bad_family.extend_from_slice(&[9, 0, 0, 0, 0, 0, 0]); // family 9
    bad_family.extend_from_slice(&[0u8; 34]);
    assert!(parse_addr(&bad_family).is_err());
}

// ------------------------------------------------------------- ADDR gossip

#[test]
fn addr_gossip_propagates_through_simulated_network() {
    let gossip: Vec<AddrEntry> = (1..=8u8).map(|i| entry(i, i, 8081)).collect();

    // Node A receives the batch from a peer (all unverified).
    let mut a = AddrBook::new();
    let added = handle_addr_message(gossip.clone(), &mut a);
    assert_eq!(added, 8);
    assert_eq!(a.len(), 8);

    // A → B → C over the wire: same set everywhere, still unverified.
    let wire = encode_addr(&a.top_k_sorted(MAX_ADDR_ENTRIES)).expect("encode");
    let mut b = AddrBook::new();
    assert_eq!(handle_addr_message(parse_addr(&wire).unwrap(), &mut b), 8);

    let wire = encode_addr(&b.top_k_sorted(MAX_ADDR_ENTRIES)).expect("encode");
    let mut c = AddrBook::new();
    assert_eq!(handle_addr_message(parse_addr(&wire).unwrap(), &mut c), 8);

    for book in [&b, &c] {
        for want in &gossip {
            let record = book.get(&want.node_id).expect("gossip address present");
            assert!(!record.verified, "gossip-learned addresses are unverified");
            assert_eq!(record.addr, want.addr);
        }
    }

    // Re-gossip is idempotent: no new entries, no demotion of a verified
    // record, no overwrite of the address we already track.
    let known = gossip[0];
    assert!(b.mark_verified(&known.node_id), "record exists to verify");
    assert!(
        !b.mark_verified(&node_id(99)),
        "unknown identity cannot be verified"
    );
    let changed = AddrEntry {
        addr: SocketAddr::new(ip4(99), 8081),
        ..known
    };
    assert_eq!(handle_addr_message(vec![changed], &mut b), 0);
    let record = b.get(&known.node_id).unwrap();
    assert!(record.verified, "verified flag survives re-gossip");
    assert_eq!(record.addr, known.addr, "known address is not overwritten");

    // Degenerate addresses (port 0 / unspecified) are skipped.
    let mut d = AddrBook::new();
    let junk = vec![
        AddrEntry {
            node_id: node_id(77),
            addr: SocketAddr::new(ip4(1), 0),
        },
        AddrEntry {
            node_id: node_id(78),
            addr: SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 8081),
        },
    ];
    assert_eq!(handle_addr_message(junk, &mut d), 0);
    assert!(d.is_empty());
}

#[test]
fn broadcast_known_addrs_sends_sorted_top_k_to_every_recipient() {
    struct Recorder {
        sent: RefCell<HashMap<NodeId, Vec<u8>>>,
        fail_on: NodeId,
    }
    impl AddrSender for Recorder {
        fn send_addr(&self, peer: &NodeId, payload: &[u8]) -> Result<(), NetError> {
            if *peer == self.fail_on {
                return Err(NetError::Transport("simulated write failure".into()));
            }
            self.sent.borrow_mut().insert(*peer, payload.to_vec());
            Ok(())
        }
    }

    let mut book = AddrBook::new();
    handle_addr_message((1..=8u8).map(|i| entry(i, i, 8081)).collect(), &mut book);

    // Connected peers include a duplicate — recipients are deduped/sorted.
    let recipients = vec![node_id(200), node_id(100), node_id(200)];
    let recorder = Recorder {
        sent: RefCell::new(HashMap::new()),
        fail_on: node_id(100),
    };

    let sent = broadcast_known_addrs(&book, &recipients, &recorder, 4).expect("broadcast");
    assert_eq!(sent, 1, "node_id(100) fails, node_id(200) gets the payload");

    let by_peer = recorder.sent.borrow();
    assert_eq!(by_peer.len(), 1);
    let payload = by_peer.get(&node_id(200)).expect("healthy peer received");
    let entries = parse_addr(payload).expect("payload is a valid ADDR message");
    assert_eq!(entries.len(), 4, "max_count caps the batch");
    let mut sorted = entries.clone();
    sorted.sort();
    assert_eq!(entries, sorted, "payload is deterministically ordered");

    // Trivial cases: nothing to send → zero sends, no error.
    let empty_book = AddrBook::new();
    assert_eq!(
        broadcast_known_addrs(&empty_book, &recipients, &recorder, 4).unwrap(),
        0
    );
    assert_eq!(broadcast_known_addrs(&book, &[], &recorder, 4).unwrap(), 0);
    assert_eq!(
        broadcast_known_addrs(&book, &recipients, &recorder, 0).unwrap(),
        0
    );
}

// ---------------------------------------------------------------- bootstrap

/// Closure that panics if the DNS seam is touched (empty `dns_seeds` must
/// never resolve). The body returns `Ok`, so the inferred future output
/// type matches the bootstrap bound.
fn dns_must_not_be_called() -> impl Fn(String) -> BoxFuture {
    |_dom: String| {
        Box::pin(async {
            panic!("DNS resolver must not be called when dns_seeds is empty");
            #[allow(unreachable_code)]
            Ok::<DnsAnswer, NetError>(DnsAnswer::empty())
        }) as BoxFuture
    }
}

#[tokio::test(start_paused = true)]
async fn bootstrap_with_only_seed_ips_finds_eight_peers_in_budget() {
    let cfg = SeedConfig {
        hard_coded_ips: (1..=8u8).map(ip4).collect(),
        ..SeedConfig::default()
    };
    let dns_called = std::sync::atomic::AtomicBool::new(false);
    let resolver = |dom: String| {
        dns_called.store(true, std::sync::atomic::Ordering::SeqCst);
        async move {
            let _ = dom;
            Ok(DnsAnswer::empty())
        }
    };

    let started = tokio::time::Instant::now();
    let outcome = bootstrap(&cfg, resolver, Vec::new())
        .await
        .expect("8 hard-coded seeds must succeed");
    let elapsed = started.elapsed();

    assert_eq!(outcome.peers.len(), MIN_BOOTSTRAP_PEERS);
    assert_eq!(outcome.sources_ok, 3, "all three sources completed");
    assert_eq!(outcome.sources_failed, 0);
    assert!(
        elapsed <= BOOTSTRAP_BUDGET,
        "budget respected: {elapsed:?} <= {BOOTSTRAP_BUDGET:?}"
    );
    assert!(
        !dns_called.load(std::sync::atomic::Ordering::SeqCst),
        "no dns_seeds configured → DNS source stays idle"
    );
    for peer in &outcome.peers {
        assert_eq!(peer.source, PeerSource::HardCodedIp);
        assert_eq!(peer.node_id, None, "bare seeds carry no identity yet");
        assert_eq!(peer.addr.port(), DEFAULT_SEED_PORT);
    }
    // Deterministic order (checklist #4): sorted by address.
    let addrs: Vec<SocketAddr> = outcome.peers.iter().map(|p| p.addr).collect();
    let mut sorted = addrs.clone();
    sorted.sort();
    assert_eq!(addrs, sorted);
}

#[tokio::test(start_paused = true)]
async fn bootstrap_dnssec_rejects_poisoned_source() {
    let poisoned_ip: IpAddr = "203.0.113.66".parse().unwrap();
    let cfg = SeedConfig {
        hard_coded_ips: (1..=8u8).map(ip4).collect(),
        dns_seeds: vec!["poisoned.example".into()],
        dnssec_required: true,
        ..SeedConfig::default()
    };
    let answer = DnsAnswer {
        addrs: vec![poisoned_ip],
        validated: false,
    };
    let resolver = move |_dom: String| {
        let answer = answer.clone();
        async move { Ok(answer) }
    };

    let outcome = bootstrap(&cfg, resolver, Vec::new())
        .await
        .expect("hard-coded seeds alone still reach the threshold");
    assert_eq!(outcome.sources_failed, 1, "poisoned DNS source failed");
    assert_eq!(outcome.sources_ok, 2);
    assert!(
        outcome.peers.iter().all(|p| p.addr.ip() != poisoned_ip),
        "poisoned addresses never enter the peer set (V-16)"
    );
    assert_eq!(outcome.peers.len(), MIN_BOOTSTRAP_PEERS);
}

#[tokio::test(start_paused = true)]
async fn multi_source_bootstrap_survives_one_source_down() {
    // DNS down (simulated outage), hard-coded gives 4, ADDR gossip gives 4:
    // the multi-source union reaches the threshold — the DoD scenario.
    let cfg = SeedConfig {
        hard_coded_ips: (1..=4u8).map(ip4).collect(),
        dns_seeds: vec!["seed1.example".into(), "seed2.example".into()],
        dnssec_required: true,
        ..SeedConfig::default()
    };
    let resolver = |_dom: String| async { Err(NetError::Discovery("simulated DNS outage".into())) };
    let gossip: Vec<AddrEntry> = (10..=14u8).take(4).map(|i| entry(i, i, 8081)).collect();

    let outcome = bootstrap(&cfg, resolver, gossip)
        .await
        .expect("union of hard-coded + gossip must reach 8");
    assert_eq!(outcome.sources_failed, 1, "exactly the DNS source is down");
    assert_eq!(outcome.sources_ok, 2);
    assert_eq!(outcome.peers.len(), MIN_BOOTSTRAP_PEERS);
    let from_dns = outcome
        .peers
        .iter()
        .filter(|p| p.source == PeerSource::Dns)
        .count();
    assert_eq!(from_dns, 0);
    assert!(outcome
        .peers
        .iter()
        .any(|p| p.source == PeerSource::AddrGossip && p.node_id.is_some()));
    assert!(outcome
        .peers
        .iter()
        .any(|p| p.source == PeerSource::HardCodedIp));
}

#[tokio::test(start_paused = true)]
async fn bootstrap_budget_caps_a_hanging_dns_source() {
    let cfg = SeedConfig {
        hard_coded_ips: (1..=8u8).map(ip4).collect(),
        dns_seeds: vec!["hang.example".into()],
        ..SeedConfig::default()
    };
    // The resolver never answers within the budget: the source must be
    // capped at BOOTSTRAP_BUDGET (simulated clock → instant wall time).
    let resolver = |_dom: String| async {
        tokio::time::sleep(Duration::from_secs(60)).await;
        Ok(DnsAnswer::empty())
    };

    let started = tokio::time::Instant::now();
    let outcome = bootstrap(&cfg, resolver, Vec::new())
        .await
        .expect("hard-coded seeds carry the run");
    let elapsed = started.elapsed();

    assert!(
        elapsed <= BOOTSTRAP_BUDGET,
        "hanging source capped: {elapsed:?} <= {BOOTSTRAP_BUDGET:?}"
    );
    assert_eq!(outcome.sources_failed, 1);
    assert_eq!(outcome.peers.len(), MIN_BOOTSTRAP_PEERS);
}

#[tokio::test(start_paused = true)]
async fn bootstrap_fails_when_no_source_reaches_threshold() {
    let cfg = SeedConfig::default(); // no IPs, no DNS
    let err = bootstrap(&cfg, dns_must_not_be_called(), Vec::new())
        .await
        .expect_err("0 peers must not bootstrap");
    assert!(matches!(err, NetError::Discovery(_)), "got {err:?}");
    assert!(err.to_string().contains('0'));

    // Under-threshold gossip alone (5 < 8) also fails.
    let cfg = SeedConfig::default();
    let gossip: Vec<AddrEntry> = (1..=5u8).map(|i| entry(i, i, 8081)).collect();
    let err = bootstrap(&cfg, dns_must_not_be_called(), gossip)
        .await
        .expect_err("5 peers must not bootstrap");
    assert!(matches!(err, NetError::Discovery(_)));
}

#[tokio::test(start_paused = true)]
async fn bootstrap_merges_gossip_and_seeds_deterministically() {
    let cfg = SeedConfig {
        hard_coded_ips: (1..=4u8).map(ip4).collect(),
        ..SeedConfig::default()
    };
    let gossip: Vec<AddrEntry> = (5..=8u8).map(|i| entry(i, i, 8081)).collect();
    let resolver = dns_must_not_be_called();

    let first = bootstrap(&cfg, &resolver, gossip.clone())
        .await
        .expect("union of 4 + 4");
    let second = bootstrap(&cfg, resolver, gossip)
        .await
        .expect("same inputs");
    assert_eq!(
        first.peers, second.peers,
        "identical inputs produce an identical, ordered peer list"
    );
    assert_eq!(first.peers.len(), MIN_BOOTSTRAP_PEERS);
    // Gossip entries carry identity, bare seeds do not.
    let with_id = first.peers.iter().filter(|p| p.node_id.is_some()).count();
    assert_eq!(with_id, 4);
}

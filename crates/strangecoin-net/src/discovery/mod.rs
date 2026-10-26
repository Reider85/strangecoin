//! S2-P11 — peer discovery: seed IPs, DNSSEC-validated DNS seeds, ADDR
//! gossip and the multi-source bootstrap that ties them together.
//!
//! ARCHITECT3 §3.6: «Peer discovery through seed nodes + addr gossip».
//! ROADMAP3 Stage 2 DoD #4: a new node finds ≥8 peers in ≤30 seconds.
//!
//! Three independent sources are tried in parallel within a single time
//! budget (the eclipse defense V-1/V-16: no single poisoned channel can
//! starve the node of peers — succeed with any combination that yields
//! [`MIN_BOOTSTRAP_PEERS`] addresses):
//!
//! 1. **hard-coded IPs** ([`seeds::SeedConfig::hard_coded_ips`]) — literal
//!    operator config, immune to DNS poisoning;
//! 2. **DNS seeds** through the DNSSEC gate ([`dnssec`]);
//! 3. **ADDR gossip** ([`addr_gossip`]) learned from peers already known.

pub mod addr_gossip;
pub mod dnssec;
pub mod seeds;

pub use addr_gossip::{AddrBook, AddrEntry, AddrRecord, AddrSender};
pub use seeds::SeedConfig;

use crate::error::NetError;
use crate::noise::NodeId;
use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

/// DoD #4: how many peers a successful bootstrap must produce.
pub const MIN_BOOTSTRAP_PEERS: usize = 8;

/// DoD #4 / prompt budget: wall-clock (or simulated-clock) bound for one
/// bootstrap attempt. All sources run concurrently, each capped at this
/// budget, so the total never exceeds it.
pub const BOOTSTRAP_BUDGET: Duration = Duration::from_secs(30);

/// Which channel a peer address came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PeerSource {
    /// ADDR gossip — the address came with a `node_id` from a peer.
    AddrGossip,
    /// Operator's literal `hard_coded_ips` config.
    HardCodedIp,
    /// DNSSEC-validated resolution of a `dns_seeds` entry.
    Dns,
}

/// One candidate produced by the bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    /// `Some` for gossip-learned addresses (the `node_id` was advertised);
    /// `None` for raw seeds — identity is learned at handshake time (S2-P12
    /// dials with `expected_node_id = None` and verifies on HELLO).
    pub node_id: Option<NodeId>,
    pub addr: SocketAddr,
    pub source: PeerSource,
}

/// Result of a successful [`bootstrap`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapOutcome {
    /// Deduped candidates, deterministically ordered by `(addr, node_id)`.
    pub peers: Vec<DiscoveredPeer>,
    /// Sources that completed without error (an empty source still counts
    /// as completed — it simply had nothing to contribute).
    pub sources_ok: usize,
    /// Sources that failed or exceeded the budget.
    pub sources_failed: usize,
}

/// Multi-source bootstrap (prompt task 5): run all three sources in
/// parallel under [`BOOTSTRAP_BUDGET`], merge and dedupe the results, and
/// succeed once the union holds at least [`MIN_BOOTSTRAP_PEERS`] addresses.
///
/// * `cfg` — the `seed_config` section ([`seeds::load_seeds`]).
/// * `dns_resolve` — the DNSSEC resolver seam: production passes
///   [`dnssec::DnssecResolver::resolve`], tests a mock closure (this is
///   how the poisoned-DNS case is exercised without a network).
/// * `gossip_addrs` — addresses already learned from ADDR gossip (empty on
///   the very first run; later runs feed their [`AddrBook`]).
///
/// Success criterion: the **union** of completed sources reaches
/// `min_peers`. The prompt phrases it as «≥1 source returns ≥8 peers» —
/// the union subsumes that case (any single source ≥8 implies union ≥8)
/// and additionally merges partial sources, which is the point of
/// multi-source bootstrap; recorded as a strengthening in the S2-P11
/// commit message.
pub async fn bootstrap<F, Fut>(
    cfg: &SeedConfig,
    dns_resolve: F,
    gossip_addrs: Vec<AddrEntry>,
) -> Result<BootstrapOutcome, NetError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<dnssec::DnsAnswer, NetError>>,
{
    // Source 1: literal IPs — pure config, cannot fail.
    let hard_coded = cfg.seed_sockets();
    // Source 3: gossip — pure memory, cannot fail (degenerate entries
    // filtered defensively; `handle_addr_message` already does this).
    let gossip: Vec<DiscoveredPeer> = gossip_addrs
        .into_iter()
        .filter(|entry| addr_gossip::is_dialable(entry.addr))
        .map(|entry| DiscoveredPeer {
            node_id: Some(entry.node_id),
            addr: entry.addr,
            source: PeerSource::AddrGossip,
        })
        .collect();

    let hard_coded_fut = async {
        hard_coded
            .iter()
            .map(|addr| DiscoveredPeer {
                node_id: None,
                addr: *addr,
                source: PeerSource::HardCodedIp,
            })
            .collect::<Vec<_>>()
    };
    let dns_fut = dns_source(cfg, dns_resolve, BOOTSTRAP_BUDGET);
    let gossip_fut = async { gossip };

    let (hard_coded_peers, dns_peers, gossip_peers) =
        tokio::join!(hard_coded_fut, dns_fut, gossip_fut);

    let mut sources_ok = 2usize; // hard-coded + gossip are infallible
    let mut sources_failed = 0usize;
    let mut all = hard_coded_peers;
    all.extend(gossip_peers);
    match dns_peers {
        Ok(peers) => {
            sources_ok += 1;
            all.extend(peers);
        }
        Err(_) => sources_failed += 1,
    }

    dedupe_sort(&mut all);
    if all.len() < MIN_BOOTSTRAP_PEERS {
        return Err(NetError::Discovery(format!(
            "bootstrap found {} peers from {sources_ok} source(s) ({sources_failed} failed), need {MIN_BOOTSTRAP_PEERS}",
            all.len()
        )));
    }
    Ok(BootstrapOutcome {
        peers: all,
        sources_ok,
        sources_failed,
    })
}

/// The DNS leg: every `dns_seeds` domain resolved (DNSSEC policy applied
/// per config), the whole source capped at `budget` so a hanging resolver
/// cannot stall the bootstrap past the 30s bound.
async fn dns_source<F, Fut>(
    cfg: &SeedConfig,
    dns_resolve: F,
    budget: Duration,
) -> Result<Vec<DiscoveredPeer>, NetError>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<dnssec::DnsAnswer, NetError>>,
{
    let domains = seeds::dedup_domains(cfg.dns_seed_domains());
    if domains.is_empty() {
        return Ok(Vec::new());
    }
    let run = async {
        let mut out = Vec::new();
        for domain in domains {
            let answer = dns_resolve(domain).await?;
            let addrs = dnssec::apply_dnssec_policy(&answer, cfg.dnssec_required)?;
            for ip in addrs {
                out.push(DiscoveredPeer {
                    node_id: None,
                    addr: SocketAddr::new(ip, cfg.default_port),
                    source: PeerSource::Dns,
                });
            }
        }
        Ok(out)
    };
    match tokio::time::timeout(budget, run).await {
        Ok(result) => result,
        Err(_) => Err(NetError::Discovery(format!(
            "DNS source exceeded budget {}s",
            budget.as_secs()
        ))),
    }
}

/// Deterministic merge: order by `(addr, node_id presence, source)` and
/// drop duplicate addresses, keeping the strongest entry — a gossip-learned
/// address (carries `node_id`) beats a bare seed for the same socket
/// (Stage 2 determinism checklist #4: same results → same order on every
/// node; no clock, no RNG).
fn dedupe_sort(peers: &mut Vec<DiscoveredPeer>) {
    peers.sort_by(|a, b| {
        a.addr
            .cmp(&b.addr)
            .then_with(|| b.node_id.is_some().cmp(&a.node_id.is_some()))
            .then_with(|| a.source.cmp(&b.source))
    });
    peers.dedup_by(|a, b| a.addr == b.addr);
}

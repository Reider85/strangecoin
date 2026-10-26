//! S2-P11 — seed configuration loader (`seed_config` section of
//! `network.json`).
//!
//! Threat V-16 (eclipse via DNS poisoning, ARCHITECT3 §6): the node must not
//! trust DNS alone — [`SeedConfig::hard_coded_ips`] are literal addresses
//! written by the operator, [`SeedConfig::dns_seeds`] go through DNSSEC
//! validation ([`super::dnssec`]) whenever [`SeedConfig::dnssec_required`]
//! is set (the default). Both are tried as independent bootstrap sources
//! together with ADDR gossip ([`super::addr_gossip`]) in
//! [`super::bootstrap`].

use crate::error::NetError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

/// Fallback P2P port for seed entries that carry an IP without one — the
/// regtest/devnet default listen port of the node (`config.rs`).
pub const DEFAULT_SEED_PORT: u16 = 8081;

fn default_dnssec_required() -> bool {
    true
}

fn default_port() -> u16 {
    DEFAULT_SEED_PORT
}

/// Bootstrap seed sources persisted in `network.json`.
///
/// Wire shape (JSON section `seed_config`):
/// ```json
/// {
///   "peers": ["127.0.0.1:8081"],
///   "seed_config": {
///     "hard_coded_ips": ["203.0.113.10", "2001:db8::10"],
///     "dns_seeds": ["seeds.strangecoin.example"],
///     "dnssec_required": true,
///     "default_port": 8081
///   }
/// }
/// ```
///
/// Missing keys fall back to [`Default`] (empty lists, DNSSEC **required**,
/// [`DEFAULT_SEED_PORT`]) — an operator who writes nothing gets the secure
/// defaults, not a DNS-trusting bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedConfig {
    /// Literal seed IPs — trusted without DNS, the V-16 fallback when DNS
    /// is poisoned or unavailable. No ports (see [`DEFAULT_SEED_PORT`]).
    #[serde(default)]
    pub hard_coded_ips: Vec<IpAddr>,

    /// DNS names of the seed nodes, resolved through the DNSSEC-validating
    /// path in [`super::dnssec`].
    #[serde(default)]
    pub dns_seeds: Vec<String>,

    /// When `true` (default) answers without a `Secure` DNSSEC proof are
    /// rejected outright; `false` downgrades to best-effort resolution.
    #[serde(default = "default_dnssec_required")]
    pub dnssec_required: bool,

    /// Port combined with every [`Self::hard_coded_ips`] entry (and DNS
    /// answers) to form a dialable [`SocketAddr`].
    #[serde(default = "default_port")]
    pub default_port: u16,
}

impl Default for SeedConfig {
    fn default() -> Self {
        SeedConfig {
            hard_coded_ips: Vec::new(),
            dns_seeds: Vec::new(),
            dnssec_required: default_dnssec_required(),
            default_port: default_port(),
        }
    }
}

impl SeedConfig {
    /// Hard-coded IPs as dialable addresses (`ip:default_port`), deduped and
    /// sorted for the Stage 2 determinism checklist #4 (same config → same
    /// order on every node).
    pub fn seed_sockets(&self) -> Vec<SocketAddr> {
        let mut out: Vec<SocketAddr> = self
            .hard_coded_ips
            .iter()
            .map(|ip| SocketAddr::new(*ip, self.default_port))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// DNS seed names with empty/whitespace entries dropped, order preserved
    /// (resolution order is config order — deterministic either way).
    pub fn dns_seed_domains(&self) -> Vec<String> {
        self.dns_seeds
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// A config that contributes no sources at all.
    pub fn is_empty(&self) -> bool {
        self.hard_coded_ips.is_empty() && self.dns_seed_domains().is_empty()
    }
}

/// Load the `seed_config` section from a `network.json`-style file.
///
/// * Missing file → [`SeedConfig::default()`] (a fresh node has no seeds yet).
/// * Missing `seed_config` key → [`SeedConfig::default()`] (back-compat with
///   Stage 1.5 files that only carry `peers`).
/// * Malformed JSON / malformed `seed_config` → [`NetError::Discovery`]:
///   seeds sit on the V-16 trust path, so a broken config fails loudly
///   instead of silently falling back to a DNS-only bootstrap.
pub fn load_seeds(config_path: &Path) -> Result<SeedConfig, NetError> {
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SeedConfig::default());
        }
        Err(err) => {
            return Err(NetError::Discovery(format!(
                "reading {}: {err}",
                config_path.display()
            )));
        }
    };
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|err| NetError::Discovery(format!("parsing {}: {err}", config_path.display())))?;
    match value.get("seed_config") {
        None => Ok(SeedConfig::default()),
        Some(section) => serde_json::from_value(section.clone()).map_err(|err| {
            NetError::Discovery(format!(
                "invalid seed_config in {}: {err}",
                config_path.display()
            ))
        }),
    }
}

/// Domains from a parsed `seed_config`, deduped (BTreeSet keeps first-seen
/// config order out of the picture — resolution order becomes lexicographic,
/// hence deterministic across nodes sharing the same seed list).
pub fn dedup_domains(domains: Vec<String>) -> Vec<String> {
    domains
        .into_iter()
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
}

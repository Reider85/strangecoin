use crate::error::StrangecoinError;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NodeMode {
    #[default]
    Full,
    Light,
    Archival,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub network_id: u32,
    pub node_mode: NodeMode,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub log_level: String,
    pub data_dir: PathBuf,
    #[serde(default)]
    pub allow_grant_blocks: bool,
    /// SCIP-0002 / BUG-S1-002: accept blocks with a zero `state_root`
    /// (no commitment). `None` resolves to `true` on regtest (network_id 3)
    /// and `false` on mainnet/testnet; `Some(true)` on mainnet/testnet is a
    /// config error.
    #[serde(default)]
    pub allow_zero_state_root: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: SocketAddr,
    pub seeds: Vec<SocketAddr>,
    pub max_peers: usize,
    /// S2-P12 (`peer_ban_threshold`): peer score below which a peer is
    /// temp-banned for 1 h (ADR-0022; mirror of
    /// `strangecoin_net::peer::DEFAULT_BAN_THRESHOLD`). Kept as the
    /// historical S2-P12 key for the temp-ban threshold — the rest of the
    /// S2-P17 scoring surface lives under `peer_scoring_*`.
    #[serde(default = "default_peer_ban_threshold")]
    pub peer_ban_threshold: f32,
    /// S2-P12 (`max_connections`): total concurrent TCP connections managed
    /// by the peer manager (distinct from `max_peers`, the known-peer target
    /// size for sync).
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    /// S2-P12 (`max_per_ip_range`): Sybil cap per IPv4 /24 or IPv6 /64 range
    /// in the connection pool (threat V-17; mirror of
    /// `strangecoin_net::peer::DEFAULT_MAX_PER_IP_RANGE`).
    #[serde(default = "default_max_per_ip_range")]
    pub max_per_ip_range: usize,
    /// S2-P16 (`rate_limit_ban_threshold`, ADR-0023): violations inside
    /// `rate_limit_ban_window_sec` that trigger the fast-path ban.
    #[serde(default = "default_rate_limit_ban_threshold")]
    pub rate_limit_ban_threshold: u32,
    /// S2-P16 (`rate_limit_ban_window_sec`): rolling violation window.
    #[serde(default = "default_rate_limit_ban_window_sec")]
    pub rate_limit_ban_window_sec: u64,
    /// S2-P16 (`rate_limit_ban_duration_sec`): fast-path ban duration
    /// (ADR-0023 tuning surface — the prompt key list omits it, ADR
    /// requires ban duration to be tunable).
    #[serde(default = "default_rate_limit_ban_duration_sec")]
    pub rate_limit_ban_duration_sec: u64,
    /// S2-P16 (`rate_limit_main_capacity`, ADR-0023): main bucket burst.
    #[serde(default = "default_rate_limit_main_capacity")]
    pub rate_limit_main_capacity: u32,
    /// S2-P16 (`rate_limit_main_refill`): main bucket tokens per second.
    #[serde(default = "default_rate_limit_main_refill")]
    pub rate_limit_main_refill: u32,
    /// S2-P16 (`rate_limit_per_type`): per-message-type sub-bucket specs of
    /// the form `"capacity/window_secs"` (e.g. `"100/10s"` = 100 burst,
    /// 10 tokens/s). `get_headers`/`get_blocks` are the ADR-0023 legacy sync
    /// sub-buckets (Stage 1 carry-over; beyond the prompt's 3-key list).
    #[serde(default)]
    pub rate_limit_per_type: RateLimitPerTypeConfig,
    /// S2-P17 (`peer_scoring_w_latency`, ADR-0022): latency grading weight.
    #[serde(default = "default_ps_w_latency")]
    pub peer_scoring_w_latency: f32,
    /// S2-P17 (`peer_scoring_w_invalid`): invalid blocks/txs penalty weight
    /// (threat V-15).
    #[serde(default = "default_ps_w_invalid")]
    pub peer_scoring_w_invalid: f32,
    /// S2-P17 (`peer_scoring_w_violation`): protocol-violations penalty
    /// weight (rate-limit feed, threat V-17).
    #[serde(default = "default_ps_w_violation")]
    pub peer_scoring_w_violation: f32,
    /// S2-P17 (`peer_scoring_w_uptime`): uptime grading weight.
    #[serde(default = "default_ps_w_uptime")]
    pub peer_scoring_w_uptime: f32,
    /// S2-P17 (`peer_scoring_warn_threshold`): score below which a peer is
    /// warned (stays connected).
    #[serde(default = "default_ps_warn_threshold")]
    pub peer_scoring_warn_threshold: f32,
    /// S2-P17 (`peer_scoring_persistent_threshold`): score below which a
    /// peer is persistently banned 30 d (`network.json`).
    #[serde(default = "default_ps_persistent_threshold")]
    pub peer_scoring_persistent_threshold: f32,
    /// S2-P17 (`peer_scoring_temp_ban_duration_sec`): temp-ban duration.
    #[serde(default = "default_ps_temp_ban_duration_sec")]
    pub peer_scoring_temp_ban_duration_sec: u64,
    /// S2-P17 (`peer_scoring_persistent_ban_duration_sec`): persistent-ban
    /// duration (ADR-0022: 30 days).
    #[serde(default = "default_ps_persistent_ban_duration_sec")]
    pub peer_scoring_persistent_ban_duration_sec: u64,
    /// S2-P17 (`peer_scoring_invalid_full_weight`): invalid events at which
    /// the penalty reaches `peer_scoring_w_invalid` (ADR-0022
    /// `INVALID_FULL_WEIGHT`).
    #[serde(default = "default_ps_invalid_full_weight")]
    pub peer_scoring_invalid_full_weight: u32,
    /// S2-P17 (`peer_scoring_violation_full_weight`): violations at which
    /// the penalty reaches `peer_scoring_w_violation` (ADR-0022
    /// `VIOLATION_FULL_WEIGHT`).
    #[serde(default = "default_ps_violation_full_weight")]
    pub peer_scoring_violation_full_weight: u32,
}

fn default_peer_ban_threshold() -> f32 {
    0.2
}

fn default_max_connections() -> usize {
    50
}

fn default_max_per_ip_range() -> usize {
    8
}

fn default_rate_limit_ban_threshold() -> u32 {
    5
}

fn default_rate_limit_ban_window_sec() -> u64 {
    60
}

fn default_rate_limit_ban_duration_sec() -> u64 {
    300
}

fn default_rate_limit_main_capacity() -> u32 {
    1000
}

fn default_rate_limit_main_refill() -> u32 {
    100
}

/// Per-type sub-bucket specs (`config.toml: rate_limit_per_type`), values in
/// the `"capacity/window_secs"` format parsed by
/// `strangecoin_net::rate_limiter::parse_rate_spec`. Every field falls back
/// to the ADR-0023 initial table when the key (or the whole table) is absent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimitPerTypeConfig {
    /// `INV` — ADR-0023: 100 burst / 10 per s.
    #[serde(default = "default_rl_inv")]
    pub inv: String,
    /// `BLOCK` — ADR-0023: 30 burst / 3 per s.
    #[serde(default = "default_rl_block")]
    pub block: String,
    /// `TX` — ADR-0023: 1000 burst / 100 per s.
    #[serde(default = "default_rl_tx")]
    pub tx: String,
    /// Legacy `GET_HEADERS` sync sub-bucket (ADR-0023 Stage 1 carry-over).
    #[serde(default = "default_rl_get_headers")]
    pub get_headers: String,
    /// Legacy `GET_BLOCKS` sync sub-bucket (ADR-0023 Stage 1 carry-over).
    #[serde(default = "default_rl_get_blocks")]
    pub get_blocks: String,
}

fn default_rl_inv() -> String {
    "100/10s".into()
}

fn default_rl_block() -> String {
    "30/10s".into()
}

fn default_rl_tx() -> String {
    "1000/10s".into()
}

fn default_rl_get_headers() -> String {
    "100/10s".into()
}

fn default_rl_get_blocks() -> String {
    "100/10s".into()
}

// S2-P17 peer-scoring defaults — mirrors of strangecoin_net::peer::scoring
// (ADR-0022 initial tables).

fn default_ps_w_latency() -> f32 {
    0.2
}

fn default_ps_w_invalid() -> f32 {
    0.5
}

fn default_ps_w_violation() -> f32 {
    0.2
}

fn default_ps_w_uptime() -> f32 {
    0.1
}

fn default_ps_warn_threshold() -> f32 {
    0.4
}

fn default_ps_persistent_threshold() -> f32 {
    0.05
}

fn default_ps_temp_ban_duration_sec() -> u64 {
    3600
}

fn default_ps_persistent_ban_duration_sec() -> u64 {
    60 * 60 * 24 * 30
}

fn default_ps_invalid_full_weight() -> u32 {
    6
}

fn default_ps_violation_full_weight() -> u32 {
    4
}

impl Default for RateLimitPerTypeConfig {
    fn default() -> Self {
        RateLimitPerTypeConfig {
            inv: default_rl_inv(),
            block: default_rl_block(),
            tx: default_rl_tx(),
            get_headers: default_rl_get_headers(),
            get_blocks: default_rl_get_blocks(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            listen_addr: "127.0.0.1:8081".parse().unwrap(),
            seeds: vec![],
            max_peers: 50,
            peer_ban_threshold: default_peer_ban_threshold(),
            max_connections: default_max_connections(),
            max_per_ip_range: default_max_per_ip_range(),
            rate_limit_ban_threshold: default_rate_limit_ban_threshold(),
            rate_limit_ban_window_sec: default_rate_limit_ban_window_sec(),
            rate_limit_ban_duration_sec: default_rate_limit_ban_duration_sec(),
            rate_limit_main_capacity: default_rate_limit_main_capacity(),
            rate_limit_main_refill: default_rate_limit_main_refill(),
            rate_limit_per_type: RateLimitPerTypeConfig::default(),
            peer_scoring_w_latency: default_ps_w_latency(),
            peer_scoring_w_invalid: default_ps_w_invalid(),
            peer_scoring_w_violation: default_ps_w_violation(),
            peer_scoring_w_uptime: default_ps_w_uptime(),
            peer_scoring_warn_threshold: default_ps_warn_threshold(),
            peer_scoring_persistent_threshold: default_ps_persistent_threshold(),
            peer_scoring_temp_ban_duration_sec: default_ps_temp_ban_duration_sec(),
            peer_scoring_persistent_ban_duration_sec: default_ps_persistent_ban_duration_sec(),
            peer_scoring_invalid_full_weight: default_ps_invalid_full_weight(),
            peer_scoring_violation_full_weight: default_ps_violation_full_weight(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub path: PathBuf,
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, StrangecoinError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| StrangecoinError::ConfigError(format!("Failed to read config: {}", e)))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| StrangecoinError::ConfigError(format!("Failed to parse config: {}", e)))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), StrangecoinError> {
        if self.network_id == 0 {
            return Err(StrangecoinError::ConfigError(
                "network_id must be > 0".into(),
            ));
        }
        if self.network_id != 1 && self.network_id != 2 && self.network_id != 3 {
            return Err(StrangecoinError::ConfigError(format!(
                "network_id must be 1 (mainnet), 2 (testnet), or 3 (regtest), got {}",
                self.network_id
            )));
        }
        if self.allow_zero_state_root == Some(true) && self.network_id != 3 {
            return Err(StrangecoinError::ConfigError(
                "allow_zero_state_root is only permitted on regtest (network_id = 3)".into(),
            ));
        }
        if self.network.max_peers == 0 {
            return Err(StrangecoinError::ConfigError(
                "max_peers must be > 0".into(),
            ));
        }
        if !(self.network.peer_ban_threshold > 0.0 && self.network.peer_ban_threshold < 1.0) {
            return Err(StrangecoinError::ConfigError(format!(
                "peer_ban_threshold must be in (0, 1), got {}",
                self.network.peer_ban_threshold
            )));
        }
        // S2-P17 (ADR-0022): band ordering 0 < persistent < temp < warn < 1
        // and non-degenerate grading weights.
        let ps = &self.network;
        if !(ps.peer_scoring_persistent_threshold > 0.0
            && ps.peer_scoring_persistent_threshold < ps.peer_ban_threshold
            && ps.peer_ban_threshold < ps.peer_scoring_warn_threshold
            && ps.peer_scoring_warn_threshold < 1.0)
        {
            return Err(StrangecoinError::ConfigError(format!(
                "peer scoring thresholds must satisfy 0 < persistent ({}) < \
                 peer_ban_threshold ({}) < warn ({}) < 1",
                ps.peer_scoring_persistent_threshold,
                ps.peer_ban_threshold,
                ps.peer_scoring_warn_threshold
            )));
        }
        for (name, weight) in [
            ("peer_scoring_w_latency", ps.peer_scoring_w_latency),
            ("peer_scoring_w_invalid", ps.peer_scoring_w_invalid),
            ("peer_scoring_w_violation", ps.peer_scoring_w_violation),
            ("peer_scoring_w_uptime", ps.peer_scoring_w_uptime),
        ] {
            if !(weight.is_finite() && weight >= 0.0) {
                return Err(StrangecoinError::ConfigError(format!(
                    "{name} must be a finite value >= 0, got {weight}"
                )));
            }
        }
        if ps.peer_scoring_w_latency + ps.peer_scoring_w_uptime <= 0.0 {
            return Err(StrangecoinError::ConfigError(
                "peer_scoring_w_latency + peer_scoring_w_uptime must be > 0 \
                 (grading part of the score)"
                    .into(),
            ));
        }
        if ps.peer_scoring_temp_ban_duration_sec == 0 {
            return Err(StrangecoinError::ConfigError(
                "peer_scoring_temp_ban_duration_sec must be > 0".into(),
            ));
        }
        if ps.peer_scoring_persistent_ban_duration_sec == 0 {
            return Err(StrangecoinError::ConfigError(
                "peer_scoring_persistent_ban_duration_sec must be > 0".into(),
            ));
        }
        if ps.peer_scoring_invalid_full_weight == 0 {
            return Err(StrangecoinError::ConfigError(
                "peer_scoring_invalid_full_weight must be > 0".into(),
            ));
        }
        if ps.peer_scoring_violation_full_weight == 0 {
            return Err(StrangecoinError::ConfigError(
                "peer_scoring_violation_full_weight must be > 0".into(),
            ));
        }
        if self.network.max_connections == 0 {
            return Err(StrangecoinError::ConfigError(
                "max_connections must be > 0".into(),
            ));
        }
        if self.network.max_per_ip_range == 0 {
            return Err(StrangecoinError::ConfigError(
                "max_per_ip_range must be > 0".into(),
            ));
        }
        if self.network.rate_limit_ban_threshold == 0 {
            return Err(StrangecoinError::ConfigError(
                "rate_limit_ban_threshold must be > 0".into(),
            ));
        }
        if self.network.rate_limit_ban_window_sec == 0 {
            return Err(StrangecoinError::ConfigError(
                "rate_limit_ban_window_sec must be > 0".into(),
            ));
        }
        if self.network.rate_limit_ban_duration_sec == 0 {
            return Err(StrangecoinError::ConfigError(
                "rate_limit_ban_duration_sec must be > 0".into(),
            ));
        }
        if self.network.rate_limit_main_capacity == 0 {
            return Err(StrangecoinError::ConfigError(
                "rate_limit_main_capacity must be > 0".into(),
            ));
        }
        if self.network.rate_limit_main_refill == 0 {
            return Err(StrangecoinError::ConfigError(
                "rate_limit_main_refill must be > 0".into(),
            ));
        }
        // Spec parsing lives in strangecoin-net (feature `net`, mandatory on
        // Stage 2 — same gate as `peer_manager_config`).
        #[cfg(feature = "net")]
        {
            let pt = &self.network.rate_limit_per_type;
            for (name, spec) in [
                ("inv", &pt.inv),
                ("block", &pt.block),
                ("tx", &pt.tx),
                ("get_headers", &pt.get_headers),
                ("get_blocks", &pt.get_blocks),
            ] {
                if let Err(err) = strangecoin_net::rate_limiter::parse_rate_spec(spec) {
                    return Err(StrangecoinError::ConfigError(format!(
                        "rate_limit_per_type.{name}: {err}"
                    )));
                }
            }
        }
        if self.data_dir.as_os_str().is_empty() {
            return Err(StrangecoinError::ConfigError(
                "data_dir must not be empty".into(),
            ));
        }
        if self.storage.path.as_os_str().is_empty() {
            return Err(StrangecoinError::ConfigError(
                "storage.path must not be empty".into(),
            ));
        }
        Ok(())
    }

    /// Network-aware default (SCIP-0002): `None` → `true` on regtest,
    /// `false` on mainnet/testnet. Explicit values always win.
    pub fn zero_state_root_allowed(&self) -> bool {
        self.allow_zero_state_root.unwrap_or(self.network_id == 3)
    }
}

#[cfg(feature = "net")]
impl Config {
    /// Map the operator's `config.toml` onto the network crate's peer-manager
    /// tunables (S2-P12 connection caps + S2-P17 `peer_scoring_*`).
    ///
    /// The temp-ban threshold keeps the historical S2-P12 key
    /// `peer_ban_threshold`; every other ADR-0022 weight/threshold/duration
    /// is a `peer_scoring_*` key. The identity keypair (Ed25519 + derived
    /// Noise static) and the `network.json` path are supplied at
    /// construction time by the orchestrator wiring (S2-P13/S2-P19).
    pub fn peer_manager_config(&self) -> strangecoin_net::PeerManagerConfig {
        let net = &self.network;
        strangecoin_net::PeerManagerConfig {
            scoring: strangecoin_net::ScoringConfig {
                w_latency: net.peer_scoring_w_latency,
                w_invalid: net.peer_scoring_w_invalid,
                w_violation: net.peer_scoring_w_violation,
                w_uptime: net.peer_scoring_w_uptime,
                warn_threshold: net.peer_scoring_warn_threshold,
                temp_ban_threshold: net.peer_ban_threshold,
                persistent_ban_threshold: net.peer_scoring_persistent_threshold,
                temp_ban_duration: Duration::from_secs(net.peer_scoring_temp_ban_duration_sec),
                persistent_ban_duration: Duration::from_secs(
                    net.peer_scoring_persistent_ban_duration_sec,
                ),
                invalid_full_weight: net.peer_scoring_invalid_full_weight,
                violation_full_weight: net.peer_scoring_violation_full_weight,
            },
            max_connections: net.max_connections,
            max_per_ip_range: net.max_per_ip_range,
            network_id: self.network_id,
            ..strangecoin_net::PeerManagerConfig::default()
        }
    }

    /// Map the operator's `config.toml` `rate_limit_*` keys onto the network
    /// crate's token-bucket tunables (S2-P16 / ADR-0023).
    ///
    /// Numeric keys map 1:1; the `rate_limit_per_type` specs
    /// (`"capacity/window_secs"`) are re-parsed through
    /// `parse_rate_spec` — `validate()` already rejected malformed specs at
    /// load, so a parse failure here falls back to the ADR-0023 default for
    /// that bucket (hand-built configs that skipped `validate`).
    pub fn rate_limit_config(&self) -> strangecoin_net::RateLimitConfig {
        use strangecoin_net::rate_limiter::{parse_rate_spec, BucketSpec, RateLimitConfig};

        let defaults = RateLimitConfig::default();
        let spec = |raw: &str, fallback: BucketSpec| parse_rate_spec(raw).unwrap_or(fallback);
        let pt = &self.network.rate_limit_per_type;

        RateLimitConfig {
            main: BucketSpec::new(
                self.network.rate_limit_main_capacity,
                self.network.rate_limit_main_refill,
            ),
            get_headers: spec(&pt.get_headers, defaults.get_headers),
            get_blocks: spec(&pt.get_blocks, defaults.get_blocks),
            inv: spec(&pt.inv, defaults.inv),
            block: spec(&pt.block, defaults.block),
            tx: spec(&pt.tx, defaults.tx),
            ban_threshold: self.network.rate_limit_ban_threshold,
            ban_window: Duration::from_secs(self.network.rate_limit_ban_window_sec),
            ban_duration: Duration::from_secs(self.network.rate_limit_ban_duration_sec),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            network_id: 3,
            node_mode: NodeMode::default(),
            network: NetworkConfig::default(),
            storage: StorageConfig {
                path: "./data/leveldb".into(),
            },
            log_level: "info".into(),
            data_dir: "./data".into(),
            allow_grant_blocks: false,
            allow_zero_state_root: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_state_root_default_is_network_aware() {
        let regtest = Config::default();
        assert_eq!(regtest.network_id, 3);
        assert!(regtest.zero_state_root_allowed(), "regtest default is true");

        let mainnet = Config {
            network_id: 1,
            ..Config::default()
        };
        assert!(
            !mainnet.zero_state_root_allowed(),
            "mainnet default is false"
        );

        let testnet = Config {
            network_id: 2,
            ..Config::default()
        };
        assert!(
            !testnet.zero_state_root_allowed(),
            "testnet default is false"
        );
    }

    #[test]
    fn zero_state_root_explicit_value_overrides_default() {
        let strict_regtest = Config {
            allow_zero_state_root: Some(false),
            ..Config::default()
        };
        assert!(!strict_regtest.zero_state_root_allowed());

        let explicit_testnet = Config {
            network_id: 2,
            allow_zero_state_root: Some(true),
            ..Config::default()
        };
        assert!(explicit_testnet.zero_state_root_allowed());
    }

    #[test]
    fn zero_state_root_rejected_on_mainnet_and_testnet() {
        for network_id in [1u32, 2u32] {
            let cfg = Config {
                network_id,
                allow_zero_state_root: Some(true),
                ..Config::default()
            };
            let err = cfg.validate().expect_err("must reject on public networks");
            assert!(err.to_string().contains("allow_zero_state_root"), "{err:?}");
        }
    }

    #[test]
    fn zero_state_root_accepted_on_regtest() {
        let cfg = Config {
            network_id: 3,
            allow_zero_state_root: Some(true),
            ..Config::default()
        };
        cfg.validate().expect("regtest may enable the opt-in");
    }

    #[test]
    fn peer_tunables_default_when_missing_from_config_file() {
        // Stage 1.5-era files (no peer keys) must keep parsing — S2-P12
        // serde defaults.
        let toml = r#"
            network_id = 3
            node_mode = "full"
            log_level = "info"
            data_dir = "./data"

            [network]
            listen_addr = "127.0.0.1:8081"
            seeds = []
            max_peers = 50

            [storage]
            path = "./data/leveldb"
        "#;
        let cfg: Config = toml::from_str(toml).expect("legacy config must parse");
        assert_eq!(cfg.network.peer_ban_threshold, 0.2);
        assert_eq!(cfg.network.max_connections, 50);
        assert_eq!(cfg.network.max_per_ip_range, 8);
        cfg.validate().expect("defaults are valid");
    }

    #[test]
    fn peer_tunables_reject_invalid_values() {
        let base = Config::default();
        let bad_threshold = Config {
            network: NetworkConfig {
                peer_ban_threshold: 1.5,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(bad_threshold.validate().is_err());

        let bad_connections = Config {
            network: NetworkConfig {
                max_connections: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(bad_connections.validate().is_err());

        let bad_per_ip = Config {
            network: NetworkConfig {
                max_per_ip_range: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(bad_per_ip.validate().is_err());
    }

    #[cfg(feature = "net")]
    #[test]
    fn peer_manager_config_maps_file_tunables() {
        let cfg = Config {
            network: NetworkConfig {
                peer_ban_threshold: 0.35,
                max_connections: 12,
                max_per_ip_range: 3,
                ..Config::default().network
            },
            ..Config::default()
        };
        cfg.validate().expect("tunables are valid");
        let pm = cfg.peer_manager_config();
        assert_eq!(pm.scoring.temp_ban_threshold, 0.35);
        assert_eq!(pm.max_connections, 12);
        assert_eq!(pm.max_per_ip_range, 3);
        assert_eq!(pm.network_id, 3);
        assert_eq!(pm.scoring.warn_threshold, 0.4, "ADR-0022 default");
        assert_eq!(pm.scoring.persistent_ban_threshold, 0.05);
        assert!(
            pm.network_json_path.is_none(),
            "path is supplied at wiring time, not from config.toml"
        );
    }

    #[test]
    fn peer_scoring_tunables_default_when_missing_from_config_file() {
        // Pre-S2-P17 files (no peer_scoring_* keys) must keep parsing and
        // get the ADR-0022 initial tables — S2-P17 serde defaults.
        let toml = r#"
            network_id = 3
            node_mode = "full"
            log_level = "info"
            data_dir = "./data"

            [network]
            listen_addr = "127.0.0.1:8081"
            seeds = []
            max_peers = 50

            [storage]
            path = "./data/leveldb"
        "#;
        let cfg: Config = toml::from_str(toml).expect("legacy config must parse");
        assert_eq!(cfg.network.peer_scoring_w_latency, 0.2);
        assert_eq!(cfg.network.peer_scoring_w_invalid, 0.5);
        assert_eq!(cfg.network.peer_scoring_w_violation, 0.2);
        assert_eq!(cfg.network.peer_scoring_w_uptime, 0.1);
        assert_eq!(cfg.network.peer_scoring_warn_threshold, 0.4);
        assert_eq!(cfg.network.peer_scoring_persistent_threshold, 0.05);
        assert_eq!(cfg.network.peer_scoring_temp_ban_duration_sec, 3600);
        assert_eq!(
            cfg.network.peer_scoring_persistent_ban_duration_sec,
            60 * 60 * 24 * 30
        );
        assert_eq!(cfg.network.peer_scoring_invalid_full_weight, 6);
        assert_eq!(cfg.network.peer_scoring_violation_full_weight, 4);
        cfg.validate().expect("defaults are valid");
    }

    #[cfg(feature = "net")]
    #[test]
    fn peer_scoring_config_defaults_match_adr0022() {
        let pm = Config::default().peer_manager_config();
        let dflt = strangecoin_net::ScoringConfig::default();
        assert_eq!(pm.scoring, dflt);
    }

    #[test]
    fn peer_scoring_tunables_reject_invalid_values() {
        let base = Config::default();

        let inverted_bands = Config {
            network: NetworkConfig {
                peer_scoring_warn_threshold: 0.1,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(inverted_bands.validate().is_err());

        let negative_weight = Config {
            network: NetworkConfig {
                peer_scoring_w_invalid: -0.1,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(negative_weight.validate().is_err());

        let zero_grading = Config {
            network: NetworkConfig {
                peer_scoring_w_latency: 0.0,
                peer_scoring_w_uptime: 0.0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(zero_grading.validate().is_err());

        let zero_full_weight = Config {
            network: NetworkConfig {
                peer_scoring_invalid_full_weight: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(zero_full_weight.validate().is_err());

        let zero_duration = Config {
            network: NetworkConfig {
                peer_scoring_temp_ban_duration_sec: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(zero_duration.validate().is_err());
    }

    #[cfg(feature = "net")]
    #[test]
    fn peer_scoring_maps_file_tunables() {
        let cfg = Config {
            network: NetworkConfig {
                peer_ban_threshold: 0.25,
                peer_scoring_w_latency: 0.3,
                peer_scoring_w_invalid: 0.4,
                peer_scoring_w_violation: 0.2,
                peer_scoring_w_uptime: 0.1,
                peer_scoring_warn_threshold: 0.6,
                peer_scoring_persistent_threshold: 0.08,
                peer_scoring_temp_ban_duration_sec: 1800,
                peer_scoring_persistent_ban_duration_sec: 86_400,
                peer_scoring_invalid_full_weight: 9,
                peer_scoring_violation_full_weight: 5,
                ..Config::default().network
            },
            ..Config::default()
        };
        cfg.validate().expect("tunables are valid");
        let pm = cfg.peer_manager_config();
        assert_eq!(pm.scoring.temp_ban_threshold, 0.25);
        assert_eq!(pm.scoring.w_latency, 0.3);
        assert_eq!(pm.scoring.w_invalid, 0.4);
        assert_eq!(pm.scoring.w_violation, 0.2);
        assert_eq!(pm.scoring.w_uptime, 0.1);
        assert_eq!(pm.scoring.warn_threshold, 0.6);
        assert_eq!(pm.scoring.persistent_ban_threshold, 0.08);
        assert_eq!(pm.scoring.temp_ban_duration, Duration::from_secs(1800));
        assert_eq!(
            pm.scoring.persistent_ban_duration,
            Duration::from_secs(86_400)
        );
        assert_eq!(pm.scoring.invalid_full_weight, 9);
        assert_eq!(pm.scoring.violation_full_weight, 5);
    }

    #[test]
    fn rate_limit_defaults_when_missing_from_config_file() {
        // Stage 1.5-era files (no rate_limit keys) must keep parsing and get
        // the ADR-0023 initial table — S2-P16 serde defaults.
        let toml = r#"
            network_id = 3
            node_mode = "full"
            log_level = "info"
            data_dir = "./data"

            [network]
            listen_addr = "127.0.0.1:8081"
            seeds = []
            max_peers = 50

            [storage]
            path = "./data/leveldb"
        "#;
        let cfg: Config = toml::from_str(toml).expect("legacy config must parse");
        assert_eq!(cfg.network.rate_limit_ban_threshold, 5);
        assert_eq!(cfg.network.rate_limit_ban_window_sec, 60);
        assert_eq!(cfg.network.rate_limit_ban_duration_sec, 300);
        assert_eq!(cfg.network.rate_limit_main_capacity, 1000);
        assert_eq!(cfg.network.rate_limit_main_refill, 100);
        assert_eq!(cfg.network.rate_limit_per_type.inv, "100/10s");
        assert_eq!(cfg.network.rate_limit_per_type.block, "30/10s");
        assert_eq!(cfg.network.rate_limit_per_type.tx, "1000/10s");
        assert_eq!(cfg.network.rate_limit_per_type.get_headers, "100/10s");
        assert_eq!(cfg.network.rate_limit_per_type.get_blocks, "100/10s");
        cfg.validate().expect("defaults are valid");
    }

    #[test]
    fn rate_limit_tunables_reject_invalid_values() {
        let base = Config::default();

        let zero_threshold = Config {
            network: NetworkConfig {
                rate_limit_ban_threshold: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(zero_threshold.validate().is_err());

        let zero_capacity = Config {
            network: NetworkConfig {
                rate_limit_main_capacity: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(zero_capacity.validate().is_err());

        let zero_refill = Config {
            network: NetworkConfig {
                rate_limit_main_refill: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(zero_refill.validate().is_err());

        let zero_window = Config {
            network: NetworkConfig {
                rate_limit_ban_window_sec: 0,
                ..base.network.clone()
            },
            ..Config::default()
        };
        assert!(zero_window.validate().is_err());
    }

    #[cfg(feature = "net")]
    #[test]
    fn rate_limit_per_type_rejects_malformed_specs() {
        let base = Config::default();
        let bad_spec = Config {
            network: NetworkConfig {
                rate_limit_per_type: RateLimitPerTypeConfig {
                    inv: "abc".into(),
                    ..RateLimitPerTypeConfig::default()
                },
                ..base.network.clone()
            },
            ..Config::default()
        };
        let err = bad_spec
            .validate()
            .expect_err("malformed inv spec must be rejected");
        assert!(
            err.to_string().contains("rate_limit_per_type.inv"),
            "error names the offending key: {err}"
        );
    }

    #[cfg(feature = "net")]
    #[test]
    fn rate_limit_config_maps_file_tunables() {
        let cfg = Config {
            network: NetworkConfig {
                rate_limit_ban_threshold: 9,
                rate_limit_ban_window_sec: 30,
                rate_limit_ban_duration_sec: 120,
                rate_limit_main_capacity: 5000,
                rate_limit_main_refill: 50,
                rate_limit_per_type: RateLimitPerTypeConfig {
                    inv: "200/10s".into(),
                    ..RateLimitPerTypeConfig::default()
                },
                ..Config::default().network
            },
            ..Config::default()
        };
        cfg.validate().expect("tunables are valid");
        let rl = cfg.rate_limit_config();
        assert_eq!(rl.main.capacity, 5000);
        assert_eq!(rl.main.refill, 50);
        assert_eq!(rl.ban_threshold, 9);
        assert_eq!(rl.ban_window, Duration::from_secs(30));
        assert_eq!(rl.ban_duration, Duration::from_secs(120));
        assert_eq!(rl.inv.capacity, 200);
        assert_eq!(rl.inv.refill, 20, "200/10s => 20 tokens/s");
        // Untouched keys keep ADR-0023 defaults.
        assert_eq!(rl.block.capacity, 30);
        assert_eq!(rl.block.refill, 3);
        assert_eq!(rl.tx.capacity, 1000);
        assert_eq!(rl.tx.refill, 100);
        assert_eq!(rl.get_headers.capacity, 100);
        assert_eq!(rl.get_blocks.refill, 10);
    }

    #[cfg(feature = "net")]
    #[test]
    fn rate_limit_config_defaults_match_adr0023() {
        let rl = Config::default().rate_limit_config();
        let dflt = strangecoin_net::RateLimitConfig::default();
        assert_eq!(rl, dflt);
    }
}

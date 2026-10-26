use crate::error::StrangecoinError;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

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
    /// `strangecoin_net::peer::DEFAULT_BAN_THRESHOLD`).
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

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            listen_addr: "127.0.0.1:8081".parse().unwrap(),
            seeds: vec![],
            max_peers: 50,
            peer_ban_threshold: default_peer_ban_threshold(),
            max_connections: default_max_connections(),
            max_per_ip_range: default_max_per_ip_range(),
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
    /// tunables (S2-P12).
    ///
    /// Only the three file tunables are overridden; scoring thresholds,
    /// ban durations and capabilities keep the ADR-0022 defaults until
    /// S2-P17 exposes them as `peer_scoring_*` keys. The identity keypair
    /// (Ed25519 + derived Noise static) and the `network.json` path are
    /// supplied at construction time by the orchestrator wiring (S2-P13/
    /// S2-P19).
    pub fn peer_manager_config(&self) -> strangecoin_net::PeerManagerConfig {
        strangecoin_net::PeerManagerConfig {
            ban_threshold: self.network.peer_ban_threshold,
            max_connections: self.network.max_connections,
            max_per_ip_range: self.network.max_per_ip_range,
            network_id: self.network_id,
            ..strangecoin_net::PeerManagerConfig::default()
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
        let pm = cfg.peer_manager_config();
        assert_eq!(pm.ban_threshold, 0.35);
        assert_eq!(pm.max_connections, 12);
        assert_eq!(pm.max_per_ip_range, 3);
        assert_eq!(pm.network_id, 3);
        assert_eq!(pm.warn_threshold, 0.4, "ADR-0022 default until S2-P17");
        assert_eq!(pm.persistent_threshold, 0.05);
        assert!(
            pm.network_json_path.is_none(),
            "path is supplied at wiring time, not from config.toml"
        );
    }
}

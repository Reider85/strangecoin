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
        self.allow_zero_state_root
            .unwrap_or(self.network_id == 3)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            network_id: 3,
            node_mode: NodeMode::default(),
            network: NetworkConfig {
                listen_addr: "127.0.0.1:8081".parse().unwrap(),
                seeds: vec![],
                max_peers: 50,
            },
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
        assert!(!mainnet.zero_state_root_allowed(), "mainnet default is false");

        let testnet = Config {
            network_id: 2,
            ..Config::default()
        };
        assert!(!testnet.zero_state_root_allowed(), "testnet default is false");
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
            assert!(
                err.to_string().contains("allow_zero_state_root"),
                "{err:?}"
            );
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
}

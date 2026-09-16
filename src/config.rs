use crate::error::StrangecoinError;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeMode {
    Full,
    Light,
    Archival,
}

impl Default for NodeMode {
    fn default() -> Self {
        NodeMode::Full
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub network_id: u32,
    pub node_mode: NodeMode,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub log_level: String,
    pub data_dir: PathBuf,
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
        }
    }
}

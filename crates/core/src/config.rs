use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global network CIDR — all nodes get an IP from this pool
pub const NETWORK_CIDR: &str = "10.13.0.0/16";

/// Control server default bind address
pub const CCS_DEFAULT_ADDR: &str = "0.0.0.0:9090";

/// STUN server used for NAT detection
pub const STUN_SERVERS: &[&str] = &[
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
];

/// Persistent node configuration stored on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Unique node ID (generated on first registration)
    pub node_id: String,
    /// Human-readable name
    pub name: String,
    /// This node's WireGuard private key (base64)
    pub private_key: String,
    /// This node's assigned VPN IP
    pub vpn_ip: String,
    /// Control server address
    pub ccs_addr: String,
}

impl NodeConfig {
    /// Load from default config path: ~/.tinyvpn/config.json
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path()?;
        let data = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&data)?)
    }

    /// Save to default config path
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    /// Check if a config already exists
    pub fn exists() -> bool {
        Self::config_path().map(|p| p.exists()).unwrap_or(false)
    }

    fn config_path() -> anyhow::Result<PathBuf> {
        let home = dirs_home()?;
        Ok(home.join(".tinyvpn").join("config.json"))
    }
}

fn dirs_home() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
    ))
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global network CIDR — all nodes get an IP from this pool
pub const NETWORK_CIDR: &str = "10.13.0.0/16";

/// Control server default bind address
pub const CCS_DEFAULT_ADDR: &str = "0.0.0.0:9090";

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
    /// Session token for authenticated requests
    pub session_token: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> NodeConfig {
        NodeConfig {
            node_id: "node-test".into(),
            name: "test".into(),
            private_key: "privkey123".into(),
            vpn_ip: "10.13.0.5".into(),
            ccs_addr: "127.0.0.1:9090".into(),
            session_token: "tok123".into(),
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join("tinyvpn_test_config");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = make_config();
        let path = dir.join("config.json");
        let data = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &data).unwrap();

        let loaded: NodeConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(loaded.node_id, config.node_id);
        assert_eq!(loaded.name, config.name);
        assert_eq!(loaded.vpn_ip, config.vpn_ip);
        assert_eq!(loaded.session_token, config.session_token);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_json_has_expected_fields() {
        let config = make_config();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"node_id\""));
        assert!(json.contains("\"vpn_ip\""));
        assert!(json.contains("\"session_token\""));
    }
}

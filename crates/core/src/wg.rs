use anyhow::{Context, Result};
use std::path::PathBuf;

const WG_CONFIG_DIR: &str = "/etc/wireguard";

pub struct WgInterface {
    name: String,
    config_path: PathBuf,
}

impl WgInterface {
    pub fn new(name: &str) -> Self {
        let config_path = PathBuf::from(WG_CONFIG_DIR).join(format!("{}.conf", name));
        Self {
            name: name.to_string(),
            config_path,
        }
    }

    /// Create wg-quick config and bring interface up
    pub fn setup(&self, our_ip: &str, private_key: &str, listen_port: u16) -> Result<()> {
        let config = format!(
            "[Interface]\n\
             PrivateKey = {}\n\
             Address = {}/16\n\
             ListenPort = {}\n\n",
            private_key, our_ip, listen_port
        );
        std::fs::write(&self.config_path, &config)
            .with_context(|| format!("writing config to {:?}", self.config_path))?;

        self.run_wg_quick("up")?;
        tracing::info!("WireGuard interface {} is up", self.name);
        Ok(())
    }

    /// Add a peer to the interface
    pub fn add_peer(
        &self,
        public_key: &str,
        allowed_ip: &str,
        endpoint: Option<&str>,
    ) -> Result<()> {
        let mut cmd = std::process::Command::new("wg");
        cmd.args(["set", &self.name, "peer", public_key, "allowed-ips", allowed_ip]);
        if let Some(ep) = endpoint {
            cmd.args(["endpoint", ep]);
        }
        let status = cmd.status().with_context(|| "running wg set")?;
        if !status.success() {
            anyhow::bail!("wg set failed for peer {}", public_key);
        }
        tracing::info!("Added peer {} (allowed-ips: {})", public_key, allowed_ip);
        Ok(())
    }

    /// Remove a peer from the interface
    pub fn remove_peer(&self, public_key: &str) -> Result<()> {
        let status = std::process::Command::new("wg")
            .args(["set", &self.name, "peer", public_key, "remove"])
            .status()
            .with_context(|| "running wg set")?;
        if !status.success() {
            anyhow::bail!("wg set remove failed for peer {}", public_key);
        }
        Ok(())
    }

    /// Tear down the interface
    pub fn teardown(&self) -> Result<()> {
        self.run_wg_quick("down")?;
        let _ = std::fs::remove_file(&self.config_path);
        tracing::info!("WireGuard interface {} is down", self.name);
        Ok(())
    }

    fn run_wg_quick(&self, action: &str) -> Result<()> {
        let status = std::process::Command::new("wg-quick")
            .args([action, &self.config_path.to_string_lossy()])
            .status()
            .with_context(|| format!("running wg-quick {}", action))?;
        if !status.success() {
            anyhow::bail!("wg-quick {} failed for {}", action, self.name);
        }
        Ok(())
    }
}

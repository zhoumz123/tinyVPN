use anyhow::{bail, Context, Result};
use std::net::UdpSocket;
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

    /// Create WireGuard interface and bring it up (bypasses wg-quick for better error handling)
    pub fn setup(&self, our_ip: &str, private_key: &str, listen_port: u16) -> Result<()> {
        // Check port availability
        if let Err(e) = UdpSocket::bind(format!("0.0.0.0:{}", listen_port)) {
            bail!(
                "UDP port {} is already in use (another WireGuard interface?). Use --port to specify a different one. Error: {}",
                listen_port, e
            );
        }

        // Write wg-quick compatible config for reference
        let full_config = format!(
            "[Interface]\n\
             PrivateKey = {}\n\
             Address = {}/16\n\
             ListenPort = {}\n",
            private_key, our_ip, listen_port
        );
        std::fs::write(&self.config_path, &full_config)
            .with_context(|| format!("writing config to {:?}", self.config_path))?;

        // Write wg-only config (no Address field) for wg setconf
        let wg_config_path = PathBuf::from(WG_CONFIG_DIR).join(format!("{}.wg.conf", self.name));
        let wg_config = format!(
            "[Interface]\n\
             PrivateKey = {}\n\
             ListenPort = {}\n",
            private_key, listen_port
        );
        std::fs::write(&wg_config_path, &wg_config)
            .with_context(|| format!("writing wg config to {:?}", wg_config_path))?;

        // Create interface
        let status = std::process::Command::new("ip")
            .args(["link", "add", &self.name, "type", "wireguard"])
            .status()
            .with_context(|| "running ip link add")?;
        if !status.success() {
            anyhow::bail!("failed to create WireGuard interface {}", self.name);
        }

        // Configure WireGuard (use stripped config without Address)
        let wg_config_path = PathBuf::from(WG_CONFIG_DIR).join(format!("{}.wg.conf", self.name));
        let status = std::process::Command::new("wg")
            .args(["setconf", &self.name, &wg_config_path.to_string_lossy()])
            .status()
            .with_context(|| "running wg setconf")?;
        if !status.success() {
            let _ = Self::force_delete(&self.name);
            anyhow::bail!("wg setconf failed for {}", self.name);
        }

        // Assign IP address
        let status = std::process::Command::new("ip")
            .args(["address", "add", &format!("{}/16", our_ip), "dev", &self.name])
            .status()
            .with_context(|| "running ip address add")?;
        if !status.success() {
            let _ = Self::force_delete(&self.name);
            anyhow::bail!("failed to assign IP {} to {}", our_ip, self.name);
        }

        // Bring interface up
        let status = std::process::Command::new("ip")
            .args(["link", "set", "mtu", "1420", "up", "dev", &self.name])
            .status()
            .with_context(|| "running ip link set up")?;
        if !status.success() {
            let _ = Self::force_delete(&self.name);
            anyhow::bail!("failed to bring up interface {}", self.name);
        }

        tracing::info!("WireGuard interface {} is up ({})", self.name, our_ip);
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
            bail!("wg set failed for peer {}", public_key);
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
            bail!("wg set remove failed for peer {}", public_key);
        }
        Ok(())
    }

    /// Tear down the interface
    pub fn teardown(&self) -> Result<()> {
        Self::force_delete(&self.name)?;
        let _ = std::fs::remove_file(&self.config_path);
        let wg_config_path = PathBuf::from(WG_CONFIG_DIR).join(format!("{}.wg.conf", self.name));
        let _ = std::fs::remove_file(&wg_config_path);
        tracing::info!("WireGuard interface {} is down", self.name);
        Ok(())
    }

    fn force_delete(name: &str) -> Result<()> {
        let status = std::process::Command::new("ip")
            .args(["link", "delete", "dev", name])
            .status()
            .with_context(|| "running ip link delete")?;
        if !status.success() {
            bail!("failed to delete interface {}", name);
        }
        Ok(())
    }
}

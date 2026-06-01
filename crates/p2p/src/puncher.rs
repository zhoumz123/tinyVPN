use anyhow::Result;
use std::net::SocketAddr;
use tracing;

/// Performs UDP hole punching with a peer
///
/// Both sides simultaneously send UDP packets to each other's public endpoint.
/// This opens a pinhole in each side's NAT, allowing bidirectional communication.
pub struct Puncher {
    /// Local UDP socket for punching
    socket: tokio::net::UdpSocket,
}

impl Puncher {
    /// Create a new puncher bound to a random port
    pub async fn new() -> Result<Self> {
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
        tracing::info!(
            "Puncher bound to {}",
            socket.local_addr()?
        );
        Ok(Self { socket })
    }

    /// Get the local address we're bound to
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }

    /// Attempt hole punching: send N packets to the peer's public endpoint
    pub async fn punch(&self, peer_endpoint: SocketAddr, our_node_id: &str) -> Result<()> {
        tracing::info!("Punching to {}...", peer_endpoint);

        let punch_msg = format!("PUNCH:{}", our_node_id);
        let data = punch_msg.as_bytes();

        // Send multiple punch packets with small delays
        for i in 0..10 {
            self.socket.send_to(data, peer_endpoint).await?;
            tracing::debug!("Punch packet {} sent to {}", i + 1, peer_endpoint);

            // Check if we received anything back
            let mut buf = [0u8; 1024];
            match tokio::time::timeout(
                std::time::Duration::from_millis(200),
                self.socket.recv_from(&mut buf),
            )
            .await
            {
                Ok(Ok((n, from))) => {
                    let msg = String::from_utf8_lossy(&buf[..n]);
                    if msg.starts_with("PUNCH:") {
                        tracing::info!("✅ Hole punched! Connected to {} → {}", from, msg);
                        return Ok(());
                    }
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => continue, // timeout, send next
            }
        }

        tracing::warn!("Hole punching failed after 10 attempts, need relay");
        anyhow::bail!("Hole punching failed — relay fallback not implemented yet")
    }

    /// Get a reference to the underlying socket (for WireGuard integration)
    pub fn socket(&self) -> &tokio::net::UdpSocket {
        &self.socket
    }
}

//! TinyVPN Relay — UDP packet forwarding for punch-failure fallback.
//!
//! Relayed traffic is attributed by **node identity**, not source port.
//! Nodes register with `REGISTER:<my_id>:<peer_id>`; the relay attributes
//! later packets by source IP and **learns each node's real WireGuard
//! endpoint** (port roaming) from observed traffic. This is required because
//! registration is sent from an ephemeral userspace socket while the actual
//! WireGuard data flows from the kernel WG listen port — different ports on
//! the same IP, so matching by full source address would never succeed.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use anyhow::Result;

const NODE_TIMEOUT_SECS: u64 = 120;
const BUF_SIZE: usize = 65535;

/// A registered node: the peers it wants to reach and its learned WG endpoint.
struct NodeEntry {
    /// All peers this node registered to relay to.
    peer_ids: HashSet<String>,
    /// Real WireGuard endpoint learned from observed packets (port roaming).
    wg_addr: Option<SocketAddr>,
    last_activity: Instant,
}

pub struct Relay {
    socket: Arc<UdpSocket>,
    /// node_id -> entry
    nodes: Arc<Mutex<HashMap<String, NodeEntry>>>,
    /// source IP -> node_id (attributes incoming packets to a node by IP)
    ip_index: Arc<Mutex<HashMap<IpAddr, String>>>,
}

impl Relay {
    pub async fn bind(addr: &str) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        tracing::info!("Relay listening on {}", socket.local_addr()?);
        Ok(Self {
            socket: Arc::new(socket),
            nodes: Arc::new(Mutex::new(HashMap::new())),
            ip_index: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Run the forwarding loop.
    pub async fn run(&self) -> Result<()> {
        let nodes = self.nodes.clone();
        let ip_index = self.ip_index.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                let now = Instant::now();
                let mut nodes = nodes.lock().await;
                nodes.retain(|id, e| {
                    let keep = now.duration_since(e.last_activity).as_secs() < NODE_TIMEOUT_SECS;
                    if !keep {
                        tracing::info!("Reaped stale node {}", id);
                    }
                    keep
                });
                // Rebuild IP index from surviving nodes' last-known endpoints.
                let mut idx = ip_index.lock().await;
                idx.clear();
                for (id, e) in nodes.iter() {
                    if let Some(addr) = e.wg_addr {
                        idx.insert(addr.ip(), id.clone());
                    }
                }
            }
        });

        let mut buf = vec![0u8; BUF_SIZE];
        loop {
            let (n, from) = self.socket.recv_from(&mut buf).await?;
            let msg = String::from_utf8_lossy(&buf[..n]);

            if let Some(rest) = msg.strip_prefix("REGISTER:") {
                let parts: Vec<&str> = rest.splitn(2, ':').collect();
                if parts.len() == 2 {
                    self.handle_register(from, parts[0].to_string(), parts[1].to_string())
                        .await;
                }
                let _ = self.socket.send_to(b"OK", from).await;
                continue;
            }

            self.forward(&buf[..n], from).await;
        }
    }

    async fn handle_register(&self, from: SocketAddr, my_id: String, peer_id: String) {
        {
            let mut nodes = self.nodes.lock().await;
            let entry = nodes.entry(my_id.clone()).or_insert(NodeEntry {
                peer_ids: HashSet::new(),
                // Seed with the registration source; real WG traffic roam-updates it.
                wg_addr: Some(from),
                last_activity: Instant::now(),
            });
            entry.peer_ids.insert(peer_id.clone());
            entry.wg_addr = Some(from);
            entry.last_activity = Instant::now();
        }
        // Attribute future packets from this IP to this node.
        self.ip_index.lock().await.insert(from.ip(), my_id.clone());
        tracing::info!("Registered {} -> {} from {}", my_id, peer_id, from);
    }

    /// Forward a data packet, learning the source's real WG endpoint.
    ///
    /// Fan-out: forward to ALL peers this node registered for. WireGuard
    /// packets are sealed for a specific recipient public key, so only the
    /// intended peer accepts each packet — the rest drop it. This lets a node
    /// reach multiple relayed peers through one source socket.
    async fn forward(&self, data: &[u8], from: SocketAddr) {
        let node_id = {
            let ip_index = self.ip_index.lock().await;
            ip_index.get(&from.ip()).cloned()
        };
        let Some(node_id) = node_id else {
            tracing::warn!("Packet from unknown IP: {}", from.ip());
            return;
        };

        let peer_addrs = {
            let mut nodes = self.nodes.lock().await;
            if let Some(entry) = nodes.get_mut(&node_id) {
                // Port learning / roaming: record the real WG source address.
                entry.wg_addr = Some(from);
                entry.last_activity = Instant::now();
            }
            let Some(entry) = nodes.get(&node_id) else { return };
            entry
                .peer_ids
                .iter()
                .filter_map(|pid| nodes.get(pid).and_then(|p| p.wg_addr))
                .collect::<Vec<_>>()
        };

        if peer_addrs.is_empty() {
            // No peer has sent yet; we've learned our endpoint, will work once
            // a peer's first packet arrives.
            tracing::debug!("No peer endpoint yet for packet from {}", from);
            return;
        }
        for addr in peer_addrs {
            if let Err(e) = self.socket.send_to(data, addr).await {
                tracing::warn!("Relay send_to {} failed: {}", addr, e);
            } else {
                tracing::trace!("Relayed {} bytes {} -> {}", data.len(), from, addr);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_pair_and_forward() {
        let relay = Relay::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.socket.local_addr().unwrap();

        let relay_handle = tokio::spawn(async move {
            let _ = relay.run().await;
        });

        // Distinct loopback IPs model two separate hosts.
        let node_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let node_b = UdpSocket::bind("127.0.0.2:0").await.unwrap();
        let mut buf = [0u8; 64];

        node_a.send_to(b"REGISTER:node-a:node-b", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), node_a.recv_from(&mut buf)).await.unwrap();
        node_b.send_to(b"REGISTER:node-b:node-a", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), node_b.recv_from(&mut buf)).await.unwrap();

        node_a.send_to(b"hello from A", relay_addr).await.unwrap();
        let (n, _) = tokio::time::timeout(std::time::Duration::from_secs(2), node_b.recv_from(&mut buf)).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"hello from A");

        node_b.send_to(b"hello from B", relay_addr).await.unwrap();
        let (n, _) = tokio::time::timeout(std::time::Duration::from_secs(2), node_a.recv_from(&mut buf)).await.unwrap().unwrap();
        assert_eq!(&buf[..n], b"hello from B");

        relay_handle.abort();
    }

    #[tokio::test]
    async fn relay_learns_wg_port_separate_from_register() {
        // Models the real topology: REGISTER uses an ephemeral userspace socket,
        // but WireGuard data flows from the kernel WG listen port — a DIFFERENT
        // port on the same IP. The relay must learn each peer's real WG port
        // from observed traffic (port roaming) and route to it, not to the
        // registration port.
        let relay = Relay::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.socket.local_addr().unwrap();

        let h = tokio::spawn(async move {
            let _ = relay.run().await;
        });

        // Registration sockets (ephemeral), distinct IPs to model two hosts.
        let reg_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let reg_b = UdpSocket::bind("127.0.0.2:0").await.unwrap();
        let mut buf = [0u8; 64];

        reg_a.send_to(b"REGISTER:node-a:node-b", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reg_a.recv_from(&mut buf)).await.unwrap();
        reg_b.send_to(b"REGISTER:node-b:node-a", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reg_b.recv_from(&mut buf)).await.unwrap();

        // WireGuard data sockets: DIFFERENT ports than registration.
        let wg_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let wg_b = UdpSocket::bind("127.0.0.2:0").await.unwrap();

        // B sends first so the relay learns B's REAL WG port (roaming from the
        // registration seed). The relay forwards this to A's seeded endpoint;
        // drain it so it doesn't sit in the register socket's buffer.
        wg_b.send_to(b"handshake-b", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_millis(500), reg_a.recv_from(&mut buf)).await;

        // A sends from its real WG port; the relay must route to B's REAL WG
        // port (learned above), not B's registration port.
        wg_a.send_to(b"data-a", relay_addr).await.unwrap();
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            wg_b.recv_from(&mut buf),
        ).await.expect("B's real WG port was never learned (port roaming broken)").unwrap();
        assert_eq!(&buf[..n], b"data-a");

        h.abort();
    }

    #[tokio::test]
    async fn relay_fanout_to_multiple_peers() {
        // A node relayed to multiple peers must reach ALL of them. The relay
        // fan-outs to every registered peer; WireGuard's per-key encryption means
        // only the intended recipient accepts each packet.
        let relay = Relay::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.socket.local_addr().unwrap();

        let h = tokio::spawn(async move {
            let _ = relay.run().await;
        });

        let reg_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let reg_b = UdpSocket::bind("127.0.0.2:0").await.unwrap();
        let reg_c = UdpSocket::bind("127.0.0.3:0").await.unwrap();
        let mut buf = [0u8; 64];

        // A registers for BOTH B and C; B and C each register for A.
        reg_a.send_to(b"REGISTER:node-a:node-b", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reg_a.recv_from(&mut buf)).await.unwrap();
        reg_a.send_to(b"REGISTER:node-a:node-c", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reg_a.recv_from(&mut buf)).await.unwrap();
        reg_b.send_to(b"REGISTER:node-b:node-a", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reg_b.recv_from(&mut buf)).await.unwrap();
        reg_c.send_to(b"REGISTER:node-c:node-a", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reg_c.recv_from(&mut buf)).await.unwrap();

        // B and C send first so the relay learns their real WG ports.
        let wg_a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let wg_b = UdpSocket::bind("127.0.0.2:0").await.unwrap();
        let wg_c = UdpSocket::bind("127.0.0.3:0").await.unwrap();
        wg_b.send_to(b"learn-b", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_millis(500), reg_a.recv_from(&mut buf)).await;
        wg_c.send_to(b"learn-c", relay_addr).await.unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_millis(500), reg_a.recv_from(&mut buf)).await;

        // A sends one packet; BOTH B and C must receive it (fan-out).
        wg_a.send_to(b"broadcast", relay_addr).await.unwrap();
        let (n, _) = tokio::time::timeout(std::time::Duration::from_secs(2), wg_b.recv_from(&mut buf))
            .await.expect("B never received (fan-out broken)").unwrap();
        assert_eq!(&buf[..n], b"broadcast");
        let (n, _) = tokio::time::timeout(std::time::Duration::from_secs(2), wg_c.recv_from(&mut buf))
            .await.expect("C never received (fan-out broken)").unwrap();
        assert_eq!(&buf[..n], b"broadcast");

        h.abort();
    }

    #[tokio::test]
    async fn unknown_packet_dropped() {
        let relay = Relay::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.socket.local_addr().unwrap();

        let relay_handle = tokio::spawn(async move {
            let _ = relay.run().await;
        });

        let stranger = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        stranger.send_to(b"random data", relay_addr).await.unwrap();

        // Nothing should come back — no registration for this IP.
        let mut buf = [0u8; 64];
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            stranger.recv_from(&mut buf),
        ).await;
        assert!(result.is_err());

        relay_handle.abort();
    }
}

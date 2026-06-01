use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tinyvpn_core::protocol::{ControlMessage, PeerInfo};

/// Global registry of all registered nodes
#[derive(Debug, Default)]
pub struct Registry {
    /// node_id → PeerInfo
    nodes: HashMap<String, PeerInfo>,
    /// Next IP to assign (simple counter from 10.13.0.1)
    next_ip_counter: u32,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            next_ip_counter: 1,
        }
    }

    /// Register a new node, return its assigned node_id and VPN IP
    pub fn register(&mut self, name: String, public_key: String) -> (String, String) {
        let node_id = format!("node-{}", uuid_short());
        let vpn_ip = self.next_ip();

        let peer = PeerInfo {
            node_id: node_id.clone(),
            name,
            vpn_ip: vpn_ip.clone(),
            public_key,
            endpoint: String::new(),
            connected: true,
        };

        self.nodes.insert(node_id.clone(), peer);
        (node_id, vpn_ip)
    }

    /// Update a node's public endpoint (from STUN)
    pub fn update_endpoint(&mut self, node_id: &str, endpoint: String) {
        if let Some(peer) = self.nodes.get_mut(node_id) {
            peer.endpoint = endpoint;
            tracing::info!("Updated endpoint for {}: {}", node_id, endpoint);
        }
    }

    /// Get all peers (excluding the requesting node)
    pub fn get_peers(&self, exclude_id: Option<&str>) -> Vec<PeerInfo> {
        self.nodes
            .values()
            .filter(|p| exclude_id.map_or(true, |id| p.node_id != id))
            .cloned()
            .collect()
    }

    /// Get a specific peer's info
    pub fn get_peer(&self, node_id: &str) -> Option<PeerInfo> {
        self.nodes.get(node_id).cloned()
    }

    fn next_ip(&mut self) -> String {
        let ip = format!("10.13.0.{}", self.next_ip_counter);
        self.next_ip_counter += 1;
        ip
    }
}

pub type SharedRegistry = Arc<RwLock<Registry>>;

fn uuid_short() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..8).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

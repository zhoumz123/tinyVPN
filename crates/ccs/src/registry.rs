use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tinyvpn_core::protocol::PeerInfo;

/// Registered node with auth and heartbeat state
#[derive(Debug)]
struct NodeEntry {
    info: PeerInfo,
    session_token: String,
    last_heartbeat: Instant,
}

/// Global registry of all registered nodes
#[derive(Debug, Default)]
pub struct Registry {
    /// node_id → NodeEntry
    nodes: HashMap<String, NodeEntry>,
    /// Next IP to assign (simple counter from 10.13.0.1)
    next_ip_counter: u32,
    /// Relay server address (set via env)
    relay_addr: String,
}

impl Registry {
    pub fn new(relay_addr: String) -> Self {
        Self {
            nodes: HashMap::new(),
            next_ip_counter: 1,
            relay_addr,
        }
    }

    /// Register a new node, return (node_id, vpn_ip, session_token)
    pub fn register(&mut self, name: String, public_key: String) -> (String, String, String) {
        let node_id = format!("node-{}", uuid_short());
        let vpn_ip = self.next_ip();
        let session_token = uuid_short();

        let peer = PeerInfo {
            node_id: node_id.clone(),
            name,
            vpn_ip: vpn_ip.clone(),
            public_key,
            endpoint: String::new(),
            connected: true,
        };

        self.nodes.insert(
            node_id.clone(),
            NodeEntry {
                info: peer,
                session_token: session_token.clone(),
                last_heartbeat: Instant::now(),
            },
        );
        (node_id, vpn_ip, session_token)
    }

    /// Validate session token, return true if valid
    pub fn validate_session(&self, node_id: &str, token: &str) -> bool {
        self.nodes
            .get(node_id)
            .map(|e| e.session_token == token)
            .unwrap_or(false)
    }

    /// Update a node's public endpoint (from STUN)
    pub fn update_endpoint(&mut self, node_id: &str, endpoint: String) {
        if let Some(entry) = self.nodes.get_mut(node_id) {
            entry.info.endpoint = endpoint.clone();
            entry.last_heartbeat = Instant::now();
            tracing::info!("Updated endpoint for {}: {}", node_id, endpoint);
        }
    }

    /// Record heartbeat
    pub fn heartbeat(&mut self, node_id: &str) {
        if let Some(entry) = self.nodes.get_mut(node_id) {
            entry.last_heartbeat = Instant::now();
            entry.info.connected = true;
        }
    }

    /// Mark stale nodes as disconnected (no heartbeat for 60s)
    pub fn reap_stale(&mut self) {
        let now = Instant::now();
        for entry in self.nodes.values_mut() {
            if now.duration_since(entry.last_heartbeat).as_secs() > 60 {
                entry.info.connected = false;
            }
        }
    }

    /// Get all peers (excluding the requesting node)
    pub fn get_peers(&self, exclude_id: Option<&str>) -> Vec<PeerInfo> {
        self.nodes
            .values()
            .filter(|e| exclude_id.is_none_or(|id| e.info.node_id != id))
            .map(|e| e.info.clone())
            .collect()
    }

    /// Get a specific peer's info
    pub fn get_peer(&self, node_id: &str) -> Option<PeerInfo> {
        self.nodes.get(node_id).map(|e| e.info.clone())
    }

    /// Get relay address
    pub fn relay_addr(&self) -> &str {
        &self.relay_addr
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

#[cfg(test)]
mod tests {
    use super::*;

    fn new_registry() -> Registry {
        Registry::new("127.0.0.1:9091".into())
    }

    #[test]
    fn register_assigns_sequential_ips() {
        let mut reg = new_registry();
        let (id1, ip1, _) = reg.register("a".into(), "pk1".into());
        let (id2, ip2, _) = reg.register("b".into(), "pk2".into());
        assert!(id1.starts_with("node-"));
        assert!(id2.starts_with("node-"));
        assert_ne!(id1, id2);
        assert_eq!(ip1, "10.13.0.1");
        assert_eq!(ip2, "10.13.0.2");
    }

    #[test]
    fn validate_session_correct() {
        let mut reg = new_registry();
        let (id, _, tok) = reg.register("a".into(), "pk1".into());
        assert!(reg.validate_session(&id, &tok));
    }

    #[test]
    fn validate_session_wrong_token() {
        let mut reg = new_registry();
        let (id, _, _) = reg.register("a".into(), "pk1".into());
        assert!(!reg.validate_session(&id, "wrong"));
    }

    #[test]
    fn validate_session_unknown_node() {
        let reg = new_registry();
        assert!(!reg.validate_session("nonexistent", "any"));
    }

    #[test]
    fn heartbeat_refreshes() {
        let mut reg = new_registry();
        let (id, _, _) = reg.register("a".into(), "pk1".into());

        // Simulate 61 seconds passing
        let entry = reg.nodes.get_mut(&id).unwrap();
        entry.last_heartbeat = Instant::now() - std::time::Duration::from_secs(61);

        reg.heartbeat(&id);
        let entry = reg.nodes.get(&id).unwrap();
        assert!(entry.info.connected);
        assert!(Instant::now().duration_since(entry.last_heartbeat).as_secs() < 5);
    }

    #[test]
    fn reap_stale_marks_offline() {
        let mut reg = new_registry();
        let (id, _, _) = reg.register("a".into(), "pk1".into());

        // Manually age the heartbeat
        let entry = reg.nodes.get_mut(&id).unwrap();
        entry.last_heartbeat = Instant::now() - std::time::Duration::from_secs(61);

        reg.reap_stale();
        let entry = reg.nodes.get(&id).unwrap();
        assert!(!entry.info.connected);
    }

    #[test]
    fn get_peers_excludes_self() {
        let mut reg = new_registry();
        let (id1, _, _) = reg.register("a".into(), "pk1".into());
        let (id2, _, _) = reg.register("b".into(), "pk2".into());

        let peers = reg.get_peers(Some(&id1));
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, id2);
    }

    #[test]
    fn update_endpoint() {
        let mut reg = new_registry();
        let (id, _, _) = reg.register("a".into(), "pk1".into());
        reg.update_endpoint(&id, "1.2.3.4:51820".into());
        let peer = reg.get_peer(&id).unwrap();
        assert_eq!(peer.endpoint, "1.2.3.4:51820");
    }

    #[test]
    fn relay_addr() {
        let reg = Registry::new("10.0.0.1:9091".into());
        assert_eq!(reg.relay_addr(), "10.0.0.1:9091");
    }
}

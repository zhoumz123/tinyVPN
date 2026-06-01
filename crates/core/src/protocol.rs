use serde::{Deserialize, Serialize};

/// Messages sent between nodes and the control server (over QUIC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Node → CCS: Register a new node
    Register {
        name: String,
        public_key: String,
    },

    /// CCS → Node: Registration confirmed with assigned IP
    RegisterOk {
        node_id: String,
        vpn_ip: String,
    },

    /// Node → CCS: Request current peer list
    GetPeers,

    /// CCS → Node: Full peer list
    PeerList {
        peers: Vec<PeerInfo>,
    },

    /// Node → CCS: Report its current public endpoint (after STUN)
    UpdateEndpoint {
        public_addr: String,
    },

    /// CCS → Node: Signal to try hole-punching with a peer
    PunchRequest {
        peer_id: String,
        peer_public_key: String,
        peer_endpoint: String,
    },

    /// Heartbeat
    Ping,
    Pong,
}

/// Information about a peer in the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_id: String,
    pub name: String,
    pub vpn_ip: String,
    pub public_key: String,
    pub endpoint: String, // public ip:port (from STUN)
    pub connected: bool,
}

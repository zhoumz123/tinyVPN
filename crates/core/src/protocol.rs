use serde::{Deserialize, Serialize};

/// Messages sent between nodes and the control server (over TCP + JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Node → CCS: Register a new node
    Register {
        name: String,
        public_key: String,
    },

    /// CCS → Node: Registration confirmed with assigned IP + session token
    RegisterOk {
        node_id: String,
        vpn_ip: String,
        session_token: String,
    },

    /// Node → CCS: Request current peer list (authenticated)
    GetPeers {
        node_id: String,
        session_token: String,
    },

    /// CCS → Node: Full peer list
    PeerList {
        peers: Vec<PeerInfo>,
    },

    /// Node → CCS: Report its current public endpoint (after STUN)
    UpdateEndpoint {
        node_id: String,
        session_token: String,
        public_addr: String,
    },

    /// Node → CCS: Request relay for a peer connection
    RequestRelay {
        node_id: String,
        session_token: String,
        target_id: String,
    },

    /// CCS → Node: Relay address assigned
    RelayAssigned {
        relay_addr: String,
        target_id: Option<String>,
    },

    /// CCS → Node: Signal to try hole-punching with a peer
    PunchRequest {
        peer_id: String,
        peer_public_key: String,
        peer_endpoint: String,
    },

    /// Heartbeat
    Ping {
        node_id: String,
        session_token: String,
    },
    Pong,
}

/// Information about a peer in the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_id: String,
    pub name: String,
    pub vpn_ip: String,
    pub public_key: String,
    pub endpoint: String,
    pub connected: bool,
}

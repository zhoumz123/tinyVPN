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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: ControlMessage) {
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: ControlMessage = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&decoded).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn register_roundtrip() {
        roundtrip(ControlMessage::Register {
            name: "test-node".into(),
            public_key: "abc123".into(),
        });
    }

    #[test]
    fn register_ok_roundtrip() {
        roundtrip(ControlMessage::RegisterOk {
            node_id: "node-abc".into(),
            vpn_ip: "10.13.0.1".into(),
            session_token: "token123".into(),
        });
    }

    #[test]
    fn peer_list_roundtrip() {
        let peers = vec![
            PeerInfo {
                node_id: "n1".into(),
                name: "alpha".into(),
                vpn_ip: "10.13.0.1".into(),
                public_key: "pub1".into(),
                endpoint: "1.2.3.4:51820".into(),
                connected: true,
            },
            PeerInfo {
                node_id: "n2".into(),
                name: "beta".into(),
                vpn_ip: "10.13.0.2".into(),
                public_key: "pub2".into(),
                endpoint: String::new(),
                connected: false,
            },
        ];
        roundtrip(ControlMessage::PeerList { peers });
    }

    #[test]
    fn relay_assigned_with_target() {
        roundtrip(ControlMessage::RelayAssigned {
            relay_addr: "1.2.3.4:9091".into(),
            target_id: Some("node-xyz".into()),
        });
    }

    #[test]
    fn relay_assigned_without_target() {
        roundtrip(ControlMessage::RelayAssigned {
            relay_addr: "1.2.3.4:9091".into(),
            target_id: None,
        });
    }

    #[test]
    fn ping_pong_roundtrip() {
        roundtrip(ControlMessage::Ping {
            node_id: "n1".into(),
            session_token: "tok".into(),
        });
        roundtrip(ControlMessage::Pong);
    }

    #[test]
    fn json_field_names() {
        let msg = ControlMessage::Register {
            name: "x".into(),
            public_key: "y".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"public_key\""));
    }
}

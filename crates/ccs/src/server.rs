use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use tinyvpn_core::protocol::ControlMessage;
use crate::registry::{Registry, SharedRegistry};

/// Run the control server on the given address (QUIC transport)
pub async fn run(addr: &str, relay_addr: String) -> Result<()> {
    let registry: SharedRegistry = Arc::new(RwLock::new(Registry::new(relay_addr)?));

    // Periodic stale-node reaper
    let reap_registry = registry.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let mut reg = reap_registry.write().await;
            reg.reap_stale();
        }
    });

    let endpoint = tinyvpn_core::tls::create_server(addr)?;
    tracing::info!("CCS listening on {}", addr);

    loop {
        let incoming = endpoint.accept().await;
        let conn = match incoming {
            Some(incoming) => incoming.await?,
            None => break,
        };
        let peer_addr = conn.remote_address();
        let registry = registry.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(conn, peer_addr, registry).await {
                tracing::warn!("Connection from {} error: {}", peer_addr, e);
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    conn: quinn::Connection,
    peer_addr: std::net::SocketAddr,
    registry: SharedRegistry,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    tracing::info!("New connection from {}", peer_addr);

    // Accept a bidirectional stream from the client
    let (mut writer, recv) = conn.accept_bi().await?;
    let mut reader = BufReader::new(recv);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let msg: ControlMessage = serde_json::from_str(line.trim())?;
        tracing::debug!("Received from {}: {:?}", peer_addr, msg);

        let response = match msg {
            ControlMessage::Register { name, public_key } => {
                let mut reg = registry.write().await;
                let (node_id, vpn_ip, session_token) = reg.register(name, public_key)?;
                tracing::info!("Registered node {} → {}", node_id, vpn_ip);
                serde_json::to_string(&ControlMessage::RegisterOk {
                    node_id,
                    vpn_ip,
                    session_token,
                })?
            }

            ControlMessage::GetPeers { node_id, session_token } => {
                let reg = registry.read().await;
                if !reg.validate_session(&node_id, &session_token) {
                    serde_json::to_string(&ControlMessage::Pong)?
                } else {
                    let peers = reg.get_peers(Some(&node_id));
                    serde_json::to_string(&ControlMessage::PeerList { peers })?
                }
            }

            ControlMessage::UpdateEndpoint {
                node_id,
                session_token,
                public_addr,
            } => {
                let mut reg = registry.write().await;
                if reg.validate_session(&node_id, &session_token) {
                    reg.update_endpoint(&node_id, public_addr);
                }
                serde_json::to_string(&ControlMessage::Pong)?
            }

            ControlMessage::RequestRelay {
                node_id,
                session_token,
                target_id,
            } => {
                let reg = registry.read().await;
                if reg.validate_session(&node_id, &session_token) {
                    let relay_addr = reg.relay_addr().to_string();
                    serde_json::to_string(&ControlMessage::RelayAssigned {
                        relay_addr,
                        target_id: Some(target_id),
                    })?
                } else {
                    serde_json::to_string(&ControlMessage::Pong)?
                }
            }

            ControlMessage::Ping { node_id, session_token } => {
                let mut reg = registry.write().await;
                if reg.validate_session(&node_id, &session_token) {
                    reg.heartbeat(&node_id);
                }
                serde_json::to_string(&ControlMessage::Pong)?
            }

            _ => {
                tracing::warn!("Unhandled message type from {}", peer_addr);
                serde_json::to_string(&ControlMessage::Pong)?
            }
        };

        writer.write_all(response.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        line.clear();
    }

    tracing::info!("Connection closed from {}", peer_addr);
    Ok(())
}

use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use tinyvpn_core::protocol::ControlMessage;
use crate::registry::{Registry, SharedRegistry};

/// Run the control server on the given address
/// MVP: uses a simple TCP + JSON protocol (upgrade to QUIC later)
pub async fn run(addr: &str) -> Result<()> {
    let registry: SharedRegistry = Arc::new(RwLock::new(Registry::new()));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("CCS listening on {}", addr);

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let registry = registry.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer_addr, registry).await {
                tracing::warn!("Connection from {} error: {}", peer_addr, e);
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
    registry: SharedRegistry,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    tracing::info!("New connection from {}", peer_addr);

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let msg: ControlMessage = serde_json::from_str(line.trim())?;
        tracing::debug!("Received from {}: {:?}", peer_addr, msg);

        let response = match msg {
            ControlMessage::Register { name, public_key } => {
                let mut reg = registry.write().await;
                let (node_id, vpn_ip) = reg.register(name, public_key);
                tracing::info!("Registered node {} → {}", node_id, vpn_ip);
                serde_json::to_string(&ControlMessage::RegisterOk { node_id, vpn_ip })?
            }

            ControlMessage::GetPeers => {
                let reg = registry.read().await;
                let peers = reg.get_peers(None);
                serde_json::to_string(&ControlMessage::PeerList { peers })?
            }

            ControlMessage::UpdateEndpoint { public_addr } => {
                // We don't know the node_id in this simplified version
                // In production, authenticate the connection first
                tracing::info!("Endpoint update: {}", public_addr);
                serde_json::to_string(&ControlMessage::Pong)?
            }

            ControlMessage::Ping => {
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

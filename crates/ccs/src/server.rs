use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use tinyvpn_core::protocol::{ControlMessage, AclGroupEntry, AclRuleEntry};
use crate::registry::{Registry, SharedRegistry};
use crate::web;

pub async fn run(addr: &str, relay_addr: &str, web_addr: &str) -> Result<()> {
    let registry: SharedRegistry = Arc::new(RwLock::new(Registry::new(relay_addr.to_string())?));

    // Spawn web dashboard
    let web_registry = registry.clone();
    let web_addr = web_addr.to_string();
    tokio::spawn(async move {
        if let Err(e) = web::run(&web_addr, web_registry).await {
            tracing::warn!("Web server error: {}", e);
        }
    });

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

        tracing::info!("New connection from {}", peer_addr);

        tokio::spawn(async move {
            loop {
                let (send, recv) = match conn.accept_bi().await {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let registry = registry.clone();
                let peer_addr = peer_addr;

                tokio::spawn(async move {
                    if let Err(e) = handle_stream(send, recv, peer_addr, registry).await {
                        tracing::debug!("Stream from {} error: {}", peer_addr, e);
                    }
                });
            }
            tracing::info!("Connection closed from {}", peer_addr);
        });
    }

    Ok(())
}

async fn handle_stream(
    mut send: quinn::SendStream,
    recv: quinn::RecvStream,
    peer_addr: std::net::SocketAddr,
    registry: SharedRegistry,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut reader = BufReader::new(recv);
    let mut line = String::new();

    if reader.read_line(&mut line).await? == 0 {
        return Ok(());
    }

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

        ControlMessage::AclAddGroup { node_id, session_token, target_node_id, group_name } => {
            let reg = registry.read().await;
            if reg.validate_session(&node_id, &session_token) {
                drop(reg);
                let reg = registry.write().await;
                if let Err(e) = reg.add_group(&target_node_id, &group_name) {
                    tracing::warn!("ACL add group error: {}", e);
                }
            }
            serde_json::to_string(&ControlMessage::Pong)?
        }

        ControlMessage::AclRemoveGroup { node_id, session_token, target_node_id, group_name } => {
            let reg = registry.read().await;
            if reg.validate_session(&node_id, &session_token) {
                drop(reg);
                let reg = registry.write().await;
                if let Err(e) = reg.remove_group(&target_node_id, &group_name) {
                    tracing::warn!("ACL remove group error: {}", e);
                }
            }
            serde_json::to_string(&ControlMessage::Pong)?
        }

        ControlMessage::AclAddRule { node_id, session_token, from_group, to_group } => {
            let reg = registry.read().await;
            if reg.validate_session(&node_id, &session_token) {
                drop(reg);
                let reg = registry.write().await;
                if let Err(e) = reg.add_rule(&from_group, &to_group) {
                    tracing::warn!("ACL add rule error: {}", e);
                }
            }
            serde_json::to_string(&ControlMessage::Pong)?
        }

        ControlMessage::AclRemoveRule { node_id, session_token, from_group, to_group } => {
            let reg = registry.read().await;
            if reg.validate_session(&node_id, &session_token) {
                drop(reg);
                let reg = registry.write().await;
                if let Err(e) = reg.remove_rule(&from_group, &to_group) {
                    tracing::warn!("ACL remove rule error: {}", e);
                }
            }
            serde_json::to_string(&ControlMessage::Pong)?
        }

        ControlMessage::AclList { node_id, session_token } => {
            let reg = registry.read().await;
            if reg.validate_session(&node_id, &session_token) {
                let groups = reg.list_groups().unwrap_or_default()
                    .into_iter()
                    .map(|(n, g)| AclGroupEntry { node_id: n, group_name: g })
                    .collect();
                let rules = reg.list_rules().unwrap_or_default()
                    .into_iter()
                    .map(|(f, t)| AclRuleEntry { from_group: f, to_group: t })
                    .collect();
                serde_json::to_string(&ControlMessage::AclListResponse { groups, rules })?
            } else {
                serde_json::to_string(&ControlMessage::Pong)?
            }
        }

        _ => {
            tracing::warn!("Unhandled message type from {}", peer_addr);
            serde_json::to_string(&ControlMessage::Pong)?
        }
    };

    send.write_all(response.as_bytes()).await?;
    send.write_all(b"\n").await?;
    send.finish()?;

    Ok(())
}

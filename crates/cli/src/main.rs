//! TinyVPN CLI Client

use std::sync::Arc;
use anyhow::Result;
use clap::{Parser, Subcommand};
use tinyvpn_core::config::NodeConfig;
use tinyvpn_core::crypto::generate_keypair;
use tinyvpn_core::protocol::ControlMessage;
use tinyvpn_core::wg::WgInterface;

const DEFAULT_WG_INTERFACE: &str = "wg-tinyvpn";
const WG_LISTEN_PORT: u16 = 51820;

#[derive(Parser)]
#[command(name = "tinyvpn")]
#[command(about = "TinyVPN — minimal mesh VPN client")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Control server address
    #[arg(long, default_value = "127.0.0.1:9090")]
    ccs: String,

    /// WireGuard interface name
    #[arg(long, default_value = DEFAULT_WG_INTERFACE)]
    interface: String,

    /// WireGuard listen port
    #[arg(long, default_value_t = WG_LISTEN_PORT)]
    port: u16,
}

#[derive(Subcommand)]
enum Commands {
    Register {
        #[arg(short, long)]
        name: String,
    },
    Connect,
    Status,
    Disconnect,

    /// Forward a TCP port on the VPN IP to a local service
    Forward {
        /// Port to listen on the VPN interface
        port: u16,

        /// Target address to forward to (default: 127.0.0.1:<port>)
        #[arg(short, long)]
        target: Option<String>,
    },

    /// Manage ACL policies
    Acl {
        #[command(subcommand)]
        action: AclAction,
    },
}

#[derive(Subcommand)]
enum AclAction {
    /// Add a node to a group
    AddGroup {
        #[arg(long)]
        node: String,
        #[arg(long)]
        group: String,
    },
    /// Remove a node from a group
    RemoveGroup {
        #[arg(long)]
        node: String,
        #[arg(long)]
        group: String,
    },
    /// Add an ACL rule
    AddRule {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    /// Remove an ACL rule
    RemoveRule {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    /// List all ACL groups and rules
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tinyvpn=info".parse()?),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Register { name } => register(&cli.ccs, &name).await,
        Commands::Connect => connect(&cli.ccs, &cli.interface, cli.port).await,
        Commands::Status => status(&cli.ccs).await,
        Commands::Disconnect => disconnect(&cli.interface).await,
        Commands::Forward { port, target } => forward(port, target).await,
        Commands::Acl { action } => acl(&cli.ccs, action).await,
    }
}

async fn register(ccs_addr: &str, name: &str) -> Result<()> {
    if NodeConfig::exists() {
        let existing = NodeConfig::load()?;
        anyhow::bail!("Node already registered: {} ({})", existing.name, existing.node_id);
    }

    println!("Generating WireGuard keypair...");
    let (secret, public) = generate_keypair();
    let public_key_b64 = base64_encode(public.as_bytes());
    let private_key_b64 = base64_encode(secret.to_bytes().as_ref());

    println!("Registering with CCS at {}...", ccs_addr);
    let response = rpc(ccs_addr, &ControlMessage::Register {
        name: name.to_string(),
        public_key: public_key_b64.clone(),
    }).await?;

    match response {
        ControlMessage::RegisterOk { node_id, vpn_ip, session_token } => {
            let config = NodeConfig {
                node_id,
                name: name.to_string(),
                private_key: private_key_b64,
                vpn_ip,
                ccs_addr: ccs_addr.to_string(),
                session_token,
            };
            config.save()?;
            println!("Registered!");
            println!("   Node ID: {}", config.node_id);
            println!("   VPN IP:  {}", config.vpn_ip);
            println!("   Config saved to ~/.tinyvpn/config.json");
        }
        other => anyhow::bail!("Unexpected response: {:?}", other),
    }

    Ok(())
}

async fn connect(ccs_addr: &str, wg_interface: &str, wg_port: u16) -> Result<()> {
    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    println!("Connecting as {} ({})...", config.name, config.vpn_ip);

    let endpoint = tinyvpn_core::tls::create_client()?;
    let conn = Arc::new(endpoint.connect(ccs_addr.parse()?, "localhost")?.await?);

    println!("Fetching peer list...");
    let response = conn_rpc(&conn, &ControlMessage::GetPeers {
        node_id: config.node_id.clone(),
        session_token: config.session_token.clone(),
    }).await?;

    let peers = match response {
        ControlMessage::PeerList { peers } => peers,
        other => anyhow::bail!("Unexpected response: {:?}", other),
    };

    if peers.is_empty() {
        println!("   No other peers in the network yet.");
    } else {
        println!("   Found {} peer(s):", peers.len());
        for peer in &peers {
            println!("   - {} ({}) [{}]", peer.name, peer.vpn_ip,
                if peer.connected { "online" } else { "offline" });
        }
    }

    println!("Setting up WireGuard interface {}...", wg_interface);
    let wg = WgInterface::new(wg_interface);
    wg.setup(&config.vpn_ip, &config.private_key, wg_port)?;

    for peer in &peers {
        println!("   Requesting relay for {}...", peer.name);
        let resp = conn_rpc(&conn, &ControlMessage::RequestRelay {
            node_id: config.node_id.clone(),
            session_token: config.session_token.clone(),
            target_id: peer.node_id.clone(),
        }).await?;
        let relay_addr = match resp {
            ControlMessage::RelayAssigned { relay_addr, target_id } => {
                if let Some(tid) = target_id {
                    if let Err(e) = register_with_relay(&relay_addr, &config.node_id, &tid).await {
                        println!("   Warning: relay registration failed for {}: {}", peer.name, e);
                    }
                }
                let addr: std::net::SocketAddr = relay_addr.parse()?;
                if addr.ip().is_loopback() || addr.ip().is_unspecified() {
                    format!("127.0.0.1:{}", addr.port())
                } else {
                    relay_addr
                }
            }
            _ => {
                println!("   Failed to get relay for {}, skipping", peer.name);
                continue;
            }
        };
        println!("   Using relay {} for {}", relay_addr, peer.name);
        let allowed_ip = format!("{}/32", peer.vpn_ip);
        if let Err(e) = wg.add_peer(&peer.public_key, &allowed_ip, Some(&relay_addr)) {
            println!("   Failed to add WG peer {}: {}", peer.name, e);
        }
    }

    println!("TinyVPN is running. Press Ctrl+C to stop.");

    let node_id = config.node_id.clone();
    let session_token = config.session_token.clone();
    let hb_conn = conn.clone();

    let heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let msg = ControlMessage::Ping {
                node_id: node_id.clone(),
                session_token: session_token.clone(),
            };
            let _ = conn_rpc(&hb_conn, &msg).await;
        }
    });

    tokio::signal::ctrl_c().await?;
    println!("\nShutting down...");
    heartbeat.abort();
    let _ = wg.teardown();
    println!("Bye!");
    Ok(())
}

async fn status(ccs_addr: &str) -> Result<()> {
    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    println!("Node: {} ({})", config.name, config.node_id);
    println!("   VPN IP: {}", config.vpn_ip);

    let response = rpc(ccs_addr, &ControlMessage::GetPeers {
        node_id: config.node_id,
        session_token: config.session_token,
    }).await?;

    match response {
        ControlMessage::PeerList { peers } => {
            println!("   Peers: {} online", peers.len());
            for peer in &peers {
                println!("   - {} ({}) → {} [{}]", peer.name, peer.vpn_ip,
                    if peer.endpoint.is_empty() { "unknown" } else { &peer.endpoint },
                    if peer.connected { "online" } else { "offline" });
            }
        }
        other => println!("   Error: {:?}", other),
    }

    Ok(())
}

async fn forward(port: u16, target: Option<String>) -> Result<()> {
    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    let target_addr = target.unwrap_or_else(|| format!("127.0.0.1:{}", port));
    let listen_addr = format!("{}:{}", config.vpn_ip, port);

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    println!("Forwarding {} -> {}", listen_addr, target_addr);
    println!("Press Ctrl+C to stop.");

    loop {
        let (client, client_addr) = listener.accept().await?;
        let target = target_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = proxy_connection(client, &target).await {
                tracing::debug!("Proxy {} -> {} error: {}", client_addr, target, e);
            }
        });
    }
}

async fn proxy_connection(client: tokio::net::TcpStream, target: &str) -> Result<()> {
    let server = tokio::net::TcpStream::connect(target).await?;
    let (mut cr, mut cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);

    tokio::select! {
        r = tokio::io::copy(&mut cr, &mut sw) => r?,
        r = tokio::io::copy(&mut sr, &mut cw) => r?,
    };

    Ok(())
}

async fn acl(ccs_addr: &str, action: AclAction) -> Result<()> {
    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    match action {
        AclAction::AddGroup { node, group } => {
            let resp = rpc(ccs_addr, &ControlMessage::AclAddGroup {
                node_id: config.node_id,
                session_token: config.session_token,
                target_node_id: node,
                group_name: group,
            }).await?;
            match resp {
                ControlMessage::Pong => println!("Group added."),
                other => println!("Error: {:?}", other),
            }
        }
        AclAction::RemoveGroup { node, group } => {
            let resp = rpc(ccs_addr, &ControlMessage::AclRemoveGroup {
                node_id: config.node_id,
                session_token: config.session_token,
                target_node_id: node,
                group_name: group,
            }).await?;
            match resp {
                ControlMessage::Pong => println!("Group removed."),
                other => println!("Error: {:?}", other),
            }
        }
        AclAction::AddRule { from, to } => {
            let resp = rpc(ccs_addr, &ControlMessage::AclAddRule {
                node_id: config.node_id,
                session_token: config.session_token,
                from_group: from,
                to_group: to,
            }).await?;
            match resp {
                ControlMessage::Pong => println!("Rule added."),
                other => println!("Error: {:?}", other),
            }
        }
        AclAction::RemoveRule { from, to } => {
            let resp = rpc(ccs_addr, &ControlMessage::AclRemoveRule {
                node_id: config.node_id,
                session_token: config.session_token,
                from_group: from,
                to_group: to,
            }).await?;
            match resp {
                ControlMessage::Pong => println!("Rule removed."),
                other => println!("Error: {:?}", other),
            }
        }
        AclAction::List => {
            let resp = rpc(ccs_addr, &ControlMessage::AclList {
                node_id: config.node_id,
                session_token: config.session_token,
            }).await?;
            match resp {
                ControlMessage::AclListResponse { groups, rules } => {
                    if groups.is_empty() {
                        println!("No groups defined.");
                    } else {
                        println!("Groups:");
                        for g in &groups {
                            println!("   {} -> {}", g.node_id, g.group_name);
                        }
                    }
                    if rules.is_empty() {
                        println!("No rules defined (all traffic denied by default).");
                    } else {
                        println!("Rules:");
                        for r in &rules {
                            println!("   {} -> {} (allow)", r.from_group, r.to_group);
                        }
                    }
                }
                other => println!("Error: {:?}", other),
            }
        }
    }
    Ok(())
}

async fn disconnect(wg_interface: &str) -> Result<()> {
    let wg = WgInterface::new(wg_interface);
    match wg.teardown() {
        Ok(()) => println!("WireGuard interface torn down."),
        Err(e) => println!("Teardown failed (may already be down): {}", e),
    }
    Ok(())
}

/// One-shot RPC: create endpoint, connect, open stream, send, receive
async fn rpc(ccs_addr: &str, msg: &ControlMessage) -> Result<ControlMessage> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let endpoint = tinyvpn_core::tls::create_client()?;
    let conn = endpoint.connect(ccs_addr.parse()?, "localhost")?.await?;
    let (mut send, recv) = conn.open_bi().await?;
    let mut reader = BufReader::new(recv);

    let data = serde_json::to_string(msg)?;
    send.write_all(data.as_bytes()).await?;
    send.write_all(b"\n").await?;
    send.finish()?;

    let mut response = String::new();
    reader.read_line(&mut response).await?;
    let msg: ControlMessage = serde_json::from_str(response.trim())?;
    Ok(msg)
}

/// RPC on an existing connection: open stream, send, receive
async fn conn_rpc(conn: &quinn::Connection, msg: &ControlMessage) -> Result<ControlMessage> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let (mut send, recv) = conn.open_bi().await?;
    let mut reader = BufReader::new(recv);

    let data = serde_json::to_string(msg)?;
    send.write_all(data.as_bytes()).await?;
    send.write_all(b"\n").await?;
    send.finish()?;

    let mut response = String::new();
    reader.read_line(&mut response).await?;
    let msg: ControlMessage = serde_json::from_str(response.trim())?;
    Ok(msg)
}

async fn register_with_relay(relay_addr: &str, my_id: &str, peer_id: &str) -> Result<()> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    let relay: std::net::SocketAddr = relay_addr.parse()?;
    let msg = format!("REGISTER:{}:{}", my_id, peer_id);

    let relay_ip = relay.ip();
    let send_addr = if relay_ip.is_loopback() || relay_ip.is_unspecified() {
        format!("127.0.0.1:{}", relay.port())
    } else {
        relay_addr.to_string()
    };
    let send_addr: std::net::SocketAddr = send_addr.parse()?;
    socket.send_to(msg.as_bytes(), send_addr).await?;

    // Fallback to original relay address
    socket.send_to(msg.as_bytes(), relay).await?;

    let mut buf = [0u8; 64];
    match tokio::time::timeout(std::time::Duration::from_secs(5), socket.recv_from(&mut buf)).await {
        Ok(Ok((n, _))) => {
            let resp = String::from_utf8_lossy(&buf[..n]);
            if resp.starts_with("OK") { Ok(()) } else { anyhow::bail!("Relay rejected: {}", resp) }
        }
        Ok(Err(e)) => Err(e.into()),
        Err(_) => anyhow::bail!("Relay registration timed out"),
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        result.push(if chunk.len() > 1 { CHARS[((triple >> 6) & 0x3F) as usize] as char } else { '=' });
        result.push(if chunk.len() > 2 { CHARS[(triple & 0x3F) as usize] as char } else { '=' });
    }
    result
}

fn get_local_ip() -> anyhow::Result<std::net::IpAddr> {
    // Try to get the local IP by connecting to a public address
    use std::net::{UdpSocket, Ipv4Addr};
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:80")?;
    Ok(socket.local_addr()?.ip())
}

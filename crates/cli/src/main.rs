//! TinyVPN CLI Client

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
    /// Register this node with the control server
    Register {
        /// Human-readable name for this node
        #[arg(short, long)]
        name: String,
    },

    /// Connect to the mesh network
    Connect,

    /// Show network status and peer list
    Status,

    /// Disconnect from the mesh and tear down WireGuard
    Disconnect,
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
    }
}

async fn register(ccs_addr: &str, name: &str) -> Result<()> {
    if NodeConfig::exists() {
        let existing = NodeConfig::load()?;
        anyhow::bail!(
            "Node already registered: {} ({})",
            existing.name,
            existing.node_id
        );
    }

    println!("Generating WireGuard keypair...");
    let (secret, public) = generate_keypair();
    let public_key_b64 = base64_encode(public.as_bytes());
    let private_key_b64 = base64_encode(secret.to_bytes().as_ref());

    println!("Registering with CCS at {}...", ccs_addr);
    let response = send_to_ccs(
        ccs_addr,
        &ControlMessage::Register {
            name: name.to_string(),
            public_key: public_key_b64.clone(),
        },
    )
    .await?;

    match response {
        ControlMessage::RegisterOk {
            node_id,
            vpn_ip,
            session_token,
        } => {
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
    use tokio::io::AsyncBufReadExt;

    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    println!("Connecting as {} ({})...", config.name, config.vpn_ip);

    // Step 1: Establish persistent TCP to CCS
    let stream = tokio::net::TcpStream::connect(ccs_addr).await?;
    let (ccs_reader, mut ccs_writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(ccs_reader).lines();

    // Step 2: STUN discover public endpoint
    println!("Discovering public endpoint via STUN...");
    match tinyvpn_p2p::discover_public_endpoint().await {
        Ok(endpoint) => {
            println!("   Public endpoint: {}", endpoint);
            let msg = ControlMessage::UpdateEndpoint {
                node_id: config.node_id.clone(),
                session_token: config.session_token.clone(),
                public_addr: endpoint.to_string(),
            };
            send_on(&mut ccs_writer, &msg).await?;
            let _ = lines.next_line().await?;
        }
        Err(e) => {
            println!("   STUN failed: {} (will rely on relay)", e);
        }
    }

    // Step 3: Get peer list
    println!("Fetching peer list...");
    let msg = ControlMessage::GetPeers {
        node_id: config.node_id.clone(),
        session_token: config.session_token.clone(),
    };
    send_on(&mut ccs_writer, &msg).await?;

    let peer_line = lines.next_line().await?.ok_or_else(|| anyhow::anyhow!("CCS disconnected"))?;
    let peers: Vec<tinyvpn_core::protocol::PeerInfo> = match serde_json::from_str::<ControlMessage>(&peer_line)? {
        ControlMessage::PeerList { peers } => peers,
        other => anyhow::bail!("Unexpected response: {:?}", other),
    };

    if peers.is_empty() {
        println!("   No other peers in the network yet.");
    } else {
        println!("   Found {} peer(s):", peers.len());
        for peer in &peers {
            println!(
                "   - {} ({}) [{}]",
                peer.name,
                peer.vpn_ip,
                if peer.connected { "online" } else { "offline" }
            );
        }
    }

    // Step 4: Setup WireGuard interface
    println!("Setting up WireGuard interface {}...", wg_interface);
    let wg = WgInterface::new(wg_interface);
    wg.setup(&config.vpn_ip, &config.private_key, wg_port)?;

    // Step 5: Connect to each peer
    for peer in &peers {
        if peer.endpoint.is_empty() {
            println!("   Skipping {} (no public endpoint yet)", peer.name);
            continue;
        }

        println!("   Punching to {}...", peer.name);
        let puncher = tinyvpn_p2p::Puncher::new().await?;
        let endpoint: std::net::SocketAddr = peer.endpoint.parse()?;

        let peer_endpoint = match puncher.punch(endpoint, &config.node_id).await {
            Ok(()) => {
                println!("   Connected to {} (direct)", peer.name);
                peer.endpoint.clone()
            }
            Err(_) => {
                println!("   Punch failed, requesting relay for {}...", peer.name);
                let relay_msg = ControlMessage::RequestRelay {
                    node_id: config.node_id.clone(),
                    session_token: config.session_token.clone(),
                    target_id: peer.node_id.clone(),
                };
                send_on(&mut ccs_writer, &relay_msg).await?;
                let relay_line = lines.next_line().await?.ok_or_else(|| anyhow::anyhow!("CCS disconnected"))?;
                match serde_json::from_str::<ControlMessage>(&relay_line)? {
                    ControlMessage::RelayAssigned { relay_addr } => {
                        println!("   Using relay {} for {}", relay_addr, peer.name);
                        relay_addr
                    }
                    _ => {
                        println!("   Failed to get relay for {}, skipping", peer.name);
                        continue;
                    }
                }
            }
        };

        // Add WireGuard peer
        let allowed_ip = format!("{}/32", peer.vpn_ip);
        if let Err(e) = wg.add_peer(&peer.public_key, &allowed_ip, Some(&peer_endpoint)) {
            println!("   Failed to add WG peer {}: {}", peer.name, e);
        }
    }

    // Step 6: Heartbeat loop + wait for Ctrl+C
    println!("TinyVPN is running. Press Ctrl+C to stop.");

    let node_id = config.node_id.clone();
    let session_token = config.session_token.clone();

    let heartbeat = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            // Heartbeat requires a persistent connection which we can't easily
            // share across tasks with the current architecture.
            // For MVP, the connection stays alive and CCS reaps after 60s timeout.
            let _ = (node_id.clone(), session_token.clone());
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

    let response = send_to_ccs(
        ccs_addr,
        &ControlMessage::GetPeers {
            node_id: config.node_id,
            session_token: config.session_token,
        },
    )
    .await?;

    match response {
        ControlMessage::PeerList { peers } => {
            println!("   Peers: {} online", peers.len());
            for peer in &peers {
                println!(
                    "   - {} ({}) → {} [{}]",
                    peer.name,
                    peer.vpn_ip,
                    if peer.endpoint.is_empty() {
                        "unknown"
                    } else {
                        &peer.endpoint
                    },
                    if peer.connected { "online" } else { "offline" }
                );
            }
        }
        other => println!("   Error: {:?}", other),
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

/// Send a control message to CCS (one-shot TCP + newline-delimited JSON)
async fn send_to_ccs(ccs_addr: &str, msg: &ControlMessage) -> Result<ControlMessage> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stream = tokio::net::TcpStream::connect(ccs_addr).await?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let data = serde_json::to_string(msg)?;
    writer.write_all(data.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    let mut response = String::new();
    reader.read_line(&mut response).await?;
    let msg: ControlMessage = serde_json::from_str(response.trim())?;
    Ok(msg)
}

/// Send a message on an existing TCP stream
async fn send_on(
    writer: &mut tokio::io::WriteHalf<tokio::net::TcpStream>,
    msg: &ControlMessage,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let data = serde_json::to_string(msg)?;
    writer.write_all(data.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let chunks = data.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

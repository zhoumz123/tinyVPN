//! TinyVPN CLI Client

use anyhow::Result;
use clap::{Parser, Subcommand};
use tinyvpn_core::config::NodeConfig;
use tinyvpn_core::crypto::generate_keypair;
use tinyvpn_core::protocol::{ControlMessage, PeerInfo};

#[derive(Parser)]
#[command(name = "tinyvpn")]
#[command(about = "TinyVPN — minimal mesh VPN client")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Control server address
    #[arg(long, default_value = "127.0.0.1:9090")]
    ccs: String,
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
        Commands::Connect => connect(&cli.ccs).await,
        Commands::Status => status(&cli.ccs).await,
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

    println!("🔑 Generating WireGuard keypair...");
    let (secret, public) = generate_keypair();
    let public_key_b64 = base64_encode(public.as_bytes());
    let private_key_b64 = base64_encode(secret.to_bytes().as_ref());

    println!("📡 Registering with CCS at {}...", ccs_addr);
    let response = send_to_ccs(
        ccs_addr,
        &ControlMessage::Register {
            name: name.to_string(),
            public_key: public_key_b64.clone(),
        },
    )
    .await?;

    match response {
        ControlMessage::RegisterOk { node_id, vpn_ip } => {
            let config = NodeConfig {
                node_id,
                name: name.to_string(),
                private_key: private_key_b64,
                vpn_ip,
                ccs_addr: ccs_addr.to_string(),
            };
            config.save()?;

            println!("✅ Registered!");
            println!("   Node ID: {}", config.node_id);
            println!("   VPN IP:  {}", config.vpn_ip);
            println!("   Config saved to ~/.tinyvpn/config.json");
        }
        other => anyhow::bail!("Unexpected response: {:?}", other),
    }

    Ok(())
}

async fn connect(ccs_addr: &str) -> Result<()> {
    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    println!("🌐 Connecting as {} ({})...", config.name, config.vpn_ip);

    // Step 1: Discover public endpoint via STUN
    println!("🔍 Discovering public endpoint via STUN...");
    match tinyvpn_p2p::discover_public_endpoint().await {
        Ok(endpoint) => {
            println!("   Public endpoint: {}", endpoint);
            // Tell CCS our public address
            let _ = send_to_ccs(
                ccs_addr,
                &ControlMessage::UpdateEndpoint {
                    public_addr: endpoint.to_string(),
                },
            )
            .await;
        }
        Err(e) => {
            println!("   ⚠️  STUN failed: {} (will rely on relay)", e);
        }
    }

    // Step 2: Get peer list
    println!("📋 Fetching peer list...");
    let response = send_to_ccs(ccs_addr, &ControlMessage::GetPeers).await?;

    match response {
        ControlMessage::PeerList { peers } => {
            if peers.is_empty() {
                println!("   No other peers in the network yet.");
            } else {
                println!("   Found {} peer(s):", peers.len());
                for peer in &peers {
                    println!(
                        "   - {} ({}) → {} [{}]",
                        peer.name,
                        peer.vpn_ip,
                        if peer.endpoint.is_empty() {
                            "no endpoint"
                        } else {
                            &peer.endpoint
                        },
                        if peer.connected {
                            "online"
                        } else {
                            "offline"
                        }
                    );
                }

                // Step 3: Try to punch with each peer
                for peer in &peers {
                    if peer.endpoint.is_empty() {
                        println!(
                            "   ⏭️  Skipping {} (no public endpoint yet)",
                            peer.name
                        );
                        continue;
                    }

                    println!("   🥊 Punching to {}...", peer.name);
                    let puncher = tinyvpn_p2p::Puncher::new().await?;
                    let endpoint: std::net::SocketAddr = peer.endpoint.parse()?;
                    match puncher.punch(endpoint, &config.node_id).await {
                        Ok(()) => println!("   ✅ Connected to {}!", peer.name),
                        Err(e) => println!("   ❌ Failed: {}", e),
                    }
                }
            }
        }
        other => anyhow::bail!("Unexpected response: {:?}", other),
    }

    println!("🌐 TinyVPN is running. Press Ctrl+C to stop.");
    // Keep alive
    tokio::signal::ctrl_c().await?;
    println!("\n👋 Bye!");

    Ok(())
}

async fn status(ccs_addr: &str) -> Result<()> {
    let config = NodeConfig::load().map_err(|_| {
        anyhow::anyhow!("Not registered yet. Run: tinyvpn register --name <name>")
    })?;

    println!("📡 Node: {} ({})", config.name, config.node_id);
    println!("   VPN IP: {}", config.vpn_ip);

    let response = send_to_ccs(ccs_addr, &ControlMessage::GetPeers).await?;

    match response {
        ControlMessage::PeerList { peers } => {
            println!("   Peers: {} online", peers.len());
            for peer in &peers {
                println!(
                    "   - {} ({}) → {}",
                    peer.name,
                    peer.vpn_ip,
                    if peer.endpoint.is_empty() {
                        "unknown"
                    } else {
                        &peer.endpoint
                    }
                );
            }
        }
        other => println!("   Error: {:?}", other),
    }

    Ok(())
}

/// Send a control message to CCS (TCP + newline-delimited JSON)
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

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    // Simple base64 — in production use the base64 crate
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

//! TinyVPN Control Coordination Server (CCS)
//!
//! Responsibilities:
//! - Node registration & key exchange
//! - Peer discovery & topology management
//! - NAT endpoint tracking (from STUN results)
//! - Instructing nodes to attempt hole punching

mod server;
mod registry;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tinyvpn=info".parse()?))
        .init();

    let addr = std::env::var("CCS_ADDR")
        .unwrap_or_else(|_| tinyvpn_core::config::CCS_DEFAULT_ADDR.to_string());

    tracing::info!("🌐 TinyVPN CCS starting on {}", addr);
    server::run(&addr).await
}

//! TinyVPN Control Coordination Server (CCS)

mod server;
mod registry;
mod web;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tinyvpn=info".parse()?))
        .init();

    let addr = std::env::var("CCS_ADDR")
        .unwrap_or_else(|_| tinyvpn_core::config::CCS_DEFAULT_ADDR.to_string());

    let relay_addr = std::env::var("RELAY_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:9091".to_string());

    let web_addr = std::env::var("WEB_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    tracing::info!("TinyVPN CCS starting on {}", addr);
    tracing::info!("Relay address: {}", relay_addr);
    tracing::info!("Web dashboard: http://{}", web_addr);

    server::run(&addr, &relay_addr, &web_addr).await
}

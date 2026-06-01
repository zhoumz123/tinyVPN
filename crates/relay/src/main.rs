//! TinyVPN Relay Server

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tinyvpn=info".parse()?))
        .init();

    let addr = std::env::var("RELAY_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:9091".to_string());

    tracing::info!("TinyVPN Relay starting on {}", addr);

    let relay = tinyvpn_relay::Relay::bind(&addr).await?;
    relay.run().await
}

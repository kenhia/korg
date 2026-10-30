//! korg-gate binary. Configuration is environment-only; see `docs/gate.md`.

use std::net::SocketAddr;
use std::sync::Arc;

use korg_gate::{router, Config, Gate};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("KORG_GATE_LOG")
                .ok()
                .and_then(|f| EnvFilter::try_new(f).ok())
                .unwrap_or_else(|| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env()?;
    let listen = config.listen;
    tracing::info!(
        %listen,
        resource = %config.resource(),
        upstream = %config.upstream,
        "korg-gate starting"
    );
    let app = router(Arc::new(Gate::new(config)?));
    let listener = tokio::net::TcpListener::bind(listen).await?;
    // Connect info is the sign-in log's fallback when the proxy header is absent.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

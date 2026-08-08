//! korg-api binary: serve the REST API (and the web bundle when present).

use std::net::SocketAddr;
use std::sync::Arc;

use korg_api::{build_router, AppState};
use korg_core::config::KorgConfig;
use korg_core::connect;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("KORG_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
    let listen_addr: SocketAddr = std::env::var("KORG_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;

    let pool = connect(&database_url).await?;
    let config = KorgConfig::from_env()?;

    // The image store (sprint 056). Created at startup so a misconfigured or
    // unmounted volume is a log line now rather than a failed paste later —
    // but never fatal: korg's work tracking must not stop serving because the
    // screenshot volume is missing.
    let images = korg_api::img::store_from_env();
    match images.ensure_root() {
        Ok(()) => tracing::info!(root = %images.root().display(), "image store ready"),
        Err(e) => tracing::error!(
            root = %images.root().display(), error = %e,
            "image store is not writable — uploads will fail until this is fixed"
        ),
    }

    let state = AppState {
        pool: Arc::new(pool),
        config: Arc::new(config),
        images: Arc::new(images),
    };
    korg_api::img::spawn_sweeper(state.clone());
    let app = build_router(state);

    tracing::info!(%listen_addr, "korg-api listening");
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

//! `korg-mcp` — stdio MCP server backed directly by `korg-core`.
//!
//! Tracing goes to stderr; stdout is reserved for the MCP wire protocol.

use anyhow::Result;
use rmcp::transport::io::stdio;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use korg_mcp::tools::KorgServer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("KORG_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;

    let pool = korg_core::connect(&database_url).await?;
    let server = KorgServer::new(pool);

    let running = server
        .serve(stdio())
        .await
        .map_err(|e| anyhow::anyhow!("MCP server failed: {e}"))?;
    running
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("MCP transport closed with error: {e}"))?;
    Ok(())
}

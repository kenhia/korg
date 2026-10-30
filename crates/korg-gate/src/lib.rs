//! korg-gate: a read-only, OAuth-gated public front door to korg's MCP
//! endpoint, so Claude Duet (which reaches tools only through public HTTPS
//! connectors) can read korg. WI 4147; the decision to allow it is WI 4146.
//!
//! Three jobs in one small process:
//!
//! 1. **OAuth authorization server** (`oauth`): RFC 9728 protected-resource
//!    metadata, RFC 8414 server metadata, RFC 7591 dynamic client registration,
//!    an authorization endpoint behind one owner passphrase, and a token
//!    endpoint with S256 PKCE and rotating refresh tokens.
//! 2. **Resource server** (`proxy`): `/mcp` answers 401 without a live access
//!    token, and with one forwards to korg-api's `/mcp`.
//! 3. **Read-only filter** (`allowlist`): deny by default — only allowlisted
//!    JSON-RPC methods and read tools pass, and `tools/list` is filtered.
//!
//! korg itself is unchanged and still unauthenticated behind this; the gate is
//! the only thing between the public internet and korg, which is why it is
//! deny-by-default on every axis. See `docs/gate.md`.

pub mod allowlist;
pub mod config;
mod oauth;
mod proxy;
pub mod store;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

pub use config::Config;
use store::Store;

pub struct Gate {
    pub config: Config,
    pub store: Store,
    pub http: reqwest::Client,
}

pub type AppState = Arc<Gate>;

impl Gate {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let store = Store::open(&config.state_dir, config.access_ttl, config.refresh_ttl)?;
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(Self {
            config,
            store,
            http,
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(oauth::protected_resource),
        )
        // The path-inserted form (RFC 9728 §3.1) for clients that probe it
        // rather than following the 401's pointer.
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(oauth::protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth::server_metadata),
        )
        .route("/register", post(oauth::register))
        .route(
            "/authorize",
            get(oauth::authorize_page).post(oauth::authorize_submit),
        )
        .route("/token", post(oauth::token))
        .route(
            "/mcp",
            post(proxy::post_mcp)
                .get(proxy::get_mcp)
                .delete(proxy::delete_mcp),
        )
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state)
}

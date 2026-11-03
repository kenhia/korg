//! korg-gate: an OAuth-gated public front door to korg's MCP endpoint, so
//! Claude Duet (which reaches tools only through public HTTPS connectors) can
//! read korg. WI 4147; the decision to allow it is WI 4146. Sprint 094
//! (WI 4293) added a second door, klams, read-only behind the same
//! authorization server; see `door`. Sprint 095 (WI 4244) opened the korg door
//! to a ruled set of writes, `origin` stamped by the gate. Sprint 097
//! (WI 4324) did the same for klams: four knowledge writes, `author_id`
//! stripped by the gate.
//!
//! Three jobs in one small process:
//!
//! 1. **OAuth authorization server** (`oauth`): RFC 9728 protected-resource
//!    metadata, RFC 8414 server metadata, RFC 7591 dynamic client registration,
//!    an authorization endpoint behind one owner passphrase, and a token
//!    endpoint with S256 PKCE and rotating refresh tokens.
//! 2. **Resource server** (`proxy`): each door — `/mcp` for korg-api, and
//!    `/klams/mcp` for klams when it is configured (sprint 094) — answers 401
//!    without a live access token *for that door*, and with one forwards to
//!    its upstream.
//! 3. **Filter** (`allowlist`): deny by default — only allowlisted JSON-RPC
//!    methods and the door's ruled tools pass, and `tools/list` is filtered.
//!
//! korg itself is unchanged and still unauthenticated behind this; the gate is
//! the only thing between the public internet and korg, which is why it is
//! deny-by-default on every axis. See `docs/gate.md`.

pub mod allowlist;
pub mod config;
pub mod door;
mod oauth;
mod proxy;
pub mod store;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::{Extension, Router};

pub use config::Config;
pub use door::Door;
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
    let doors: Vec<Door> = state.config.doors().collect();
    let mut router = Router::new()
        // The root form stays korg's: it is what the korg connector's 401 has
        // always pointed at.
        .route(
            "/.well-known/oauth-protected-resource",
            get(oauth::protected_resource).layer(Extension(Door::Korg)),
        );
    for door in doors {
        router = router
            // The path-inserted form (RFC 9728 §3.1): for korg, for clients
            // that probe it rather than following the 401's pointer; for every
            // other door, the only form there is.
            .route(
                &door.metadata_path(),
                get(oauth::protected_resource).layer(Extension(door)),
            )
            .route(
                door.path(),
                post(proxy::post_mcp)
                    .get(proxy::get_mcp)
                    .delete(proxy::delete_mcp)
                    .layer(Extension(door)),
            );
    }
    router
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
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state)
}

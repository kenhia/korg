//! Configuration, read once from the environment at startup.
//!
//! On the Pi these arrive through the unit's `EnvironmentFile=/etc/korg-gate/env`
//! (see `deploy/korg-gate/`). Every variable is documented in `docs/gate.md`, and
//! the docs_drift suite holds that table to the `env::var` calls in this file.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context};
use url::Url;

/// The one redirect URI the gate will ever register or send a code to: the
/// callback shared by claude.ai on the web, Desktop and mobile. Claude Code's
/// loopback redirect is deliberately *not* accepted — the gate exists for Duet.
pub const CLAUDE_CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";

/// The identity stamped on every request forwarded upstream, so korg's audit
/// trail can tell Duet traffic apart from fleet agents.
pub const AGENT_HEADER: &str = "x-homelab-agent";
pub const AGENT_NAME: &str = "claude-duet";

#[derive(Clone, Debug)]
pub struct Config {
    /// Loopback address to bind; Funnel terminates TLS in front of it.
    pub listen: SocketAddr,
    /// The connector URL exactly as entered in claude.ai, e.g.
    /// `https://korg-gate.example.ts.net/mcp`. It is the OAuth `resource`.
    pub public_url: Url,
    /// korg-api's MCP endpoint, e.g. `https://kubsdb.example.ts.net:5674/mcp`.
    pub upstream: Url,
    /// The owner's sign-in passphrase.
    pub passphrase: String,
    /// Where registered clients and refresh tokens persist across restarts.
    pub state_dir: PathBuf,
    /// Header carrying the original client address (set by tailscaled for
    /// Funnel traffic). The *last* entry is used, because that is the one the
    /// trusted proxy appended; anything to its left is client-supplied.
    pub client_ip_header: String,
    pub access_ttl: Duration,
    pub refresh_ttl: Duration,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let listen = std::env::var("KORG_GATE_LISTEN")
            .unwrap_or_else(|_| "127.0.0.1:8475".to_string())
            .parse()
            .context("KORG_GATE_LISTEN is not a socket address")?;
        let public_url =
            std::env::var("KORG_GATE_PUBLIC_URL").context("KORG_GATE_PUBLIC_URL is required")?;
        let upstream =
            std::env::var("KORG_GATE_UPSTREAM").context("KORG_GATE_UPSTREAM is required")?;
        let passphrase = std::env::var("KORG_GATE_PASSPHRASE")
            .context("KORG_GATE_PASSPHRASE is required (it lives in krot, never in the repo)")?;
        let state_dir = std::env::var("KORG_GATE_STATE_DIR")
            .context("KORG_GATE_STATE_DIR is required")?
            .into();
        let client_ip_header = std::env::var("KORG_GATE_CLIENT_IP_HEADER")
            .unwrap_or_else(|_| "x-forwarded-for".to_string());
        let config = Self {
            listen,
            public_url: public_url.parse().context("KORG_GATE_PUBLIC_URL")?,
            upstream: upstream.parse().context("KORG_GATE_UPSTREAM")?,
            passphrase,
            state_dir,
            client_ip_header,
            access_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(60 * 60 * 24 * 30),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        // Plain http is tolerated only for a loopback host, which is what the
        // integration tests use; claude.ai itself only ever dials https.
        let local = matches!(
            self.public_url.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]")
        );
        if self.public_url.scheme() != "https" && !local {
            bail!("KORG_GATE_PUBLIC_URL must be https");
        }
        if self.public_url.path() != "/mcp" {
            bail!("KORG_GATE_PUBLIC_URL must end in /mcp — the gate serves MCP there");
        }
        if self.public_url.query().is_some() || self.public_url.fragment().is_some() {
            bail!("KORG_GATE_PUBLIC_URL must carry no query or fragment");
        }
        // A short passphrase is the whole attack surface (korg:4146#comment-4097
        // risk 1), so refuse to start with one rather than warn.
        if self.passphrase.chars().count() < 20 {
            bail!("KORG_GATE_PASSPHRASE must be at least 20 characters");
        }
        Ok(())
    }

    /// The authorization-server issuer: the public URL's origin. Its RFC 8414
    /// metadata is then at the root `/.well-known/oauth-authorization-server`,
    /// which every client probes, rather than at a path-inserted variant only
    /// some do.
    pub fn issuer(&self) -> String {
        self.public_url.origin().ascii_serialization()
    }

    /// The `resource` value: the connector URL, byte for byte.
    pub fn resource(&self) -> String {
        self.public_url.as_str().to_string()
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.issuer())
    }
}

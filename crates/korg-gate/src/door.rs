//! The gate's doors: one upstream MCP server each, behind one authorization
//! server (sprint 094, WI 4293).
//!
//! A door is a path on the gate's origin, an upstream it proxies to, an OAuth
//! resource and scope, and an explicit tool allowlist. Every token is bound to
//! the door it was issued for (RFC 8707), so a klams token opens nothing at
//! `/mcp` and a korg token nothing at `/klams/mcp`.
//!
//! The set is closed on purpose. Each door publishes another server's tools to
//! the internet, which is a decision (korg+ GP-17), not configuration — so a new
//! door is a new variant here, with its own allowlist and forcing test, and the
//! environment only says where its upstream lives.

use serde::{Deserialize, Serialize};

use crate::allowlist::{klams_tool_allowed, tool_allowed};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Door {
    /// korg-api, at `/mcp`. The default, because every grant persisted before
    /// sprint 094 was a korg grant and carries no `door` field.
    #[default]
    Korg,
    /// klams, at `/klams/mcp`.
    Klams,
}

impl Door {
    pub const ALL: [Door; 2] = [Door::Korg, Door::Klams];

    /// The upstream's name, as the sign-in page and refusals say it.
    pub fn name(self) -> &'static str {
        match self {
            Door::Korg => "korg",
            Door::Klams => "klams",
        }
    }

    /// Where the door is served on the gate's origin.
    pub fn path(self) -> &'static str {
        match self {
            Door::Korg => "/mcp",
            Door::Klams => "/klams/mcp",
        }
    }

    /// The one scope a token for this door carries.
    pub fn scope(self) -> &'static str {
        match self {
            Door::Korg => "korg:read",
            Door::Klams => "klams:read",
        }
    }

    /// Whether this door lets `tool` through: its ruled reads and writes
    /// (korg's since sprint 095, klams's since sprint 097).
    pub fn allows(self, tool: &str) -> bool {
        match self {
            Door::Korg => tool_allowed(tool),
            Door::Klams => klams_tool_allowed(tool),
        }
    }

    /// What a refusal says this door exposes.
    pub fn exposes(self) -> &'static str {
        match self {
            Door::Korg => "korg's reads and the ruled write set",
            Door::Klams => "klams's reads and the ruled write set",
        }
    }

    /// RFC 9728 `resource_name`, a display string. Each lost "(read-only)"
    /// when its door gained writes (korg in sprint 095, klams in 097).
    pub fn resource_name(self) -> &'static str {
        match self {
            Door::Korg => "korg",
            Door::Klams => "klams",
        }
    }

    /// What the sign-in page says the client wants, as HTML.
    pub fn sign_in_ask(self) -> &'static str {
        match self {
            Door::Korg => "access to korg's reads and the ruled write set",
            Door::Klams => "access to klams's reads and the ruled write set",
        }
    }

    /// The sign-in page's submit button.
    pub fn sign_in_button(self) -> &'static str {
        "Allow access"
    }

    /// RFC 9728 §3.1's path-inserted metadata URL for this door's resource.
    pub fn metadata_path(self) -> String {
        format!("/.well-known/oauth-protected-resource{}", self.path())
    }
}

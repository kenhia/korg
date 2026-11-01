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

use crate::allowlist::{KLAMS_READ_TOOLS, READ_TOOLS};

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

    /// The tools this door lets through, sorted.
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Door::Korg => READ_TOOLS,
            Door::Klams => KLAMS_READ_TOOLS,
        }
    }

    pub fn allows(self, tool: &str) -> bool {
        self.tools().binary_search(&tool).is_ok()
    }

    /// RFC 9728 §3.1's path-inserted metadata URL for this door's resource.
    pub fn metadata_path(self) -> String {
        format!("/.well-known/oauth-protected-resource{}", self.path())
    }
}

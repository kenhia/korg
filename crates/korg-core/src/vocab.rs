//! The domain vocabulary, validated app-side at every write boundary (WI #526).
//!
//! These sets used to live in four places at once: Rust consts, DB CHECK
//! constraints and enum casts, hand-written MCP JSON schemas, and TypeScript
//! const-tuples. The DB copies were doing the enforcing, which meant a typo'd
//! t-shirt size or card status came back as a 500 with raw Postgres text.
//! korg-core is now the authority; the DB constraints are a backstop.
//!
//! `wi_type` was entirely free text before this (D-2). The vocabulary is the
//! union of what the live corpus actually uses plus `chore`, so nothing
//! existing is invalidated.

use crate::error::RepoError;

/// Canonical work-item statuses (WI #285). Lifecycle: `open → resolved`
/// (implemented; may still need a user test / may not be PR'd) `→ done`
/// (agent satisfied — terminal but still visible in default lists)
/// `→ closed` (Ken only; hidden by default).
pub const WI_STATUSES: [&str; 4] = ["open", "resolved", "done", "closed"];

/// Work-item types (D-2). `brainstorm` is deliberate: half-formed ideas get
/// filed as work items rather than lost.
pub const WI_TYPES: [&str; 7] = [
    "task",
    "bug",
    "chore",
    "feature",
    "research",
    "tweak",
    "brainstorm",
];

/// T-shirt sizes; mirrors the `wi_tshirt` CHECK in migration 0001.
pub const WI_TSHIRTS: [&str; 7] = ["XS", "S", "M", "L", "XL", "Huge", "Unknown"];

/// Kanban columns; mirrors the `card_status` enum in migration 0001.
pub const CARD_STATUSES: [&str; 6] = ["Backlog", "Research", "OnDeck", "Active", "Done", "Cut"];

/// Reading-list dispositions; mirrors the `link_disposition` enum (0004).
pub const LINK_DISPOSITIONS: [&str; 5] = ["Unread", "Done", "Revisit", "Summarized", "VaultSaved"];

/// Sprint-proposal lifecycle; mirrors `sprint_proposal_status` (0008).
pub const PROPOSAL_STATUSES: [&str; 4] = ["proposed", "active", "done", "declined"];

/// Daily-report statuses; mirrors the `report.status` CHECK (0010).
pub const REPORT_STATUSES: [&str; 3] = ["ok", "attention", "problem"];

/// Project lifecycle statuses (WI #246). Default WI-page rail shows only
/// `active` + `maintenance` unless "show all" is on.
pub const PROJECT_STATUSES: [&str; 4] = ["active", "maintenance", "inactive", "archived"];

/// Reject a value outside its vocabulary with the full allowed set in the
/// message — the error doubles as the documentation an agent needs to retry.
pub fn validate(value: &str, allowed: &[&str], what: &str) -> Result<(), RepoError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(RepoError::InvalidInput(format!(
            "invalid {what} '{value}' — expected one of: {}",
            allowed.join(", ")
        )))
    }
}

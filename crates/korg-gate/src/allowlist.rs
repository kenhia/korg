//! What the gate lets through. Deny by default, on both axes.
//!
//! **Tools** are an explicit list of names, not a prefix rule. A rule like
//! "anything starting `get_`" would wave through a future tool that merely
//! *starts* like a read, and a tool reaching the public internet is a decision
//! somebody should make on purpose. The `allowlist` test suite reads korg's
//! real tool catalogue (`crates/korg-mcp/tests/tools_schema.json`) and fails
//! when a new read-shaped tool appears that this list has not ruled on — so the
//! decision is forced, not forgotten.
//!
//! klams's door (sprint 094) has its own list, and since klams has no naming
//! rule to lean on, its forcing test is stricter: every tool in klams's
//! committed catalogue (`tests/klams_tools.json`) must be in
//! [`KLAMS_READ_TOOLS`] or [`KLAMS_REFUSED_TOOLS`], so a tool klams adds fails
//! the build until somebody rules on it.
//!
//! **JSON-RPC methods** are the MCP lifecycle and the two tool methods. Nothing
//! else — resources, prompts, completion, logging, sampling responses — passes.

/// Read-only korg tools exposed through the gate (WI 4147: `get_*`, `list_*`,
/// `search`, `neighbors`, `work_item_flow`). Sorted; keep it that way.
pub const READ_TOOLS: &[&str] = &[
    "get_attachment",
    "get_board",
    "get_handoff",
    "get_item",
    "get_program",
    "get_project",
    "get_proposal",
    "get_report",
    "get_schedule",
    "get_work_item",
    "list_areas",
    "list_attachments",
    "list_awaiting",
    "list_cards",
    "list_comments",
    "list_links",
    "list_programs",
    "list_projects",
    "list_proposals",
    "list_report_sources",
    "list_reports",
    "list_schedules",
    "list_work_items",
    "neighbors",
    "search",
    "work_item_flow",
];

/// Read-only klams tools exposed through the gate's klams door (WI 4293).
/// Sorted; keep it that way.
pub const KLAMS_READ_TOOLS: &[&str] = &[
    "event_search",
    "memory_get",
    "memory_related",
    "memory_search",
];

/// klams tools ruled *out* of the klams door: every write, and identity
/// registration. Not consulted at runtime — anything not in
/// [`KLAMS_READ_TOOLS`] is refused — but the catalogue test needs it to tell a
/// tool somebody ruled on from one nobody has seen yet. Sorted.
pub const KLAMS_REFUSED_TOOLS: &[&str] = &[
    "dissent_propose",
    "memory_add",
    "memory_append_event",
    "memory_delete",
    "memory_supersede",
    "memory_update",
    "register_author",
];

/// JSON-RPC methods the gate forwards, on every door.
///
/// `server/discover` is MCP 2026-07-28's mandatory server method, which a
/// dual-era client probes before falling back to `initialize`. It is forwarded
/// and the upstream's own answer stands (WI 4243, option a), so the gate
/// tracks each upstream's MCP era instead of claiming one for it.
pub const METHODS: &[&str] = &[
    "initialize",
    "notifications/initialized",
    "ping",
    "server/discover",
    "tools/list",
    "tools/call",
];

pub fn tool_allowed(name: &str) -> bool {
    READ_TOOLS.binary_search(&name).is_ok()
}

pub fn method_allowed(method: &str) -> bool {
    METHODS.contains(&method)
}

/// The naming rule WI 4147 states. Used only by the catalogue test, to notice
/// a new read-shaped tool the explicit list has not ruled on.
pub fn looks_read_only(name: &str) -> bool {
    name.starts_with("get_")
        || name.starts_with("list_")
        || matches!(name, "search" | "neighbors" | "work_item_flow")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_tools_is_sorted_so_binary_search_is_sound() {
        let mut sorted = READ_TOOLS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, READ_TOOLS);
    }

    #[test]
    fn klams_lists_are_sorted_and_disjoint() {
        for list in [KLAMS_READ_TOOLS, KLAMS_REFUSED_TOOLS] {
            let mut sorted = list.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, list);
        }
        for name in KLAMS_READ_TOOLS {
            assert!(!KLAMS_REFUSED_TOOLS.contains(name), "{name} is in both");
        }
    }

    #[test]
    fn every_listed_tool_follows_the_read_naming_rule() {
        for name in READ_TOOLS {
            assert!(looks_read_only(name), "{name} is not read-shaped");
        }
    }

    #[test]
    fn unknown_methods_and_tools_are_refused() {
        assert!(!method_allowed("resources/read"));
        assert!(!method_allowed("tools/Call"));
        assert!(!tool_allowed("create_work_item"));
        assert!(!tool_allowed(""));
        assert!(!tool_allowed("get_work_item "));
    }
}

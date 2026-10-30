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

/// JSON-RPC methods the gate forwards.
pub const METHODS: &[&str] = &[
    "initialize",
    "notifications/initialized",
    "ping",
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

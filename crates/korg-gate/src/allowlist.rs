//! What the gate lets through. Deny by default, on both axes.
//!
//! **Tools** are an explicit list of names, not a prefix rule. A rule like
//! "anything starting `get_`" would wave through a future tool that merely
//! *starts* like a read, and a tool reaching the public internet is a decision
//! somebody should make on purpose. The `allowlist` test suite reads korg's
//! real tool catalogue (`crates/korg-mcp/tests/tools_schema.json`) and fails
//! when korg serves a tool that is in none of [`READ_TOOLS`], [`WRITE_TOOLS`]
//! and [`REFUSED_TOOLS`] — so the decision is forced, not forgotten.
//!
//! Sprint 095 (WI 4244) opened the korg door to a ruled set of writes, the
//! scheduling set Claude Duet needs. Every other korg write is refused by name.
//! Sprint 099 (WI 4322) added `update_schedule`, limited to a status change
//! ([`LIMITED_TOOLS`]).
//!
//! klams's door (sprint 094) has its own lists, and since klams has no naming
//! rule to lean on, its forcing test is stricter: every tool in klams's
//! committed catalogue (`tests/klams_tools.json`, a full-scope view) must be in
//! [`KLAMS_READ_TOOLS`], [`KLAMS_WRITE_TOOLS`] or [`KLAMS_REFUSED_TOOLS`], so a
//! tool klams adds fails the build until somebody rules on it. Sprint 097
//! (WI 4324) opened it to a ruled set of knowledge writes.
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
    "list_tagged_days",
    "list_work_items",
    "neighbors",
    "search",
    "work_item_flow",
];

/// korg writes exposed through the gate (WI 4244, ruled by Ken 2026-10-31):
/// the scheduling set — comment, raise Awaiting Ken, file and update work
/// items, save a link, wire edges, propose sprints and programs — plus
/// `update_schedule` (WI 4322, Ken 2026-11-03), narrowed by [`LIMITED_TOOLS`].
/// Widening it is a decision (korg+ GP-17), not an edit. Sorted; keep it that
/// way.
pub const WRITE_TOOLS: &[&str] = &[
    "add_comment",
    "create_link",
    "create_program",
    "create_work_item",
    "propose_sprint",
    "relate",
    "set_awaiting",
    "update_schedule",
    "update_work_item",
];

/// korg writes the gate forwards only with exactly these arguments, each
/// present and nothing else, and advertises in `tools/list` with its schema
/// cut down to them and the description given here. `update_schedule` is a
/// status change — retire (`done`), pause, or resume — and none of the
/// template, cadence, anchor, notes or archive fields (WI 4322).
pub const LIMITED_TOOLS: &[LimitedTool] = &[LimitedTool {
    name: "update_schedule",
    fields: &["node_id", "status"],
    description: "Change a schedule's status by node_id: `done` retires it, `paused` \
                  stops it surfacing without losing its anchor or history, `active` \
                  resumes it. Through korg-gate this tool takes `node_id` and \
                  `status` and nothing else; returns the updated schedule.",
}];

/// One [`LIMITED_TOOLS`] entry.
pub struct LimitedTool {
    pub name: &'static str,
    /// The argument names a call must carry, all of them and no others.
    pub fields: &'static [&'static str],
    /// The description the gate advertises in place of korg's.
    pub description: &'static str,
}

/// The [`LIMITED_TOOLS`] entry for `name`, if it has one.
pub fn limited(name: &str) -> Option<&'static LimitedTool> {
    LIMITED_TOOLS.iter().find(|t| t.name == name)
}

/// korg tools ruled *out* of the korg door (WI 4244's ruling): proposal and
/// program status changes, handoffs, comment and link edits and deletes,
/// `unrelate`, cards, areas, projects, reports, and creating or materializing
/// a schedule. Tagging and untagging Ken's calendar days (WI 4664) joined it
/// unruled-by-default: the proposal kept korg-gate out of that slice, and
/// opening them is a GP-17 decision. Not consulted at
/// runtime — anything in neither [`READ_TOOLS`] nor [`WRITE_TOOLS`] is refused
/// — but the catalogue test needs it to tell a tool somebody ruled on from one
/// nobody has seen yet. Sorted.
pub const REFUSED_TOOLS: &[&str] = &[
    "add_tagged_day",
    "create_area",
    "create_card",
    "create_handoff",
    "create_project",
    "create_report",
    "create_schedule",
    "delete_area",
    "delete_comment",
    "delete_link",
    "mark_link_read",
    "materialize_schedule",
    "remove_tagged_day",
    "review_report",
    "set_report_source",
    "unrelate",
    "update_area",
    "update_card",
    "update_comment",
    "update_handoff",
    "update_link",
    "update_program",
    "update_project",
    "update_proposal",
];

/// The korg write tools that carry korg's self-reported `origin` field
/// (#1879). The gate overwrites it with [`DUET_ORIGIN`] whatever the caller
/// sent, so every Duet-written comment and edge says who wrote it.
pub const ORIGIN_TOOLS: &[&str] = &["add_comment", "relate"];

/// The `origin` the gate stamps on [`ORIGIN_TOOLS`] calls.
pub const DUET_ORIGIN: &str = "claude-duet";

/// Read-only klams tools exposed through the gate's klams door (WI 4293).
/// `event_search` takes an `author_id` too, as a filter; reads are forwarded
/// untouched.
/// Sorted; keep it that way.
pub const KLAMS_READ_TOOLS: &[&str] = &[
    "event_search",
    "memory_get",
    "memory_related",
    "memory_search",
];

/// klams writes exposed through the klams door (WI 4324, ruled by Ken
/// 2026-11-02): add, supersede, update and delete knowledge. `memory_add` is
/// further narrowed to [`KLAMS_KNOWLEDGE_KIND`], and every one of these has
/// `author_id` stripped ([`KLAMS_AUTHOR_FIELD`]) so klams attributes the write
/// to the identity the gate's header binds. Widening it is a decision
/// (korg+ GP-17), not an edit. Sorted; keep it that way.
pub const KLAMS_WRITE_TOOLS: &[&str] = &[
    "memory_add",
    "memory_delete",
    "memory_supersede",
    "memory_update",
];

/// klams tools ruled *out* of the klams door: events and dissents (irreversible
/// or not Duet's to propose), identity registration, and every admin-scope tool
/// klams serves. Not consulted at runtime — anything in neither
/// [`KLAMS_READ_TOOLS`] nor [`KLAMS_WRITE_TOOLS`] is refused — but the
/// catalogue test needs it to tell a tool somebody ruled on from one nobody has
/// seen yet. Sorted.
pub const KLAMS_REFUSED_TOOLS: &[&str] = &[
    "dissent_propose",
    "memory_admin_hard_delete",
    "memory_admin_list_authors",
    "memory_admin_list_deleted",
    "memory_admin_merge_authors",
    "memory_admin_remove_author",
    "memory_admin_restore",
    "memory_append_event",
    "register_author",
];

/// The one `memory_add` kind the klams door forwards. Facts (user, task, env)
/// stay out: the gate refuses a `memory_add` whose `arguments.kind` is not
/// exactly this.
pub const KLAMS_KNOWLEDGE_KIND: &str = "knowledge";

/// The argument the gate strips from every [`KLAMS_WRITE_TOOLS`] call, so klams
/// fills in the author bound to the forwarded identity rather than one the
/// caller named. Strip, don't stamp: the gate does not know that author's id.
pub const KLAMS_AUTHOR_FIELD: &str = "author_id";

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

/// Whether the korg door lets `name` through: a ruled read or a ruled write.
pub fn tool_allowed(name: &str) -> bool {
    READ_TOOLS.binary_search(&name).is_ok() || WRITE_TOOLS.binary_search(&name).is_ok()
}

/// Whether the klams door lets `name` through: a ruled read or a ruled write.
pub fn klams_tool_allowed(name: &str) -> bool {
    KLAMS_READ_TOOLS.binary_search(&name).is_ok() || KLAMS_WRITE_TOOLS.binary_search(&name).is_ok()
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
    fn korg_lists_are_sorted_and_disjoint() {
        for list in [READ_TOOLS, WRITE_TOOLS, REFUSED_TOOLS] {
            let mut sorted = list.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, list);
        }
        for name in READ_TOOLS.iter().chain(WRITE_TOOLS) {
            assert!(
                !REFUSED_TOOLS.contains(name),
                "{name} is allowed and refused"
            );
        }
        for name in WRITE_TOOLS {
            assert!(!READ_TOOLS.contains(name), "{name} is a read and a write");
        }
        for name in ORIGIN_TOOLS {
            assert!(
                WRITE_TOOLS.contains(name),
                "{name} stamps origin but is not a write"
            );
        }
        for tool in LIMITED_TOOLS {
            assert!(
                WRITE_TOOLS.contains(&tool.name),
                "{} is limited but is not a write",
                tool.name
            );
            assert!(
                tool.fields.contains(&"node_id"),
                "{} cannot name its target",
                tool.name
            );
        }
    }

    #[test]
    fn klams_lists_are_sorted_and_disjoint() {
        for list in [KLAMS_READ_TOOLS, KLAMS_WRITE_TOOLS, KLAMS_REFUSED_TOOLS] {
            let mut sorted = list.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted, list);
        }
        for name in KLAMS_READ_TOOLS.iter().chain(KLAMS_WRITE_TOOLS) {
            assert!(
                !KLAMS_REFUSED_TOOLS.contains(name),
                "{name} is allowed and refused"
            );
        }
        for name in KLAMS_WRITE_TOOLS {
            assert!(
                !KLAMS_READ_TOOLS.contains(name),
                "{name} is a read and a write"
            );
        }
        assert!(klams_tool_allowed("memory_add"));
        assert!(klams_tool_allowed("memory_search"));
        assert!(!klams_tool_allowed("memory_append_event"));
        assert!(!klams_tool_allowed("memory_admin_restore"));
        assert!(!klams_tool_allowed("get_item"));
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
        assert!(tool_allowed("create_work_item"));
        assert!(!tool_allowed("update_proposal"));
        assert!(!tool_allowed("create_schedule"));
        assert!(!tool_allowed("materialize_schedule"));
        assert!(!tool_allowed("create_work_item "));
        assert!(!tool_allowed(""));
        assert!(!tool_allowed("get_work_item "));
    }
}

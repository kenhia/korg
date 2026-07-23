//! korg-mcp library surface (so the tool dispatch is unit-testable).
pub mod tools;

/// The `instructions` an MCP client receives from `initialize` — the only
/// overview of korg an agent gets before it has called anything.
///
/// It therefore has to name every category of tool. Two sprints' worth of
/// clients were told korg covers work items, cards, links, relationships,
/// topics and daily planning, and never learned that sprint proposals, reports,
/// projects or comments existed (F-12). `docs_drift::the_server_instructions_
/// name_every_category` now fails if a category goes unmentioned; the full
/// catalogue is `docs/api.md`.
pub fn server_instructions() -> &'static str {
    "korg MCP server — one typed-node data model over Postgres covering work items, cards, \
     comments, reading-list links, generalized relationships, topics, daily planning, \
     sprint proposals, reports, and projects and areas. \
     Mutations validate their target and return the updated entity; errors are isError \
     results carrying {message, code} where code is one of invalid_input, not_found, \
     conflict, internal. Paginated collection reads (list_work_items, list_cards, \
     list_links, list_topics, survey_work_items) return {items, total, limit, offset}; \
     the unpaginated ones (list_proposals, list_reports, list_projects, list_areas, \
     list_comments, list_daily_plan) return a bare array. All exclude \
     archived rows unless you ask for them. Writes take a project or area by name \
     (`project`/`area`) or by id (`project_id`/`area_id`), never both."
}

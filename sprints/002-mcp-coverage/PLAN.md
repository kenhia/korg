# Sprint 002 — close MCP coverage gaps vs korg-core / REST

## Problem

The korg MCP surface lags the capabilities that already exist in `korg-core`
(and are already exposed over REST). Agents can create things they cannot then
manage. Follows the same pattern as WI #92 (`update_work_item`).

## Gap analysis (core fn → MCP)

Done by diffing `pub async fn` in `korg-core` against the `tool(...)`
descriptors in `korg-mcp`. Gaps this sprint closes:

| Tool | Core fn | Why |
|------|---------|-----|
| `create_project` | `repo::create_project` | Could list but not create projects over MCP. |
| `create_area` | `repo::create_area` | Areas not creatable over MCP. |
| `list_areas` | `repo::list_areas` | Areas were wholly invisible over MCP, making `create_work_item.area_id` unusable. |
| `update_card` | `repo::update_card` | Cards could be created but never updated — same class of gap as WI #92. |
| `unrelate` | `repo::unrelate` | `relate` existed with no way to remove an edge. `neighbors` already returns `rel_id`. |
| `add_comment` | `repo::add_comment` | Comments wholly absent from MCP. |
| `list_comments` | `repo::list_comments` | " |
| `delete_comment` | `repo::delete_comment` | " |

Deferred (Low value, can be their own WIs): `set_node_tags`,
`set_link_disposition`, `recent_project`.

## Plan

1. Wire the 8 tools above into `korg-mcp` (descriptors, arg structs, dispatch).
   Reuse the `double_option` helper from sprint 001 for nullable card fields
   (`project_id`, `category`).
2. Tests: extend `crates/korg-mcp/tests/server.rs` — create/list project +
   area, update a card, relate→unrelate round-trip, comment add/list/delete.
   Bump tool-count assertions 17 → 25 in `korg-mcp` and `korg-api`.
3. Build + clippy + full workspace test.
4. Deploy to `kubsdb` (see DEPLOY.md), verify `tools/list` count + a live call.
5. Ship via /sprint-ship.

## Out of scope

No core changes — every capability already exists in `korg-core`. Pure MCP
surface exposure + deploy.

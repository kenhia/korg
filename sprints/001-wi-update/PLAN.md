# Sprint 001 — `update_work_item` MCP tool (WI #92)

## Problem

WI #92: agents using the korg MCP can only `create`, `get`, and `list` work
items. There is no way to change a work item's status (e.g. resolve #90/#91),
edit its fields, or archive it. Agents are blocked from closing out their own
work.

## Root cause

The capability already exists in the core layer:
`korg_core::repo::update_work_item(pool, wi_number, WorkItemPatch)` is fully
implemented ([crates/korg-core/src/repo.rs](../../crates/korg-core/src/repo.rs))
and exercised by the web/API surface (M5). It was simply never wired into the
MCP tool surface ([crates/korg-mcp/src/tools.rs](../../crates/korg-mcp/src/tools.rs)).

## Plan

1. **Add an `update_work_item` MCP tool** in `korg-mcp`:
   - Tool descriptor with a partial-update JSON schema (every field optional).
   - `UpdateWorkItemArgs` deserialize struct mapping onto `WorkItemPatch`.
   - Dispatch arm calling `repo::update_work_item`.
2. **Nullable-field semantics.** Core's `WorkItemPatch` uses `Option<Option<T>>`
   for fields that can be *cleared* vs *left untouched* (`details`, `sprint`,
   `area_id`, `parent`, `category`). Use a `double_option` deserializer so:
   - key absent → leave untouched
   - key present & `null` → clear
   - key present & value → set
3. **Test.** Extend `crates/korg-mcp/tests/server.rs`: create a WI, flip its
   status, edit a field, clear a nullable field, archive it; assert via
   `get_work_item`. Bump the tool-count assertion (16 → 17).
4. **Build + test** the workspace.
5. **Deploy to `kubsdb`** (see DEPLOY.md).

## Out of scope

No core changes — `update_work_item` already does what we need. This sprint is
purely MCP surface exposure + deploy.

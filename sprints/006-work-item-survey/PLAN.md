# Sprint 006 — slim, paginated work-item survey

## Context

Running `/refill-queue` for the first time hit exactly the failure mode
korg's own closed WI #85 was meant to prevent: an unscoped `list_work_items`
call returned ~198k characters and blew the MCP tool-output limit, forcing a
file-spill + manual Python pass to read it back. #85 added a `project`
filter, but there's still no way to get a cheap, cross-project view — every
row carries full `content`/`details` regardless of need, and there's no
pagination, so the payload only gets worse as the instance grows.

**Goal:** a new, separate tool/endpoint returning a slim projection
(`wi_number`, `node_id`, `project`, `title`, `wi_type`, `wi_status`,
`wi_tshirt` — no `content`/`details`) with real pagination (`limit`/
`offset`, `total` in the response). Ken's call: a new command, not a mode
bolted onto `list_work_items` — and pagination belongs on the new command
specifically; `list_work_items` itself is out of scope here.

Companion decisions from the same conversation (round 3 of the agent-planning
work):
- `refill-queue`'s `$ARGUMENTS` handling was too naive — it substituted
  free-form guidance directly into "scope to this project," producing
  garbled instructions on its first real invocation. Fixed to check against
  `list_projects` first and only scope if it's an exact match.
- `sprint-ship`'s korg close-out (added in sprint 004) only marked the
  *proposal* done, never the work items it covered — caught live when three
  shipped fixes sat as `open` and two never-implemented items got silently
  dropped from scope. Fixed: Step 6.5 now resolves every covered WI to
  `resolved` (skipping anything already `resolved`/`closed`) before marking
  the proposal `done`. The `resolved` → `closed` transition stays a human
  (or explicitly-trusted-agent) call by design.

## Plan

### 1. korg-core — [crates/korg-core/src/repo.rs](crates/korg-core/src/repo.rs)
- `WorkItemSummary` (the slim row) + `WorkItemSurvey` (`items`, `total`,
  `limit`, `offset`).
- `survey_work_items(pool, project, wi_status, archived, limit, offset)`:
  single query using `count(*) OVER()` for `total` so it reflects the full
  filtered count, not just the page — one round trip, not count-then-select.

### 2. MCP — [crates/korg-mcp/src/tools.rs](crates/korg-mcp/src/tools.rs)
- `survey_work_items(project?, wi_status?, archived?, limit? default 50 max
  500, offset? default 0)`. Server-side `clamp`/`max` on limit/offset as
  defense in depth beyond the JSON-schema bounds (a client can send anything
  over the wire).

### 3. REST — [crates/korg-api/src/lib.rs](crates/korg-api/src/lib.rs)
- `GET /api/work-items/survey`, registered before/alongside
  `/api/work-items/:wi_number` — axum's router disambiguates the static
  `survey` segment from the `:wi_number` capture regardless of registration
  order (matchit prioritizes static routes).

### 4. Skills
- `refill-queue` (both variants, kai + cleo): fixed the `$ARGUMENTS` project-
  matching bug, and switched Step 1 from the `list_work_items` workaround to
  the new `survey_work_items` (looping on `offset` until `total` is
  collected).
- `sprint-ship` (both variants, kai + cleo): Step 6.5 now resolves covered
  WIs before marking the proposal done (see Context above).

### 5. Tests
- **core**: exercised via the MCP/API tests below (no new pure-repo test —
  `survey_work_items` has no branching logic beyond what those cover).
- **mcp** ([crates/korg-mcp/tests/server.rs]): `survey_work_items_paginates_and_filters`
  — two-item pages don't overlap, `total` reflects the full filtered count,
  slim projection excludes `content`/`details`, status filter works. Bumped
  the tool-count assertions (28 → 29) here and in `korg-api/tests/mcp_http.rs`.
- **api** ([crates/korg-api/tests/api.rs]): `survey_work_items_end_to_end` —
  same pagination/slimness assertions over the REST route.

### 6. Deploy to kubsdb
- `docker build` → `docker save | ssh kubsdb docker load` →
  `docker compose up -d` from `/datastore/korg/` (the lesson from sprint
  004's deploy stuck — compose file is authoritative, not `docker inspect`).

## Verification
- `cargo test --workspace` green.
- Post-deploy: `GET /api/work-items/survey?limit=2` returns `{items, total,
  limit, offset}` with 2 items and the full count in `total`.

## Out of scope
- Pagination on `list_work_items` itself — Ken's call was the new command
  only.
- A web UI consumer of `survey_work_items` — this is an agent/MCP-facing
  tool; the Work Items page already has its own per-project view via
  `list_work_items`.
- Auto-resolving `sprint-ship`'s covered-WI status changes without a
  "shipped scope narrower than covers" escape hatch — documented as "remove
  the untouched items via `unrelate` before shipping" rather than building
  active tracking of per-item completion.

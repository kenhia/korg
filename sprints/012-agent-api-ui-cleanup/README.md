# Sprint 012 — agent API + UI cleanup

Proposal `korg:394`. Cleared the whole open korg backlog: WIs #392, #289, #291
(agent-facing API/MCP correctness) and #290, #313 (UI polish).

## Inline comments + comment_count (#392)

Agents calling `get_work_item` couldn't see comments, where resolution
rationale/decisions often live — a correctness gap, not just convenience.

- **`comment_count` everywhere.** Added a correlated-subquery column to
  `WORKITEM_SELECT` (so `get_work_item` / `list_work_items` /
  `list_work_items_by_project` all carry it) and to `survey_work_items`. It's
  the hint that says "this row has discussion — fetch it." Flows out through
  the REST responses and the TS `WorkItem` type.
- **Inlined on the single-item fetch.** New `get_work_item_detail` returns the
  row plus up to `WORKITEM_COMMENT_CAP` (10) comments and a
  `comments_truncated` flag; the true total is `comment_count`. The MCP
  `get_work_item` now uses it, so agents see the discussion without a second
  call. `list_comments` remains the escape hatch for the capped tail.
- Kept lists lean (count only) — inlining every row would multiply cost for
  little gain.

## Validation errors → 4xx (#289)

`ApiError` mapped every `anyhow` error to 500, so bad input (unknown status,
missing project) looked like a server fault to agents keying off status codes.
Added `korg_core::repo::RepoError { InvalidInput, NotFound }`; `validate_status`
and the project-lookup misses now return it, and korg-api's error layer
downcasts it to **400 / 404** (alongside the existing `PlanningError` mapping).
Everything else still 500.

## Move a work item between projects (#291)

`update_work_item` exposed `area_id` but not project, so an MCP move was
impossible and setting an area from another project silently corrupted state
(node.project_id vs workitem.area_id disagreeing).

- `WorkItemPatch` / the REST + MCP `update_work_item` gained `project_id`
  (`Some(Some(id))` move, `Some(None)` unassign, `None` leave).
- Area consistency is now enforced: an `area_id` must belong to the work
  item's (post-move) project or the update is rejected (`InvalidInput`); a
  move that leaves a now-foreign area behind clears it, unless a valid
  `area_id` is supplied in the same call.

## UI papercuts

- **#290** — the nav `active()` did a raw `startsWith`, so `/plan` also lit up
  on `/planning`. Now matches full path segments (exact, or `href + "/"`).
- **#313** — the work-items table shows a **Project** column when "All
  projects" is selected. Find-by-ID from the all-view now highlights in place
  (instead of switching to the hit's project), so that column stays visible
  and shows where the hit lives.
- **Width follow-up** (Ken, verifying #313): the Work Items route gets its own
  "roomy" layout — `max-w-[80%]` (≈10% gutters each side), roomier than the
  narrow default but not edge-to-edge like Cards/Link Up. The single-item
  detail view stays capped at `max-w-5xl` so long prose remains readable.

## Verified

- `cargo test -p korg-core -p korg-api -p korg-mcp` green, including new
  `sprint012` core tests (project move + area validation; capped-comment
  detail + comment_count) and the `#289` API status-code test.
- `pnpm check` / `lint` / `build` clean.
- (Deploy + live smoke on kubsdb recorded below.)

## Deployed

Deployed to `kubsdb` 2026-07-12 (pre-merge, for live UI verification) via the
`deploy-kubsdb` skill. Image `sha256:6d0b61a5…` (latest; superseding earlier
cuts `cc436cab…`/`4b6560fd…` as the Work Items width was tuned; prior
production image `44676f59…` kept for rollback); container healthy. Verified live: `#289` PATCH bad wi_status → 400,
PATCH unknown project → 404; `#392` REST `get_work_item` carries `comment_count`
and MCP `get_work_item(382)` inlines its single comment (the resolution note
that used to hide in `list_comments`). #291's cross-project move + area
validation are covered by `sprint012` core tests (not exercised against prod
data); #290/#313 are visual — pending Ken's browser check.

# 059 — Backlog flow endpoint: give korg a time-series read

Proposal `korg:1320`, work item #1318. Slice 1 of the **Make the backlog
legible** program (`korg:1322`); kfdc #1319 (the Rate of Fire board panel)
depends on this and cannot start until the endpoint exists.

## Goal

korg has no time-series read: none of the ~40 REST routes returns daily
aggregates, and the lean work-item row carries no timestamps, so backlog
trend is invisible without going around korg to Postgres — exactly the
workaround kfdc's architecture rule forbids.

Build `GET /api/work-items/flow?days=N` plus a matching MCP tool. One row per
day in the board's timezone (`KORG_TIMEZONE`):

    { day, added, closed, backlog, added_durable, closed_durable }

The durable split is the point, not a refinement: 77.6% of all closed work
items lived less than a day (sprint-internal churn), so a plain added/closed
chart shows ~1:1 forever and reproduces the "we clear 20 and open 21"
illusion instead of resolving it.

## Decisions inherited from the proposal (Ken, 2026-08-15)

- **Ship at `days=6` default, not 10.** The `transition` table only begins
  2026-08-08, so a 6-day window sits entirely inside the transition horizon —
  `closed` reads from transitions alone, correct by construction, no
  `node.updated` proxy, no fallback branch. Widening to 10 after 2026-08-18
  is a follow-up tweak, deliberately not this sprint.
- **`added` must come from `node.created` on every code path.** Creating a
  work item writes no transition row (verified on production: all 170
  then-open items had zero), so an `added` series derived from transitions is
  silently all zeros.
- **A caller passing `?days=60` gets the series clamped to the horizon, with
  the horizon named in the response.** Never zeros for uncovered days — the
  silent-wrong-answer failure mode this whole sprint is a response to.
- **Durable = lived (or has so far lived) more than 7 days.** Named constant,
  not a literal.
- **Exclude `node.archived` throughout**, so today's `backlog` equals what
  `list_work_items` reports as `total`.
- Serving this from `/api/board` was considered and rejected: the rollup is
  already large and every consumer pays for it; this series has one consumer
  and its own `days` parameter.

kfdc #1319 takes its series length from the response rather than hardcoding
6, so the later widening needs no board-side edit.

## Decisions

**D-1 — The horizon is a constant, not a data-derived guess.**
`FLOW_TRANSITION_HORIZON = 2026-08-08` in `repo.rs`, beside `FLOW_DAYS_DEFAULT
= 6` and `FLOW_DURABLE_AFTER_DAYS = 7`. Deriving the horizon from
`min(transition.at)` was considered and rejected: the first *recorded*
transition is not when the log *began* — a quiet week after the migration
would claim a later horizon than real, and an empty log (every fresh test
database) has no answer at all. The constant is exactly right for production
and safely correct for any database created after that date, whose log is
complete for its whole life. Widening the default to 10 after 2026-08-18 is
the one-line edit the proposal promised.

**D-2 — The timezone is threaded into `KorgServer`, not read from the
environment.** The MCP server held only a `PgPool`; `work_item_flow` is the
first tool that needs `KORG_TIMEZONE`. Rather than an `env::var` in the
dispatch path (a second read site for a variable `KorgConfig` already owns,
and a new row for the docs_drift env-table fence), `KorgServer::new` now takes
the timezone from whoever mounts it — korg-api passes
`config.timezone_name()`, the test helper pins `"UTC"`. Two construction
sites, both updated.

**D-3 — All bucketing happens in SQL, in one query.** `AT TIME ZONE $tz` on
Postgres's own clock, so REST and MCP cannot disagree and `generated` is the
same clock every timestamp came from. Backlog at a day's end is the current
status un-walked: the status at instant T is the `from_status` of the first
transition after T, else the current status. A close counts once per day it
happened; a reopened item rejoins the backlog on its reopen day.

**D-4 — `added_durable` is structurally zero at the launch default, and that
is documented rather than "fixed".** An item created inside a 6-day window
cannot have lived more than 7 days, so the column stays zero until the window
widens past the threshold. This is the honest consequence of Ken's two
decisions (6-day window, 7-day threshold) — durability of an arrival is only
knowable in retrospect. `closed_durable` has no such lag and carries the
drawdown signal immediately. Flagged in the tool description, `docs/api.md`,
and the response's self-describing `durable_after_days`, so kfdc #1319 renders
it knowingly.

## What shipped

- `crates/korg-core/src/repo.rs` — the three `FLOW_*` constants,
  `WorkItemFlowDay` + `WorkItemFlowSeries` (ts-rs exported), and
  `work_item_flow(pool, days, tz)`: one SQL pass — window clamped to the
  horizon via `LEAST`, `added`/`added_durable` from `node.created`,
  `closed`/`closed_durable` from dedup'd close transitions, `backlog`
  reconstructed per day-end from the log, archived excluded throughout.
- `crates/korg-core/src/ops.rs` — `ops::WorkItemFlow { days }` with a
  `schema::flow_days` helper advertising the default and the clamp.
- `crates/korg-api/src/lib.rs` — `GET /api/work-items/flow`, and
  `mcp_service`/`KorgServer::new` now carry the timezone.
- `crates/korg-mcp/src/tools.rs` — the `work_item_flow` tool (54th), catalogue
  entry + dispatch arm; `KorgServer.timezone`.
- Tests: `crates/korg-core/tests/sprint059.rs` (7 tests: window shape, horizon
  clamp + floor, added-from-created, churn-vs-durable close split, reopened
  item backlog reconstruction, backlog == `list_work_items` total invariant,
  timezone bucketing UTC vs Anchorage); MCP sweep + dispatch fixture; REST
  sweep. `just check` clean.
- Docs: `docs/api.md` catalogue row + `## The flow series (#1318)` section,
  `docs/usage.md` REST row, README count 53 → 54, regenerated
  `tools_schema.json` and `web/src/lib/generated/korg.ts`
  (`WorkItemFlowDay`/`WorkItemFlowSeries` are the types kfdc imports).

## Follow-ups

- After 2026-08-18: widen `FLOW_DAYS_DEFAULT` to 10 (one constant + fixture
  adjustments) — file as a tweak or fold into the live sprint, per the
  proposal.
- kfdc #1319 (Rate of Fire panel) is unblocked once this deploys.

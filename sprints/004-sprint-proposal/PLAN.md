# Sprint 004 — Agent Planning: `sprint_proposal` as a first-class korg feature

## Context

An ad-hoc "agent planning" workflow emerged organically: survey open work
items across projects, propose a handful of sprints, and capture them
somewhere durable. The first attempt at capturing this (2026-07-03, same day)
was a bootstrap using the existing primitives — a task work item per
proposed sprint in a new `Agent-Plan` project, `relate`d to the work items
each one covers. That worked, but cost 1 `create_work_item` + N `relate`
calls per proposal, and a "proposed sprint" isn't really a work item — it
has no natural status lifecycle (open/closed doesn't distinguish
proposed/active/done/declined), and ordering it meant overloading fields
that don't mean "drag order."

**Goal:** promote the concept to a first-class node kind, `sprint_proposal`,
following the same pattern that added `link` (0002) and `slot` (0003): widen
`node.kind`, add one detail table. Covered work items stay on the existing
generalized `relationship` table (label `covers`) — no new join table, that
mechanism already does exactly this job.

Decided in the brainstorm (round 2, comment on `korg` #113):
1. **Ordering** mirrors `card.rank` (NUMERIC, drag-orderable) — not a
   literal "priority" field. Ken drags in the web UI; agents reorder via
   `update_proposal`; both carry roughly equal weight. `pinned` proposals
   always sort first, then by rank.
2. **Closing a proposal** is done by whoever ships the sprint, not
   automatically by korg — `sprint-ship` gets an optional, non-blocking
   phase that marks a proposal `done` if a `korg:<node_id>` reference is
   available (via `$ARGUMENTS` or a `.korg-sprint-proposal` marker file) and
   the korg MCP tools are reachable. A companion `start-sprint` skill
   resolves a proposal into an active branch and leaves that marker.
3. **Refill-on-drain** is explicitly NOT korg's job — a skill Ken runs
   manually for now; later, a planned homelab-monitoring agent's job.
4. **Naming** stays `sprint_proposal` — consistent with how "sprint" is
   already used across contexts (day job + homelab), clearer for agent
   discussion than a homelab-only term.

## Plan

### 1. DB migration — `crates/korg-core/migrations/0008_sprint_proposal.sql` (new)
- Widen `node_kind_check` to add `'sprint_proposal'`.
- New enum `sprint_proposal_status` (`proposed|active|done|declined`).
- New table `sprint_proposal(node_id PK, title, summary, status DEFAULT
  'proposed', rank NUMERIC, pinned BOOLEAN DEFAULT FALSE)`.
- Indexes on `(pinned DESC, rank ASC)` (the list order) and `status`.

### 2. korg-core — [crates/korg-core/src/repo.rs](crates/korg-core/src/repo.rs)
- `NewProposal`, `ProposalRef`, `ProposalRow`, `ProposalPatch`.
- `create_proposal`: resolves `covers` wi_numbers to node ids *before* the
  transaction (mirrors `update_work_item`'s `parent` handling), then inserts
  the node + detail row + all `covers` edges in one transaction. Unresolved
  wi_numbers are silently dropped, matching the existing lenient-parent
  behavior rather than erroring the whole call.
- `list_proposals(status: Option<&str>)`: `ORDER BY pinned DESC, rank ASC`.
- `update_proposal`: same "only bind what's present" shape as `update_card`.

### 3. MCP — [crates/korg-mcp/src/tools.rs](crates/korg-mcp/src/tools.rs)
- `propose_sprint(title, summary, work_item_numbers[], project_id?, rank?,
  pinned?, category?, tags?)` — the bundled tool that was the whole point:
  collapses 1 create + N relates into one call.
- `list_proposals(status?)`, `update_proposal(node_id, ...)`.

### 4. REST — [crates/korg-api/src/lib.rs](crates/korg-api/src/lib.rs)
- `GET/POST /api/proposals`, `PATCH /api/proposals/:node_id`, mirroring the
  MCP surface (handler functions named like the REST concept, calling the
  `repo::` — same pattern as `update_card`/`create_work_item`).

### 5. Web
- New route `/planning` ([web/src/routes/planning/+page.svelte](web/src/routes/planning/+page.svelte)):
  two `svelte-dnd-action` zones (Pinned, Queue) using the same `midRank`
  drag-reorder helper as the Cards board; Active/Done/Declined as
  collapsible read-only lists. Each card shows covered-WI chips (via
  `neighbors` + a `list_work_items` join, not a new bulk endpoint), Pin
  toggle, Start/Decline/Done buttons, and a copy icon for
  `/start-sprint korg:<node_id>`.
- Nav entry "Planning" in [+layout.svelte](web/src/routes/+layout.svelte).
- `api.ts`: `Proposal` type, `PROPOSAL_STATUSES`, `proposals()`,
  `createProposal()`, `updateProposal()`.
- **Out of scope**: no create-proposal form in the UI. The primary creation
  path is agents via `propose_sprint`; the web UI is a review/action
  surface, not the entry point.

### 6. Skills (deployed to kai + cleo user-level; kubs0 — Ken's own pass)
- `sprint-ship` (both `.claude` and `.copilot` variants): optional Context
  Discovery step resolves a `korg:<node_id>` reference; optional Phase 6.5
  marks it `done` and removes the marker file. Non-blocking either way,
  mirroring the existing `kpidash` start/end-activity integration.
- New `start-sprint` skill (both variants): resolves a proposal + its
  covered work items, reports scope grouped by project (asks which repo to
  start with if it spans more than one — a proposal can cover multiple
  projects/repos, a branch can't), marks it `active`, branches, and leaves
  the `.korg-sprint-proposal` marker for `sprint-ship` to pick up later.

### 7. Migrate the bootstrap
- Re-create the 5 `Agent-Plan` proposals as real `sprint_proposal` nodes via
  `propose_sprint` (same covered work items, same relative order via rank).
- Archive the 5 original task WIs in the `Agent-Plan` project, with a
  comment cross-referencing the new node id, rather than deleting — keeps
  the history of how this feature got bootstrapped.

### 8. Tests
- **core** ([crates/korg-core/tests/proposals.rs], new): bundled
  create+covers in one transaction (including a dropped unresolved
  wi_number), pinned-first-then-rank ordering + status filter, patch-only-
  given-fields.
- **mcp** ([crates/korg-mcp/tests/server.rs]): `propose_sprint_and_lifecycle`
  — propose, list, pin, activate, filter by status. Also bumped the two
  hard-coded tool-count assertions (25 → 28) in `server.rs` and
  `korg-api/tests/mcp_http.rs`.
- **api** ([crates/korg-api/tests/api.rs]): `/api/proposals` end-to-end
  (create, list, PATCH lifecycle, status filter).

### 9. Deploy to kubsdb
- `docker build` → `docker save | ssh kubsdb docker load` → recreate via the
  `docker-compose.yml` already at `/datastore/korg/` on kubsdb (more
  authoritative than reconstructing the `docker run` invocation by hand —
  see Deploy notes for how that was learned the hard way).

## Verification
- `cargo test --workspace` green (16 passed across core/mcp/api, including 3
  new proposal tests + the updated tool-count assertions).
- `pnpm exec svelte-check` and `pnpm build`: 0 errors (one pre-existing,
  unrelated warning in `WorkItemForm.svelte`).
- Post-deploy: `GET /api/health` ok; `GET /api/proposals` reachable;
  `tools/list` over MCP reports 28 tools including `propose_sprint`,
  `list_proposals`, `update_proposal`; the 5 real proposals created and
  verified in pinned/rank order.

## Out of scope
- Auto-closing a proposal when all its covered WIs close (manual for now,
  per decision #2).
- The refill/drain trigger (explicitly not korg's job, per decision #3).
- A create-proposal form in the web UI (agents are the primary creation
  path).
- Folding `sprint-ship`/`start-sprint` into the `kagent-harness`
  single-source generator — that project is still early (2 assets built);
  these two skills stay hand-maintained in both `.claude` and `.copilot`
  locations for now.

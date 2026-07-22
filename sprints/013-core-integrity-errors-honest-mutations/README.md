# Sprint 013 — core integrity: one error type, honest mutations

Proposal `korg:553` — the first bundle (B1) of the 2026-07 deep-review cleanup
(`sprints/review/REVIEW.md`). Covers WIs #524–#529, closing findings F-02,
F-03, F-04, F-05, F-06, F-07, F-18, F-21, and decisions D-2, D-5, D-6, D-7.

The theme: korg-core becomes the authority on what a value may be and what a
mutation must prove before it writes, and both transports report the result
honestly. Nothing here changes what korg stores — it changes what korg
*admits* to.

## One error taxonomy (#524, F-02, D-5)

The crate had three regimes — `RepoError` (mapped to 4xx), `PlanningError`
(mapped precisely), and bare `anyhow::bail!` (always 500) — so invalid dates,
unknown reports, bad t-shirt sizes and FK violations reached agents as 500s
carrying raw Postgres text.

- New `korg_core::error`: `RepoError` gains `Conflict` and moves here;
  `ErrorCode { InvalidInput, NotFound, Conflict, Internal }` is the stable
  classification; the `ErrorClass` trait answers "what code is this?" for
  `RepoError`, `PlanningError`, and any `anyhow::Error` (which downcasts to
  them, defaulting to `Internal`).
- REST bodies are now `{"error": "...", "code": "not_found"}` — additive, so
  the existing client that reads `error` is unaffected. MCP `isError` content
  mirrors it as `{message, code}`.
- `topics.rs` gave up its `bail!`s; the api-layer parse/preset/rank errors
  became `ApiError::invalid`; `relate` and `add_comment` check their endpoints
  up front instead of letting the FK violation surface.

## Mutations validate, acknowledge, and return the entity (#525, F-03, F-04)

`PATCH /api/work-items/9999` answered `200 {"ok":true}`. Worse, `update_card`
and `update_proposal` bound only the node id, so `PATCH /api/cards/<work-item
node>` archived the *work item* and reported success — a slip that got likelier
the moment 0009 made `wi_number == node_id`.

- `require_kind` / `require_node` guard every mutation; a missing or wrong-kind
  target is `NotFound`, and nothing is written.
- Every create/update returns the row a read would return: `create_work_item`
  → `WorkItemRow` (superset of the old `{node_id, wi_number}`), `create_card` /
  `update_card` → `CardRow`, links → `LinkRow`, topics → `Topic`, proposals →
  `ProposalRow` (`create_proposal` returns it flattened alongside `covered`),
  projects → `ProjectRow`, daily-plan create/complete → `DailyPlanItem`,
  reorder → the day in its new order. Supporting reads `get_card`,
  `get_link`, `get_project`, `get_proposal`, `daily_plan::get_item` came along;
  they are core-only until B3 gives them surfaces.
- Deletes return `{"deleted": bool}` — `delete_comment` and `unrelate` no
  longer claim success over nothing. No `{"ok": true}` responses remain.
- Single-item reads 404 instead of answering `200 null` (D-6), on REST and as
  MCP `isError` not-found: work items, nodes, topics, reports.

## Vocabulary validated at the boundary (#526, F-05, F-06, D-2)

New `korg_core::vocab` holds every set in one place; the DB CHECKs and enum
casts are now a backstop rather than the UX.

- `wi_type` was free text. It is now `task, bug, chore, feature, research,
  tweak, brainstorm` — the union of the live corpus (`task`, `bug`, `feature`,
  `research`, `tweak`, `brainstorm`; no `chore`) plus `chore`, so no existing
  row is invalidated. Also an enum in the MCP schema.
- `wi_tshirt`, card `status`, link `disposition`, proposal `status`, report
  `status` validate app-side; the error names the whole allowed set.
- `create_work_item` enforces area ∈ project, which only `update_work_item`
  had been defending.
- An unresolvable `parent` is `InvalidInput`. It used to fall through to
  `Some(None)` and silently *clear* the parent — invisible corruption from the
  caller's side.

## Report re-runs replace their findings (#527, F-07, D-7)

`upsert_report` only ever added `finding` edges, so a corrected re-run left
retracted findings attached and `get_report` over-reported them. The edge set
is now replaced inside the upsert transaction (matching on "the other end",
since orientation stays id-canonical until B2). The tool description says so.

## `--reset` states its blast radius and refuses by default (#528, F-21)

`TRUNCATE node, project, area … CASCADE` destroys *every* node kind, while the
flag help, README, `docs/migration.md` and the justfile all said "work items /
cards / projects / areas". The import is one-shot and long finished, so a
`--reset` against a live database is almost by definition an accident.

Wording corrected in all four places; korg-migrate now connects to korg
*before* any snapshot work, prints the per-kind inventory it would destroy, and
refuses unless `KORG_RESET_CONFIRM=yes`.

## `project.updated` advances (#529, F-18)

Migration `0013_project_touch.sql` attaches the existing `touch_updated()`
trigger to `project`. Latent today (`ProjectRow` exposes no timestamps) but a
booby trap for anything that starts sorting projects by recency.

## Web

The client follows the server, not the other way round: `httpMaybe` returns
`null` on 404 for the reads that treat absence as normal (find-by-ID,
refresh-after-edit), everything else still throws, and the response types now
name the entities the API actually returns. No UI behavior changed.

## Documentation

`docs/usage.md` gains a **Response and error contract** section — the two rules
(mutations return the entity; errors are typed), the code→status table, and the
validated vocabularies. The normative `docs/api.md` is B5's.

## Verified

- `cargo test --workspace` — **43 passed, 0 failed**; `cargo clippy
  --workspace --all-targets` clean; `cargo fmt` clean.
- New `korg-api/tests/contract.rs` implements the review's §4.2 matrix
  (missing → 404 with a code, bad input → 400 with a code, cross-kind patch
  → 404 *and* provably no mutation, mutations return entities); new MCP probes
  in `korg-mcp/tests/server.rs` assert `isError` + `code`; new core tests cover
  finding replacement and the project touch trigger.
- `pnpm check` clean (319 files, 0 errors); `pnpm build` ok.
- Playwright e2e against a scratch Postgres: **26 passed**, 1 flaky
  (`daily-planner` topic-picker timing; passes on retry and on
  `--repeat-each=2 --retries=0`).
- Live REST probes of the §4.2 matrix against a scratch instance — every row
  green, including `PATCH /api/cards/<wi node>` → 404 `no card with node_id N`
  with the work item verifiably untouched.
- `--reset` guard exercised both ways: refused with the inventory in the
  message, then allowed with `KORG_RESET_CONFIRM=yes`.

## Out of scope (per the bundle)

Response envelopes and pagination, relationship direction/backfill, schema and
TS generation — B2/B3/B4. `list_cards` / `list_proposals` / `neighbors`
ordering tie-breakers (F-19) and the atomic `update_link` (F-20) stay with B3
even where this sprint touched the same functions.

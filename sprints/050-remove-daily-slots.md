# 050 — Remove the daily-slots feature, and the CI that proves it's gone

Proposal korg:1075 (slice 1 of program korg:1084). Covers #862 (S) and #965
(L). Branch `050-remove-daily-slots`, run 2026-08-08.

Ken's decision (2026-08-04, scope corrected 2026-08-07): the daily "slots"
planning feature comes out of korg whole — the daily plan, its History view,
the topics infrastructure that fed it, the data, and both the MCP and REST
surfaces. Topics are the part that *falls out*, not the target. Cards and
reading-list links are untouched; what they lose is only the ability to be
planned onto a day.

## #862 first — the CI that verifies the removal

The pairing was mechanical, not thematic: #965's own post-removal check was "a
manual grep across ~25 files", and #862's thesis is that a test that fails CI
beats a convention a session might skim. So `docs_drift` grew three assertion
classes before anything was removed:

- **Vocabularies in prose** — docs/usage.md's vocabulary bullet list and any
  backticked pipe-enumeration in `docs/` must match `vocab.rs` exactly (the
  class 816 caught as a four-value project status that had become two).
  `vocab::EXPORTED` went `pub` so the suite reads the one registry rather than
  a copy. usage.md gained the two bullets the coverage assert requires
  (program status, project category). Both fences were mutation-tested by
  reintroducing `maintenance`; both fired.
- **Migration headers** — every migration since 0018 must state its rollback
  semantics and carry the `DDL ONLY` / `DATA ONLY` / `DDL + DATA` declaration,
  checked against the statement kinds actually present in the SQL
  (statement-initial scan; `CREATE TEMP TABLE` scaffolding + its `DROP` and
  transient trigger toggles excluded — calibrated against 0018–0023).
- **Sprint deploy records** — with `.sprint-deploy` declared, every shipped
  record since 047 must carry `## Deployed` (or an explicit `## Not
  deployed`); the highest-numbered record is exempt because its deploy
  happens post-merge. The enforcement lands on the *next* sprint's PR, which
  is the point. This record's own Deployed section below is the first one the
  fence will demand.

The contract-assertion candidates (#862's second half: defaults documented,
`omitted` envelopes, overflow siblings) are deliberately deferred — they land
with or after the rank-3 read-contract sprint, per the WI.

## #965 — the removal

Migration `0024_remove_daily_slots.sql`, in 0012's own pattern (0012 built
daily planning on the grave of 0003's timebox slots; 0024 closes the line):
delete `daily_plan_item` nodes, then `topic` nodes (plan items first —
`source_node_id` is ON DELETE RESTRICT and a topic can be a source), letting
comments and relationship edges cascade so `neighbors` cannot dangle; drop
both tables; restate `node_kind_check` as the 0023 list minus the two kinds.

**The data, counted live on production 2026-08-08 (pre-deploy):**

- `daily_plan_item`: **6 rows**, 2026-07-11 → 2026-08-02, 4 completed
  (`daily_plan_history` 2025-01-01..2026-08-06 total=6; nothing planned on or
  after 2026-08-07). Real usage, but a month of light use, not an archive.
- `topic`: **1 row** — node 879, the archived agent-surface-037 probe, zero
  comments. Matches the WI's inventory exactly.

Seven nodes total. Rolling back across this deploy is NOT a clean re-tag — an
older image selects from the dropped tables — so the pre-deploy dump is the
rollback, per the 0024 header.

What came out, matching the WI's verified inventory:

- **korg-core**: `daily_plan.rs`, `topics.rs`; their `ops.rs` request types;
  `PlanningError` and its `error.rs` arms; the delete guard's plan-item
  count; the topic joins in `related_context` and `AWAITING_SELECT`; the
  `topic`/`daily_plan_item` preview arms.
- **korg-mcp**: the 13 tools and their dispatch arms — 57 → **44**. The
  server instructions no longer name the two categories, and
  `mcp_http.rs` now asserts the retired names never come back.
- **korg-api**: the 10 `/api/topics*` + `/api/daily-plan*` routes and
  handlers. `KorgServer` no longer carries the config at all.
- **web**: `/history`, `/topics`, `TopicPicker.svelte`, the Cards page's
  plan-this-week drop strip and per-tile drag handle, `lib/dates.ts` (no
  consumers left), the topic/daily-plan client calls, and three e2e specs.
- **docs**: tool catalogue, collection-shape table, disposal table, REST
  endpoint table, node-kind lists, `--reset` warnings, CLAUDE.md /
  copilot-instructions — changed together with the code, which the extended
  `docs_drift` now enforces rather than requests.
- `schema.rs` asserts `topic` and `daily_plan_item` stay gone the same way it
  already did for 0003's `slot`/`slot_template`.

### `/` is a slim Today page, not a deletion

Ken's call (2026-08-07, recorded on #965): keep `/` as a Today page carrying
the Awaiting Ken lane, the daily-report health pill, the active proposals
list (open) and the cards tray (collapsed) — everything the 590-line week
board's page did except the grid and its two drag-drop channels. "Which
sprint am I on" outlives the planner: proposal rows keep project chip, id,
status badge and preview, and link through to Planning. The page dropped out
of the layout's full-width set; it is lanes and trays now.

### Decisions taken in flight

- **`/plan` stays — deliberate deviation from the WI inventory.** #965 listed
  "3 UI routes: /plan, /history, /topics". `/plan` is the dependency-graph /
  frontier view over work items + `depends_on` edges, backed by
  `GET /api/projects/:name/plan` — which the same WI's non-goals protect and
  the plan-status skill consumes. It was inventoried on a name match, the
  exact trap the WI itself flags for `/daily-reports`. Route, nav entry and
  API are untouched.
- **`KORG_TIMEZONE` and the DST tests stay**, per the recommendation carried
  on the proposal and program: `LifecycleContext` slimmed to
  `KorgConfig::local_today`/`local_today_at`, currently uncalled but exactly
  what the rank-2 sprint (#581/#950) needs a week from now. The env var is
  still required at startup, so the deploy skill's claim stays true.

## Verification

- `just check` green end-to-end (fmt, generated-artefact drift, web checks,
  clippy `-D warnings`, full workspace tests — including the 4 new
  docs_drift fences against the post-removal tree).
- The full Playwright e2e suite (60 tests) run against a locally built
  bundle + korg-api over a throwaway Postgres: all pass, including the
  rewritten smoke test for the slim Today and the a11y sweep minus the two
  deleted routes.

## Follow-ups

- **Slice 2 (kyac:1083 / WI #1082) after this deploys**: drop
  `list_daily_plan` and `daily_plan_history` from kyac's korg allowlist and
  correct sprint 010's live-verified line. Sequenced by program korg:1084 —
  kyac edits *after* korg's removal is live, or kyac loses two working tools
  for nothing.
- #862's contract assertions land with the rank-3 read-contract sprint.
- korg #950's staleness card targets "the Today page", which still exists —
  that coupling held.

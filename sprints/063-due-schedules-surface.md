# 063 — Due schedules where eyes are

**Proposal:** korg:1393 (rank 5) · **Work items:** #1385, #1113
**Branch:** `063-due-schedules-surface` · Run 2026-08-17.

Sprint 051 built the schedules feature in #950's honour — the WI that exists
because kmon crashed and korg kept serving its last GREEN for eleven days — and
then computed due-ness only on `/schedules` and `list_schedules`, the two places
you go when you are *already* thinking about schedules. Schedule 1112 came due
and sat there for nine days, invisible. That is #950's own lesson ("a channel
nobody looks at is not a channel") reappearing inside the feature built to
answer it, which is why F-2 of the 044–059 re-review is the finding this sprint
lands rather than a nice-to-have.

The second item is the same feature's other first-contact bruise: the cadence
set could not spell the second real schedule anybody filed.

**Cross-project plan:** korg is mapped to `korg+/` in the cross-project-planning
index. The plan settled this sprint's one open design question before it
started — **GP-1** (agents curate korg; the board renders korg) makes the
surface a `due_schedules` block on `get_board`, so kfdc and korg-dash inherit it
and due-ness keeps a single definition; the Today pill is the human half; kfo
may cite due-ness in a Scouting Report but is never the surface. Nothing built
here contradicted a `GP-n`, so the plan needs no amendment.

## What shipped

### #1385 — `due_schedules` on the board, and a pill on Today

`BoardRollup` gained one field: what is due **right now**, soonest-due first.

Three decisions, each of which had a plausible alternative:

- **It is `list_schedules(due_only: true)`'s rows, not a board query.** Writing
  the predicate a second time would have agreed on the day it was written, and
  the clause it would most plausibly have lost is the outstanding-item one —
  which is exactly what stops the block nagging about a drill whose work item is
  already open and already in the queue. `the_board_and_the_list_cannot_disagree_about_due`
  drives a due, a paused and an already-fired schedule through both reads and
  asserts the same rows in the same order.
- **Uncapped**, like `sources` and for the same reason inverted from `reports`:
  the block is sorted by due-ness, so the row a cap drops is the one that has
  waited longest — the failure it exists to catch. It is bounded by construction
  anyway, since a schedule that fires leaves the block until its item finishes.
- **Full `ScheduleRow`s**, the D-4 move `programs` made with `ProgramRow`: a
  consumer renders a due row with the component it already has, and
  `preview_title` means no follow-up read to answer "due *what*?".

Today's pill sits beside the report-health pill, from the same one definition
(`scheduleDuePill`) so the colour cannot drift from `/schedules`. It renders
**only when something is due** — the Awaiting-lane rule; a pill reading "0 due"
every day is furniture, and furniture is what the eye learns to skip. Its
`title` lists the substituted preview titles and it links to `/schedules`, which
is the page that can act on them.

### #1113 — `fortnightly`

The cadence set mixes two families and only one member of the week family
existed. `weekly` and `fortnightly` are whole weeks, so they preserve the
*weekday* a schedule fires on; `monthly` and up advance by calendar month and
drift off it (2026-08-08 is a Saturday, 2026-09-08 is a Tuesday). "Every two
weeks, Saturday noon" was therefore expressible only as `weekly` with a note
saying it was interim — which is what schedule 1112 has been carrying since
2026-08-08.

Migration **0028** restates the `schedule_cadence_check`; `SCHEDULE_CADENCES` is
now ordered by interval; the generated TS and the MCP schema followed on their
own.

One thing the WI did not anticipate: **the interval table lived in SQL twice**,
the same `CASE` written out in `schedule_due_sql` and in `schedule_select`. The
doc comment above it said the fact lived "only in the `CASE` below and in
`schedule_select`" — i.e. it named its own duplication and moved on. Adding an
arm to one and not the other yields a NULL `due_at` on one read path and a
correct one on the other, so the sprint collapsed them into one
`cadence_interval_sql()` before adding the arm.

The question #1113 raised deliberately — whether the week family should be a
counted `every_n_weeks: 2` — is answered in `SCHEDULE_CADENCES`' own doc
comment rather than left to be re-derived: named values keep the vocabulary
closed and greppable, which `validate_status`, the CHECK constraint and two
docs_drift fences all rest on. Revisit at a **third** week interval, not a
second.

### The rider: a fence over the board's own table

`docs/api.md`'s board table is exactly the hand-maintained inventory the
`docs_drift` suite exists to fence, and it had already lost a field —
`sources`, shipped in sprint 051 and never added. So the sprint added
`the_board_table_documents_every_rollup_field`, which reads the serialized
rollup and asserts every field has a row, and put `sources` back. The test
builds an empty `BoardRollup` literal on purpose: a new field stops the test
file compiling, and the author of that error is one line away from the assertion
telling them where to document it.

## Verification

`just check` green (fmt, gen drift, svelte-check, eslint, clippy `-D warnings`,
full test suite). New coverage: 10 tests in `crates/korg-core/tests/sprint063.rs`,
one docs_drift fence, two Playwright specs
(`web/tests/e2e/today-due-schedules.spec.ts`).

Run against a real stack rather than inferred from the tests — scratch Postgres,
`korg-api`, the built bundle:

| Check | Result |
|---|---|
| `POST /api/schedules` with `cadence: "fortnightly"`, anchored 2026-07-20 | accepted; `due_at: 2026-08-03` (exactly 14 days), `due: true` |
| `GET /api/board` | `due_schedules` carries the fortnightly row; the not-yet-due quarterly drill is absent |
| Today in a browser | the amber "2 DUE" pill renders beside the date |
| Playwright `today-due-schedules` | 2 passed |

## Not in scope

- **kmon**, listed in #1385 as "optional after". The board and the pill are the
  daily-traffic surfaces; adding a third channel before the first two have been
  lived with is how you get channels nobody looks at.
- **Flipping schedule 1112 to `fortnightly`** — it cannot be done before the
  deploy, because production validates the cadence against the vocabulary in the
  image it is running. It is the first post-deploy action; see below.

## Deployed

**2026-08-17** · image `kubsdb.encke-wahoo.ts.net:5000/korg:4edb599c0328`
(digest `sha256:5db836a8…`) · rollback target `22b42507a3e6` (sprint 062),
confirmed present in the registry before building.

The deployed revision is `4edb599`, one commit past the merge: the ship was
blocked by a dirty tree carrying a concurrent session's correct fix to
`CLAUDE.md` (migrations live in `crates/korg-core/migrations/`, not
`crates/korg-migrate/`), which was committed on its own rather than stashed or
swept into this sprint's PR. Clean-tree-only is the rule the deploy skill exists
to hold (D-9), and this is what honouring it looks like.

Migration **0028** applied on startup: `migrations 27 → 28`, every row count
unchanged (1311 nodes, 944 work items, 250 proposals, 49 projects, 43 reports,
30 cards, 4 links — `node_max` and `seq_last` both still 1412). A DDL-only
migration that moved a row count would have been the interesting result.

### Verified live

| Item | Check | Result |
|---|---|---|
| deploy gate | container revision label vs the commit built | `4edb599c0328…` — match asserted inside the deploy, not after it |
| #1385 | `GET /api/board` | `due_schedules` present, carrying **#1112** — the schedule the WI was filed about, due since 2026-08-08 |
| #1385 | Today at `https://kubsdb.encke-wahoo.ts.net:5674/` | amber **"1 DUE"** pill renders beside the report-health pill |
| #1385 | `tools/list` over MCP | `get_board`'s description names `due_schedules` |
| #1113 | `tools/list` over MCP | `` `cadence` is once\|weekly\|fortnightly\|monthly\|quarterly\|yearly `` |
| #1113 | `PATCH /api/schedules/1112` → `fortnightly` | accepted; `due_at` moved 2026-08-08 → **2026-08-15** (anchor 2026-08-01 + 14 days), still due |
| — | `GET /plan` deep link | 200 |

Schedule 1112's notes were rewritten in the same call: the paragraph explaining
why it was filed `weekly` "because korg has no fortnightly cadence" is gone,
replaced by what it now is and when it changed. Leaving that note to rot was the
failure #1113 explicitly asked to avoid.

**Not smoke-tested live, deliberately:** the outstanding-item clause (a due
schedule leaving the board when its work item is created, and returning when
that item finishes). Exercising it means materialising a real drill in
production; it is covered by `a_materialized_schedule_leaves_the_board_and_returns_when_done`
in `sprint063.rs`, which drives the full round trip.

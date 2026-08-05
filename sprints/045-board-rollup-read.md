# 045 — Board rollup read: one call, the whole board

**Proposal:** korg:973 (rank 5, pinned) · **Work item:** #970 (feature, M)
**Branch:** `045-board-rollup-read`
**Context:** kfdc Phase 0, slice 4 — the last one. Handoff korg:974 ("kfdc
kickoff") carries the standing decisions; the load-bearing ones here are **#1:
agents curate korg, the board renders korg — no LLM in the render path** and
**#4: korg-dash consumes the same rollup read, it is not replaced.**

All three recorded prerequisites are closed before this starts, as #970's
`depends_on` edges require: proposal 825 `done` (sprint 042, membership on
rows), #968 and #969 `resolved` (sprint 044, the program layer).

## Goal

One read — REST and MCP — returning everything a board renders, so kfdc and
korg-dash issue **one** request instead of the crawl. The 2026-07-31 backlog
review measured the crawl the expensive way: 17 `get_proposal` calls plus a
script to answer one membership question.

The panels are named in `docs/design/kfdc-concept.html` (kai:`~/src/tools/kfdc`)
and this read is designed with them in hand:

| Panel | What it needs | Where it comes from |
|---|---|---|
| Fire Missions | active proposals + progress | `active` |
| On Deck | ranked queue + what the defaults hid + per-project depth | `queue`, `proposals_omitted`, `depth` |
| Operations | programs with ordered slices and per-slice status | `programs`, `programs_omitted` |
| Commander's Call | the awaiting-Ken lane | `awaiting` |
| Sensor Net | report health | `reports` |
| header statline | live / active / shipped / projects / awaiting | **derived** — see D-3 |
| Deconfliction | mined collisions | **not in this sprint** — D-8 |
| ticker | recent events | **not in this sprint** — D-7 |

## The measurement (production, 2026-08-05)

Taken before any decision, per the 043/044 precedent.

| | |
|---|---|
| nodes | 884 (workitem 672 · sprint_proposal 138 · report 26) |
| proposals | 138 total · **15 live** · 3 active |
| `covers` edges | 450 |
| programs / `includes` edges / awaiting rows | **0 / 0 / 0** — 044 shipped the layer empty |
| projects | 39 (**30 active**) |
| longest proposal `summary` | **499** chars |
| all 15 live summaries together | **5,950** chars (~1.5k tokens) |

Two of these decide things below. The 499 settles D-5: migration 0021 really did
move every over-cap summary into `notes`, so the ≤500 cap (#860) is true of the
whole corpus and not just of new writes. The three zeroes are why the Operations
and Commander's Call panels render empty today — that is 044 arriving empty by
design, not a gap in this read.

## Decisions

### D-1 — One tool, `get_board`, taking **no arguments**

`list_awaiting` set the precedent (#969: a lane "one read can list") and the
reasoning transfers exactly: every filter this read could offer is already
decided by *what the panels are*. A `project` filter would produce a board of
one repo, which is the opposite of the question — kfdc exists because Ken can no
longer hold ~30 projects in his head. Caps are fixed and documented rather than
parameterised; a consumer that needs more of one corpus calls that corpus's own
read, which is what the two-level read contract already says.

**`get_board`, not `list_board` / `board_rollup` / `get_overview`:**

- `get_*` is the convention for a focused read returning one composite object —
  `get_proposal`, `get_program`, `get_handoff` all do it.
- Deliberately **not** `list_*`. `docs_drift::the_archived_affordance_matches_the_instructions`
  fences every `list_` tool into the collection-read contract, and this is not a
  collection: it is one object holding several. Naming it `list_` would make it
  claim an `archived` filter and an envelope it does not have.
- "Board" is the word #970, proposal 973, the kfdc roadmap and the concept all
  already use for the thing this feeds.

REST: `GET /api/board`.

### D-2 — One aggregate pass over `covers`, serving two panels (the #976 answer)

This is the decision proposal 973's comment asked for by name. #976 recorded
that `wi_counts` is the dominant cost of both list reads (~6.8ms) and flagged
the risk: *aggregate creep multiplying this shape per panel* as a board's worth
of new aggregates lands on the same surface.

Two things answer it.

**The board never calls the list reads.** It does not pay `wi_counts` at all —
that aggregate belongs to `list_work_items`' paging envelope, and the board does
not page work items. So the measured 6.8ms is not multiplied by anything here;
it is simply absent.

**The progress aggregate runs once, not once per panel.** Fire Missions needs
per-proposal work-item status counts. Operations needs the *same* counts, one
level up, for each program slice — `get_program_detail` already computes them
per program. Writing those as two aggregates is exactly the creep #976 named. So:

```sql
WITH cov AS (
  SELECT r.left_id AS proposal_id, count(*) AS covered_count,
         count(*) FILTER (WHERE w.wi_status = 'open')     AS open, …
    FROM relationship r JOIN workitem w ON w.node_id = r.right_id
   WHERE r.relationship = 'covers'
   GROUP BY r.left_id )
```

one pass over 450 edges, and **both** panels read from it. The statement fetches
every proposal that is either live *or* a slice of a live program, so a `done`
slice in a finished program (the concept's `OP INFRA-CLEANUP`) is rolled up by
the same pass. Slices are then bucketed **in Rust** from a tiny second fetch of
the `includes` edges — not a second aggregate.

The board is a fixed number of statements over small corpora, independent of how
many panels render from it. Measured at production scale in "What shipped".

### D-3 — No counters block; every header figure is derivable

The tempting shape is a `counts: {proposals_live, active, projects, shipped,
awaiting}` object feeding the header statline. It is refused, because a counter
that can disagree with the list printed beside it is a bug generator — and it is
literally the aggregate creep D-2 just argued against.

Every statline figure comes out of what the board already returns:

| statline | derivation |
|---|---|
| live proposals | `active.len() + queue.len()` |
| active | `active.len()` |
| shipped | `proposals_omitted.done` |
| awaiting Ken | `awaiting.len()` |
| projects | `depth` — see below |

The one that was *not* derivable was projects: `planning_rollup` returns every
project regardless of status (39 rows), so counting them gives 39 where the
board wants 30. The fix is **one additive field**, `status`, on
`PlanningRollupRow` — not a parallel counter, and not a second per-project
projection. The Planning rail is unaffected (it reads the fields it already
read), and the board can now dim an inactive project as well as count active
ones.

### D-4 — Reuse the row types verbatim; invent exactly one

`AwaitingRow`, `ReportRow`, `PlanningRollupRow`, `ProgramRow`, `ProgramSlice`,
`ProposalOmitted` and `ProgramOmitted` all appear on the board **unchanged**. A
consumer that renders `get_program`'s slices renders the board's Operations
panel with the same code, and `omitted` means on the board exactly what it means
on `list_proposals` — the rows the defaults hid — rather than a second dialect
for the same idea.

The one new type is `BoardProposal`: a proposal plus its work-item status
rollup, because no existing row carries progress. It is not `ProposalLeanRow`
+ counts by inheritance; ts-rs flattening across a `sqlx::FromRow` boundary buys
nothing here and the field list is short.

### D-5 — `summary` is on the board's proposal rows; `notes` is not

Fire Missions renders the summary as the mission's subtitle — it is the panel,
not decoration. `list_proposals` went lean precisely because summaries were the
payload problem (#852: 110 rows, ~46k tokens, and the summaries were most of
it), so putting one back needs a reason and a number.

The number is the measurement above: #860 capped `summary` at 500 characters and
migration 0021 moved every over-cap summary to `notes`, so production's longest
is **499** and all fifteen live summaries together are **5,950 characters**. The
cap is what makes this affordable, and it did not exist when #852 measured.

`notes` is unbounded and stays behind `get_proposal`. So does the covered
work-item list — the board renders *progress*, and a consumer that wants the
items makes the focused read the two-level contract already points it at.

### D-6 — Active and queued proposals are two lists sharing one row type

Split into `active` and `queue` because they are two panels answering two
questions ("what is firing" vs "what is next"), which is the real consumer
boundary — and because splitting server-side means no consumer re-implements
which statuses count as live.

One row type because the same aggregate produces both, and a queued row's
progress then costs nothing. Both are ordered pinned-first, then rank, then
node_id — the same stable order `list_proposals` has had since F-19.

### D-7 — No synthesized event feed

#970 asks for "recent events/reports". Reports: yes, and `reports` carries the
newest few — `report_date` is the only date in korg that records *when something
happened* rather than when a row was last touched, and it is Sensor Net's source.

Events: no, and deliberately. korg has **no transition log**. The available
substitute is `node.updated`, which advances on any edit — so a "recently
shipped" list built from it would date a proposal by its last tag edit and
present that as a ship date. On a board whose entire promise is that it renders
korg deterministically, a plausible-looking wrong date is worse than an absent
panel. The concept's ticker entries ("queue re-review: 16 live, dupes folded")
are narrative anyway: they are curator output, and the kfdc roadmap puts the
ticker in Phase 3 alongside the curator.

Filed as a follow-up rather than guessed at.

### D-8 — Deconfliction stays out of v1

`depends_on` edges are real, typed and deterministic — #970's own edges to 825,
#968 and #969 are the worked example — and "is this queue row blocked" is the
single highest-value sentence On Deck could add. It is still out, for two
reasons: deciding *which* edges constitute a blocker (proposal-level, work-item
level, transitive closure, terminal-or-not) is a design question, not a query;
and it is not in #970's scope list, which names active proposals, the queue,
programs, awaiting and events. Scoping this sprint up is not the sprint's call.

Filed, so the board's next slice has a named home rather than living in this
paragraph.

### D-9 — No korg web page for the board

044 shipped minimal `/programs` pages because korg had no other way to see a
program. The board is different: **kfdc is its UI**, by the roadmap and by
standing decision #1. A second board inside korg's own web app would be a
duplicate that drifts. `PlanningRollupRow`'s new field is additive, so the
Planning rail needs no change either.

## Build order

1. **`repo.rs`** — `BoardProposal`, `BoardProgram`, `BoardRollup`;
   `board_rollup()`; `status` on `PlanningRollupRow`.
2. **`korg-mcp/tools.rs`** — `get_board` + dispatch.
3. **`korg-api`** — `GET /api/board`.
4. **`just gen`** — ts-rs bindings + MCP schema snapshot.
5. **Docs** — `docs/api.md` catalogue (a "Board" category) + a Board section;
   `docs/usage.md` REST table; README tool count 56 → 57; `docs_drift`'s
   `CATEGORIES`.
6. **Tests** — `crates/korg-core/tests/sprint045.rs`, plus transport sweeps.
7. **Measure** at production scale — the number #976 is owed.

## What shipped

### One tool, one route, no migration

`get_board` (57th tool) and `GET /api/board`, both onto `repo::board_rollup` —
**no schema change**, because everything the board needs already existed after
042/043/044. The only contract change anywhere is one additive field, `status`
on `PlanningRollupRow` (D-3), which the Planning rail ignores.

### The read, in statements

Six, fixed, independent of how many panels render from them:

| # | statement | what it feeds |
|---|---|---|
| 1 | `SELECT now()` | `generated` — Postgres's clock, the one every other timestamp came from |
| 2 | `list_programs` (reused) | Operations, and the ids statement 3 is keyed off |
| 3 | the `includes` edges of *those* programs | slice order |
| 4 | **the aggregate pass** — `cov` CTE + every live-or-sliced proposal | Fire Missions *and* Operations' rollups |
| 5–8 | `proposal_omitted`, `list_awaiting`, `planning_rollup`, `list_reports` (all reused) | On Deck, Commander's Call, depth, Sensor Net |

Keying statement 3 off statement 2's ids is what makes `programs` and `slices`
unable to disagree; there is a test for it. Statement 4 fetches proposals that
are live *or* a slice, which is how a `done` slice in a finished program gets
rolled up without re-entering the queue — the case two tests pin from both
directions (`done`, and `archived`).

### The measurement — the number #976 was owed

`EXPLAIN (ANALYZE)` per statement against production (kubsdb, 2026-08-05, the
corpus measured above):

| statement | plan | exec |
|---|---|---|
| board aggregate pass | 1.56 ms | **0.92 ms** |
| planning_rollup (depth) | 0.44 | 0.62 |
| list_awaiting | **3.44** | 0.21 |
| proposal_omitted | 0.09 | 0.16 |
| list_reports LIMIT 5 | 0.09 | 0.06 |
| *baseline* wi_counts | 0.12 | 0.35 |

Three findings, recorded on #976, which is resolved by them:

1. **The feared multiplication does not happen.** The board never calls the list
   reads, so `wi_counts` is not on its path at all; and the two panels that need
   progress share one pass over the 450 `covers` edges. The whole aggregate story
   is 0.92 ms.
2. **Aggregates are not what dominates.** The largest number on the board's
   critical path is `list_awaiting`'s **planning** time — 3.4 ms, 17× its own
   execution, because `AWAITING_SELECT` resolves a title across kinds through
   nine LEFT JOINs, replanned every call. Imperceptible, not worth optimising,
   and worth writing down: a future "the board feels slow" starts there, not at
   the counts.
3. **The measurement points differ and must not be compared.** #976's 6.8 ms was
   the REST call path; 0.35 ms above is the same statement's raw execution. The
   end-to-end figure for `/api/board` lands with the deploy.

The aggregate was also run for real against production and eyeballed: 15 live
proposals, 3 active, with honest progress tracks (820 reads 4 covered / 3 open /
1 closed).

### Tests

`crates/korg-core/tests/sprint045.rs`, 13 tests, plus a REST sweep
(`the_board_is_one_request_with_every_panel_on_it`), an MCP sweep
(`get_board_returns_every_panel_and_takes_no_arguments`), and the dispatch
fixture. The ones pinning a decision rather than a behaviour:

- `a_board_slice_is_identical_to_the_one_get_program_returns` — D-2 executable.
  It compares the board's slices against `get_program_detail`'s **by value**, so
  the moment anyone computes that rollup a second time it fails.
- `a_done_slice_is_rolled_up_but_never_enters_the_queue` and
  `an_archived_slice_stays_out_of_the_queue` — the trap in D-2's widened fetch,
  from both directions.
- `every_header_figure_is_derivable_and_agrees_with_its_own_read` — D-3, asserting
  each derivation against the corpus read that owns that number. Both transport
  sweeps additionally assert `board.counts` is **absent**.
- `the_board_carries_the_summary_and_leaves_notes_behind` — D-5, and it checks
  `notes` is absent from the *serialised* board, not merely from the struct.
- `progress_follows_the_edges_and_is_never_cached` — a `covers` edge written
  after the fact changes the next read's progress.

`just check` green. Tool count 56 → 57 in the README and both count assertions.

### Not in this sprint

- **The ticker** (D-7) — filed as **#977**. korg has no transition log; the
  reports half shipped.
- **Deconfliction** (D-8) — filed as **#978**. `depends_on` is deterministic and
  valuable; which edges constitute a blocker is a design question.
- **A korg web page** (D-9) — kfdc is the board's UI.
- **Per-project, per-status work-item counts** — korg-dash names them as a
  *maybe*; `depth` carries live/in-proposal/total per project, which is the
  "planning-queue depth" its README actually asks for. Revisit when the
  kdeskdash infographic contract is settled, not before.

## Coordination

Two-pane protocol (`sprints/planning/2026-08-korg-agent-surface.md` §7) is in
force: **explicit paths only on `git add`** — never `-A`, `.`, or `commit -a`.

---

## Log

- **2026-08-05** — Branched. Prerequisites verified closed (825 `done`, #968/#969
  `resolved`). Production measured; decisions D-1…D-9 recorded above.
- **2026-08-05** — Built in the order above. No migration needed — 042/043/044
  had already put every part in place. `just check` green.
- **2026-08-05** — Measured against production and answered #976 on its own
  comments (resolved). The interesting result is that the aggregate this sprint
  was warned about is not what dominates: `list_awaiting`'s query *planning* is.
  Follow-ups #977 (ticker / event log) and #978 (deconfliction) filed and linked
  to #970.

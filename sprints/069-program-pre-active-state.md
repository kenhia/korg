# 069 — `queued`, the program state before `active`

Proposal korg:1445. Covers #1424 (M).
Branch `069-program-pre-active-state`.

Slice 1 of program korg:1447, which spans korg and kfdc. Slice 2 is
kfdc:1446 (#1444 + #1196) and is blocked on this shipping — it cannot
render a state korg does not emit.

## Goal

Every program korg had ever created reported `active`, whether or not
anyone had started it, and kfdc's Operations panel put an ACTIVE callout on
it. #1424 has the screenshot. The cause is one line of 0023: the column was
defaulted to `'active'` and `create_program` never wrote it, so "active"
was not a claim anyone made — it was what a program got for existing.

That is the exact failure `holding` was added to prevent, one step later in
the lifecycle. So the fix is `holding`'s argument moved earlier.

## What shipped

**`queued` in the vocabulary.** `PROGRAM_STATUSES` is now
`queued → active ⇄ holding → done`, and `queued` is **live** — the
Operations panel is the read that most needs to show it, so filing it
terminal would have hidden it from the surface it exists for. The doc
comment carries the meaning at length, deliberately: kfdc #1196 exists
because a status value's documentation asserted a meaning korg did not
have, and kfdc switches its colour on these literals.

**`create_program` writes the column.** From `PROGRAM_INITIAL_STATUS`
rather than a literal, because a literal at the insert site is how the
authority (#526: `vocab` is it) and the DB default drifted in the first
place. Migration 0030 moves the default to match, widens the CHECK, and
backfills.

**The promotion.** `promote_queued_programs_over` in `repo/programs.rs`,
one statement so it rides the caller's transaction and writes its own
`transition` row. Called from every path that can start a slice.

**The UI.** A `queued` pill on both program pages, keyed by
`ProgramStatus`, so the next status added is a compile error rather than an
undefined class. Blue, and deliberately the coolest of the four: `queued`
means nobody is on this yet and must not read louder than `active`, which
is the whole bug.

**The docs.** `docs/api.md`'s program section and status table,
`docs/usage.md`'s vocabulary list and program-header sentence.
`docs_drift` gates all of it.

## Decisions

- **`queued`, not `created` or `ready`.** #1424 floated the other two.
  `ready` asserts a readiness korg cannot check — a program can be created
  over slices nobody has scoped, and a status whose docs claim more than
  the data supports is precisely the #1196 failure mode. `created` names
  how the row got here rather than where the work stands, and would be the
  only status in any korg vocabulary to do that. `queued` says the one
  thing that is true and checkable.

- **The state is a fact about the slices, and korg maintains it as one.**
  `queued` means *no slice of this program has started*. A stored status
  asserting a derived fact is only honest if every path that can falsify it
  updates it, so all three do:

  | path | how |
  |---|---|
  | `update_proposal` | a slice starts under an existing program |
  | `create_program` | the program is built over a slice already running (`slices_started`, so it is never *born* lying) |
  | `relate` | a running slice is attached afterwards — api.md's documented way to extend a program |

  The third is the one a rule living only in `update_proposal` would miss,
  and it is why `relate` is now transactional: an `includes` edge that
  commits without the promotion leaves the claim false.

- **"Started" is `active` or `done`, not "not `proposed`".** Named
  `PROPOSAL_STARTED_STATUSES` and registered in `PARTITIONS`, because three
  places ask the question and must not diverge. `declined` is a decision
  *not* to do the work, so a program whose slice was dropped was never
  started by it. `done` is in, because work that shipped was plainly begun
  whether or not anyone marked it `active` on the way through — a program
  cannot still be waiting to start on a slice that has finished. This is
  the case the two obvious shortcuts get wrong in opposite directions, and
  it has a test on both the promotion and the `create_program` path.

- **The promotion is one-directional, and only out of `queued`.**
  `holding` is a statement somebody made on purpose — "resting between
  slices" — and `done` is finished. Neither is korg's to overturn off a
  slice edit, and nothing demotes *into* `queued` behind you. Held by
  `promotion_leaves_every_other_program_status_alone`.

- **Backfill by migration, and it deliberately does not overrule anybody.**
  Two conditions: no slice started, **and** the program has never recorded
  a transition. `create_program` writes none, so zero rows means nobody has
  ever set this status by hand — and a program somebody explicitly moved to
  `active` is them saying it is running. It writes its own transition rows,
  which is what makes §3 both re-appliable and reversible: after the fact
  nothing else distinguishes a row the backfill moved from one a writer set
  to `queued` on purpose.

## The backfill moves zero rows in production, and that is the honest result

Rehearsed against the live corpus before writing this: 24 programs, 23
`done` and one `active` — korg:1447, this sprint's own program, whose
slice 1445 went active when `/start-sprint` marked it. §3 considers only
`active` rows and excludes any with a started slice, so it matches nothing.

Kept anyway. It is correct, it is documented, and it costs one statement on
a table with two dozen rows; a host whose corpus differs is exactly the case
it exists for. But the sprint record should say what actually happened
rather than imply a correction that did not occur.

**This has a consequence for slice 2.** The program notes ask kfdc to
verify against a real program in the new state rather than a hand-edited
fixture, and after this deploys there is no such program — every historical
one is `done` and 1447 is genuinely running. The next program created will
be `queued`. kfdc:1446 should create one (or wait for one) rather than
assume the corpus already holds an example.

## Cross-project plan

korg is routed to `korg+/PLAN.md`. Two decisions bear on this and neither
is contradicted:

- **GP-1** — agents curate korg, the board renders korg. This slice *is*
  GP-1: the state belongs in korg's vocabulary, which is why the program
  splits here at all.
- **GP-13** — a figure the consumer cannot compute is korg's to return, not
  the consumer's to reconstruct. Directly binds requirement 4. kfdc must
  switch on the emitted `queued` literal and must never derive
  "unstarted" from "all slices are `proposed`" — that reconstruction is
  what GP-13 forbids, and the fact that korg maintains the state on three
  write paths is what makes the emitted value worth more than the
  derivation anyway.

**No amendment needed.** The plan holds no decision about program status
vocabulary, and nothing here refines or contradicts one. Adding `queued` is
GP-13 being *followed*, not revised.

## Tests

`crates/korg-core/tests/sprint069.rs`, 17 tests: the vocabulary
classification, both birth paths (core and the DB default as backstop), all
three strand paths and their negatives, the `active`/`done`/`declined`
distinction, and the two reads kfdc consumes (`list_programs` default and
`board_rollup`'s `programs`).

Three existing tests moved, all for the same reason and none of them a
weakening:

- `sprint044::the_lane_is_oldest_first_and_carries_each_kind_status` —
  expected `active` for a fresh program. Its claim is that the awaiting
  lane resolves each kind's own status; the literal was the birth default.
- `sprint049::node_preview_renders_a_program` — same. Rewritten to assert
  the badge equals `created.row.status`, so it tests "status is a badge"
  instead of pinning whichever status a program is born in.
- `sprint053::the_ticker_covers_work_items_proposals_and_programs_newest_first`
  — now sees **two** program events, because the promotion is a status
  change and belongs in the ticker. It shares a transaction (and an `at`)
  with the proposal transition that caused it, so it doubles as the
  tie-break assertion that test's doc comment is about: the id orders them,
  and the effect sorts above its cause.

## Follow-ups

- **`holding` → `active` when the next slice starts.** The same rule one
  state over, and korg does not do it today either — a program resting
  between slices stays `holding` until somebody says otherwise. Deliberately
  out of scope: this sprint is the state *before* `active`, not a general
  program state machine, and `holding` is a deliberate statement in a way
  `queued` is not. Filed on #1424.

## Deployed 2026-08-19

- **Image** `kubsdb.encke-wahoo.ts.net:5000/korg:d5fc8a0181f7`
  (digest `sha256:5539160b0b29…`), from merge commit `d5fc8a0`.
- **Rollback target** `korg:62db604c9bb6` (sprint 068), confirmed present in
  the registry before building. Note that rollback is image-only: 0030 is a
  schema migration, so going back across it needs a restore, not a re-tag.
- **Backup** before deploy: `korg-20260819-032116.sql.gz`, 1,624,496 bytes —
  from this morning and larger than the night before.

Verified live:

| check | result |
|---|---|
| running revision == commit built | `d5fc8a0181f7fa8a…`, asserted in the deploy script |
| `post-deploy-check.sh --compare` | OK, exit 0 |
| migrations | 29 → **30** (0030 applied) |
| every row count vs baseline | **unchanged** — cards 30, links 4, projects 50, proposals 259, reports 44, work items 963, nodes 1345 |
| program statuses | **23 `done` + 1 `holding`, 91 `includes` edges — identical to the pre-deploy baseline** |
| `GET /api/programs?status=queued` | 200, `{"items":[],"omitted":{"archived":0,"done":23}}` |
| `GET /api/programs?status=ready` | `invalid_input` — *"expected one of: queued, active, holding, done"* |
| board `programs` block | serves 1447 as `holding` with 2 slices; `programs_omitted.done` 23 |
| deep links | `/`, `/plan`, `/programs`, `/programs/1447` all 200 |

**The migration's backfill moved zero rows in production, as rehearsed before
the sprint was written.** That is the result, not a shortfall: §3 only
considers `active` programs and excludes any with a started slice, and the
corpus holds none. The row counts above are the evidence it changed no data
while adding the state.

The refusal message is the most useful single check here — it is korg's own
vocabulary answering, so it proves the new value reached the deployed binary
rather than just the database.

# 053 — The board's two deferred panels: a real event log and deterministic deconfliction

Proposal korg:1076 (rank 3). Covers #977 (M), #978 (M) and #1101 (XS, a
ride-along). Branch `053-board-events-and-deconfliction`, run 2026-08-08.

Slice 1 of program 1078 ("Sequencing on the board"). Both real items change one
contract — `get_board` — which is the whole reason they shipped together: kfdc
absorbs one shape change instead of two.

## What shipped

- **Migration 0026** — an append-only `transition` table. korg's first record of
  *when a thing changed state*.
- **`get_board.events`** — the ticker. Newest 20 transitions, resolved enough to
  render a line each.
- **`get_board.blocked`** — Deconfliction's deterministic half. Every unmet
  `depends_on` holding up a live row, with the blocking ref named.
- **`vocab::WI_FINISHED_STATUSES`** — a second partition over the work-item
  vocabulary, and the thing that stops #978 being subtly wrong.
- **`scripts/post-deploy-check.sh`** — #1101's fix, plus a fence so it cannot
  rot the same way again.

## #1101 first, because it makes this sprint's own deploy honest

Two lines and an open question. `/api/reports` defaults to `limit=30`, so the
deploy gate's report count had been pinned at exactly 30 for some time — and a
count that cannot fall cannot detect the loss the compare exists for. The 052
deploy filed reports from two new sources and still printed `reports: 30 -> 30
(0)`.

**The open question the WI raised is answered: `/api/proposals` and
`/api/projects` are fine, and now for a written-down reason.** Both are
unconditional `fetch_all` — `repo::list_proposals` and `repo::list_projects`
take no limit at all. Only reports pages. So this was one endpoint's bug, not a
latent class across all three.

The fix asks for a ceiling (`ARRAY_CEILING = 100000`) **and asserts the answer
came back under it**. That second half is the point: asking for a big number
fixes today, and the assertion is what stops the fix from rotting into the same
silent saturation the day the corpus outgrows it. This is the second
`post-deploy-check.sh` defect in two sprints (#1086 was the retired
`/api/topics` count) and the proposal's comment said a third would justify a
fence rather than a third doc note — the assertion *is* that fence, arriving one
defect early, because it costs a line.

Worth naming the difference from #1086: that one was a hard failure (`curl -fsS`
on a removed route under `set -euo pipefail`). This one exits 0 and reports a
wrong number, which is quieter and worse — nobody has a reason to look.

## #977 — what counts as an event

The sprint's one real scoping question, and #977 explicitly refused to answer it
in advance: does a created proposal, a landed report, or a written handoff count
as an event, alongside status transitions?

**D-1. korg logs the events that have no honest timestamp anywhere else.**

That is the rule, and it settles the question by derivation rather than by
taste. A node's creation already has `node.created`. A report landing already
has `report_date` — the one date in korg that records when something *happened*.
A handoff has its own `node.created`. A status change had **nothing**, and it is
the one that says "shipped". So the log covers transitions, the ticker renders
creations and reports from columns korg already had, and the curator's narrative
entries ("queue re-review: 16 live, dupes folded") stay where kfdc's roadmap put
them — Phase 3, prose, not a transition.

This keeps the sprint deterministic end to end, which was the recommendation in
the proposal's notes. It also means the table stays small: one row per real
change, not one per write.

**D-2. No backfill, and the empty state is a contract.**

There is no history to backfill *from*. Manufacturing one out of `node.updated`
is the precise lie the table was built to stop telling — that is #977's entire
thesis. So the log starts at 0026 and fills forward, and `BoardRollup::events`
documents the consequence: an empty ticker means "nothing has moved since the
migration", never "nothing has ever moved". A renderer that cannot say that
difference out loud should render nothing rather than "no recent activity".

**D-3. A write that does not change the status is not an event.**

This is the invariant everything else rests on, and it is not hypothetical:
`post-deploy-check.sh` re-PATCHes a status to the value it already holds on
every single deploy, and agents re-set statuses routinely. A log that recorded
status *writes* would rebuild `node.updated`'s failure one table over. So the
update paths read before they write, equality means silence, and 0026 carries a
`transition_actually_changed` CHECK as the backstop. Two tests pin it.

**D-4. No actor column.**

korg has no authenticated writer to record. The `relationship.origin` precedent
is self-reported and unverified, which is defensible for "which tool drew this
edge" and not defensible on an event log, where a self-reported actor reads as
provenance while being nothing of the sort. When korg grows a real writer
identity the table takes a column.

**D-5. Schedules are not logged, and that is a completeness decision.**

`materialize_schedule` moves a `once` schedule to `done` outside
`update_schedule`, so hooking only the update path would give that kind a
half-recorded history. A feed that is silently partial for one kind is worse
than one that never claimed to cover it. A schedule's real history is its
`materializes` edges, which record every firing already. The three kinds hooked
are the three `settle_awaiting` hooks, which is what #977 named.

Writes go through a `record_transition` helper called inside the same
transaction as the status write — same discipline as `advance_completed_anchor`,
and for the same reason. A test drives a patch that changes the status and then
fails on an unresolvable area name, and asserts the event rolled back with it.

## #978 — the four questions, answered

All four had defensible alternatives. Each answer is pinned by a test.

**D-6. Granularity: both, distinguished.** A proposal is blocked when it depends
on something unfinished *or* when a covered work item does. `via` is `proposal`
or `covered`. They are kept apart because they mean different things to a
reader: the first is a sequencing decision about the sprint, the second is one
task inside it waiting on something outside. Merging them would have discarded
that for good.

**D-7. Doneness: derived from the vocabulary, per kind — and it needed a new
name.**

This is the one that would have shipped subtly wrong. `WI_TERMINAL_STATUSES`
holds `closed` **alone**, and the obvious move is to read it as "finished". It
is not: it is a *list-visibility* split, answering which rows a lean
`list_work_items` hides, and `done` stays visible by design. But `done` means
"agent satisfied" — a dependency on it **is** met. Reading the visibility split
here would report every `done` dependency as an unmet blocker, on a panel whose
entire promise is that it is right.

So work items now carry two partitions over one vocabulary:

| set | question | members |
|---|---|---|
| `WI_LIVE` / `WI_TERMINAL` | which rows does a lean list show? | `closed` is terminal |
| `WI_FINISHED` / `WI_UNFINISHED` | is a dependency on this met? | `done`, `closed` are finished |

`resolved` lands the other way and deliberately still blocks: "implemented; may
still need a user test / may not be PR'd" is not landed, and a queue row told it
was unblocked by unlanded work is told to build on sand.

Two things keep this honest rather than clever. A partition test asserts the two
sets are **not equal** — if a future edit made them agree, the name would stop
earning its existence and the next reader would collapse them back. And
`advance_completed_anchor` (sprint 051's schedule rule, which has always used
exactly `done`/`closed`) now reads the const instead of a matching literal, so
"the work is finished" has one definition rather than two that happen to agree.

Proposals and programs need no new set: for them the live/terminal split and the
finished/unfinished split genuinely coincide.

**D-8. Transitivity: one hop.** A closure is a different query and a different
UI, and nothing has asked for one. A→B→C surfaces as two rows, so the chain is
visible without being computed.

**D-9. Shape: the refs, not a boolean.** The concept renders a card naming both
sides (`korg 825 ⟂ korg #861`); a boolean cannot name the thing the reader has
to go look at, and cannot be widened later without another contract change.

**D-10. Two exclusions, sharing one rationale.** An archived blocker does not
block, and neither do the dependencies of a covered item that has itself
finished. A row held blocked forever by something nobody will ever do is worse
than a row that quietly frees up. What archiving a depended-on node *should*
surface is a dangling-dependency warning — a different card, and nothing has
asked for it. Likewise a blocker whose kind has no lifecycle korg tracks (a
link, a report, a handoff) is not a blocker: korg cannot say whether such a node
is finished, and inventing an answer is the failure this panel exists to avoid.

## The cross-project question: kfdc #1070, answered in korg

The proposal flagged #1070 as a **design input, not a downstream consumer**, and
it was right to. Ken's note: *"This is essentially showing the same data twice,
in Operations and Deconfliction."*

He is correct about the symptom, and the fix cannot live in kfdc, because **kfdc
cannot filter what korg did not label**. So:

**D-11. `sequenced_by` names the live program whose ordered slices already
express this dependency**, or `null`. korg reports the dependency either way and
says which panel is already drawing it; the render decides whether to show it.
Neither side guesses, and korg does not drop a true fact to make one panel
tidier.

It follows the `covers` edge for the covered-item case: a task dependency
running between two slices of one program is sequence, not collision, and
without following the owning proposal every cross-slice task dependency would
have read as an unsequenced card — which is exactly the duplication #1070 is
about.

## Notes

`blocked` is deliberately **not** a view of `proposal_edges` and does not
replace it. That list is *every* edge between two live rows whatever it says;
this one is the subset meaning "cannot start yet", widened to blockers that are
not proposals at all. A test pins both directions of that.

Ordering is explicit and stable — F-19's lesson about equal ranks shuffling
applies to Deconfliction cards too.

Cost: three queries, no new data. `depends_on` edges were already real and
typed, which is the case #978 made for building this instead of mining it out of
prose.

## Follow-ups

- **Schedule transitions.** Excluded on the completeness argument in D-5.
  Doing it properly means hooking `materialize_schedule` as well as
  `update_schedule`; worth a small item if the ticker ever wants "the restore
  drill fired".
- **Dangling-dependency surfacing.** D-10 leaves a real gap: archiving a
  depended-on node silently unblocks its dependents. That is the right call for
  a "blocked" panel and the wrong call for data hygiene — a different card.
- **The curator half of the ticker** stays out of scope, with kfdc Phase 3.
- **`/api/reports` has no envelope.** #1101's cheap fix is fenced, but the
  honest version is that a corpus count should not be a function of a page size
  at all. Giving that read a `total` would retire `ARRAY_CEILING`.

# 079 — `soaking`, the `soaks` edge, the soak fields, and `reviewed`

Proposal korg:2162, slice 1 of program korg:2167 (design handoff korg:2150,
program handoff korg:2168, opening handoff korg:2169). Covers #2151 (M),
#2152 (M), #2153 (S) and #2154 (S). Branch
`079-soaking-soaks-and-reviewed`. Run as an overseen karc leg, `korg-9ad76d`.

## Goal

A program whose engineering is finished but whose acceptance can only be
satisfied by the **passage of days** had nowhere to sit. It stayed `active`,
kfdc renders `active` in Operations, and Operations means "wants your
attention" — so for two or three days the board generated demand nobody could
satisfy. Ken's framing, from WI 2149: *"I will essentially be looking at an
'almost done' program for a couple of days with not much that I can do
immediately to drive it forward"* — and the constraint that rules out the easy
fix, *"just closing the program doesn't feel like the right move. We haven't
completed it."*

The design's own verdict was that **korg is not wrong; the rendering is** — so
the schema change here is small and slice 2 (kfdc) is where the problem visibly
goes away. What this slice owes the rest of the program is a vocabulary the
later legs can name without guessing.

## Premise check

All four items' claims held, verified before branching. `PROGRAM_STATUSES` and
`PROGRAM_LIVE_STATUSES` were where #2151 said; `update_program` did validate
enum membership only; `promote_queued_programs_over`,
`parked_programs_are_never_auto_promoted` and `record_transition` all existed as
described; the registry was `[LabelSpec; 9]` with `covers`/`includes` exactly
the shape #2152 wanted to copy; `report_date_fmt` and its `::option` variant
existed, which is the precedent #2153's date needed; `upsert_report`'s
`(source, report_date)` upsert had the `existing` UPDATE branch #2154's reset
belongs in. 0033 was `comment_origin`, so 0034 was free.

One correction the premise check did not catch and the first compile did: the
table is **`workitem`**, not `work_item`. The proposal, both handoffs and every
work item spell it `work_item`, and the migration was written that way. Caught
before it ran, but it is worth recording as the seventh candidate the notes
warned about — a name that looks right, reads right in four documents, and
matches nothing.

## The design decisions this slice made concrete

All five were settled upstream and none was re-litigated. What follows is only
where building them forced a choice the design had left open.

**One migration, not four.** 0034 carries the widened `program.status` CHECK,
the two `workitem` columns and `report.reviewed` together. The proposal left
this to the leg; the argument for one file is that the MCP surface describes all
four facts at once, so splitting them admits a window where the tool
descriptions promise something the database refuses. It is DDL only — every
column is nullable or defaulted, nothing is backfilled, no existing row changes
meaning — which is what keeps the rollback claims clean.

**`soaking` is live, and that was the load-bearing choice.** Filed terminal it
would partition just as cleanly and vanish from `list_programs` and
`board.programs` on the day it most needs watching. A soaking program is
precisely the one somebody must still act on: judge the evidence, or notice the
test was invalidated. Moving it to a calmer *panel* is the consumer's job (GP-19
exactly), never korg's.

**`soaking` is declared on entry and derived on exit, and that pairing is new.**
GP-19 draws the line between `queued` (derived — korg maintains it) and `parked`
(declared — korg never touches it). `soaking` is neither cleanly: it is declared,
and it is the only status korg **gates on entry**, but a slice starting under it
lifts it back to `active` the way one lifts a `queued` program. That is
deliberate — entering is a claim worth checking, and leaving must stay
frictionless because a soak that fails at 04:00 has to become new work without
arguing with a state machine. It refines GP-19 rather than contradicting it;
see *Plan amendment* below.

**`promote_queued_programs_over` was renamed `promote_programs_over`.** #2151
said "extend it". Extending it and keeping the name would have left a function
whose name asserts one member of a two-member set — the drift class kfdc #1196
exists because of, in a file whose own doc comment says *"This prose is
load-bearing rather than decorative."* The gated set is now
`PROGRAM_LIFTABLE_STATUSES`.

The rename forced a real change, not just a signature: the transition log needs
the status each program moved *out of*, and `UPDATE … RETURNING` yields the new
row. With one liftable status that could be a bound literal; with two it cannot.
The query is now two CTEs, and `a_slice_starting_lifts_a_soaking_program`
asserts the log records `soaking → active` rather than a hardcoded `queued`.
A log that recorded every lift as leaving `queued` would quietly falsify the one
record of a soak having failed.

**The entry rule fires only on entry.** Re-asserting `soaking` on a program
already soaking does not re-run the check. Otherwise an unrelated
`update_program` — retitling it, say — starts failing the moment the first soak
is judged, for a reason that has nothing to do with the call.
`re_asserting_soaking_does_not_recheck` pins it.

**Neither `soaks` refusal is re-checked after the edge exists.** A soak already
in the array is the operator's to adjust; moving a `check_after` out is a normal
act, and a rule that fought it would make the honest move harder than the
dishonest one.

**A blank `invalidated_if` is a missing one.** `"   "` is refused. The design
calls the invalidation statement the most valuable and most droppable part of
the model; whitespace is exactly how a required field gets skipped.

**`related_context` now takes a slice of labels.** `get_program` carries two
edge sets in their own arrays (`slices`, `soaks`), and reporting either twice
would spend a slot of the LB-3 cap saying what the payload already said better —
the treatment `has_attachment` already gets on a work item. Five call sites, one
line each.

**`review_report`, not `update_report`.** One field. Rewriting a report's
content is already `create_report`, which upserts and replaces the finding edge
set with it; a second general update path would give korg two ways to rewrite a
body that could disagree about the edges, and the edge replacement is the part
`upsert_report` gets right (D-7).

**`set_report_reviewed` deliberately does not `touch_node`.** Marking a report
read is not a change to the report. Bumping `updated` would move it in a
recency-ordered list and make the source-freshness read think a new report had
landed — which is the "stale GREEN" failure `sources` exists to catch.

## The wire format, tested as such

The proposal named this as a live risk: *"`check_after` as a string on one
transport and a date on the other (0010's `report_date` history is the
precedent — reuse `report_date_fmt`)."*

It does. `WorkItemRow.check_after` serializes through `report_date_fmt::option`,
`NewWorkItem` deserializes through it, and `WorkItemPatch` goes through a
purpose-written `double_option_date` — because the plain `double_option` would
have routed the value through `time`'s own serde impl instead, which is the
divergence in miniature. `check_after_is_yyyy_mm_dd_on_the_wire` asserts the
JSON both directions, that explicit `null` clears, and that an **absent** key
does not.

## What shipped

- **Migration 0034** — `program.status` CHECK widened to admit `soaking`;
  `workitem.check_after DATE` and `workitem.invalidated_if TEXT`;
  `report.reviewed BOOLEAN NOT NULL DEFAULT false`. Column comments on all
  three. Structural postcondition block, no counts (0018/0030/0031's lesson).
- **`soaks`** registered in `korg_core::relationships` (`REGISTRY` 9 → 10),
  `program` → `workitem`, directed, ranked, no same-project rule, with
  `SOAKS_LABEL` as the one spelling. Two refusals in `repo::relate`.
- **`soaking`** in `PROGRAM_STATUSES` / `PROGRAM_LIVE_STATUSES` /
  `PROGRAM_LIFTABLE_STATUSES`, with `check_soaking_entry` and the widened lift.
- **`soaks[]`** on `get_program` and `board.programs[]`, one batched query
  (`program_soaks_for`) so the board does not become N+1.
- **`reviewed`** on `ReportRow`, reset in the `upsert_report` replace branch,
  three-way filter on `list_reports`, `review_report` (MCP) and
  `PUT /api/reports/:node_id/reviewed` (REST).
- **Web** — `soaking` styled indigo (cool, so it cannot re-create the bug it
  fixes); an *Extended tests* section on the program page giving
  `invalidated_if` equal billing with the date rather than a tooltip; the soak
  fields on the generic node-detail page via `preview.rs`; a *mark reviewed*
  toggle on the reports list.
- **Docs** — `docs/api.md` tool catalogue and label registry, `docs/usage.md`
  vocabulary bullet, REST table and report section, README tool count 55 → 56.
- **Tests** — `crates/korg-core/tests/sprint079.rs`, 19 cases.
  `web/tests/e2e/soaking.spec.ts`, 3 cases.

## Follow-ups

- **The e2e spec was written but not run.** Playwright is not in `just check`
  or CI — it needs a built bundle, a running `korg-api` and a Postgres, and CI
  says so deliberately. `soaking.spec.ts` is written against the existing
  `program-close-out.spec.ts` conventions and should be run against a live
  instance at the next opportunity.
- **`list_work_items` stays lean** — the soak fields are on the full row and
  the detail read, not the lean MCP summary, exactly as #2153 asked. If the
  soak scan wants "every soak across every program" as one call, that is a new
  read and a new work item, not a widening of this one.

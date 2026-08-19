# 068 — Work-items open counts, category on the project rollup, the F-9 attachment nit

Proposal korg:1443. Covers #1442 (S), #1414 (XS) and #1412 (XS).
Branch `068-work-item-open-counts`.

## Goal

Ken asked for the numbers Planning already shows to appear on the Work
items page too, but reduced to one figure: how much open work is in each
project. #1414 rode along because it is the same project-rollup row one
layer down, and #1412 because it costs minutes and clears the last open
item from the 044–059 surface re-review.

## What shipped

**#1414 — `category` on `PlanningRollupRow`.** One column on the existing
statement (`pj.category`), one field on the row, `just gen`, and the board
table in `docs/api.md` plus `get_board`'s tool description updated together
so `docs_drift` stays green. The same move sprint 045 made for `status`,
for the same reason: this read returns *every* project, so the row has to
carry what a consumer needs to drop one.

The case that forced it is `eval`. Sprint 065 (#466) made it a permanently
**active** project — #884 makes an archived project refuse writes, so the
residue bucket has to stay live — which puts harness residue inside every
project-spanning read while `status` says, accurately, `active`.

**#1442 — open counts on the Work items rail.** Three placements, exactly
as Ken specified them: a total on "All projects", a per-category subtotal
on each group label, and a per-project figure to the left of the
edit-project pencil. All three render through one snippet, and all three
read `planning_rollup`'s `wi_total`.

**#1412 — the attachment miss names both spellings.** `attachment_not_found`
in `repo/attachments.rs`, used by all three sites that produced the string.
`no attachment with node_id 3114 (img-c2a)`.

## Decisions

- **"Open" means unarchived and not `closed` — the page's own default
  filter, not `wi_status == "open"`.** This was the one judgement call in
  #1442 and it is close to forced. `resetFilters` on this page hides
  exactly `closed` and archived, so `wi_total` is the count of rows you get
  when you click the project. The strict reading would have put a `3` next
  to a project that opens to seven rows — the failure `planning_rollup`'s
  own doc comment warns about ("two definitions of 'spoken for' on one
  screen is a bug report waiting to happen"). korg's vocabulary already
  calls this set open work, in that same comment. The tie is held by a
  test, not by a comment: `the_rail_count_is_exactly_what_the_default_list_shows`.

- **korg does not dim or hide EVAL projects, and ships no `is_eval` flag.**
  This was #1414's open step 2. There is no web board consuming `depth` —
  the two rails read `ProjectRow`, which has carried `category` since #678,
  and already group by it, so an EVAL project lands in its own labelled
  group. More binding: GP-10 (korg+ plan, project tiers as data) treats
  EVAL as the tier below every tier, and a switch keyed on
  `category == "EVAL"` is the parallel switch that decision forbids. korg
  ships the datum; each consumer decides. Recorded in the field's doc
  comment, in `docs/api.md`, and in a test that asserts the *absence* of
  filtering on purpose.

- **The counts are re-read from the server, never adjusted in the client.**
  Decrementing a number here is how the rail would come to disagree with
  the list beside it; the status vocabulary decides what open means and
  only one place gets to hold that rule. The refresh hangs off `loadItems`
  — every mutation path already ends there — rather than off a list of
  mutation sites somebody has to keep complete. Quick Edit is the one
  exception (it deliberately does not reload the list, so a row closed
  there stays put) and refreshes for itself.

- **The count sits outside the "All projects" button, not inside it.**
  Inside, it joined the button's accessible name — "All projects 158" —
  and every caller addressing that row by its exact name broke. The e2e
  suite caught it (`project-scope-shared.spec.ts`), which is the argument
  for running that suite: `pnpm check`, `pnpm lint` and the whole Rust gate
  were green with the bug in the tree. Project rows already had the right
  shape, count as a sibling of the name button; "All projects" now matches.

- **#1424 stayed out**, per its own comment (id 809): it needs a kfdc-side
  work item and a program first, which is an act of planning, not a scope
  decision inside this sprint.

## Tests

`crates/korg-core/tests/sprint068.rs`, six tests: `category` rides the row
(including `None` for an unclaimed project); the EVAL case end to end,
asserting korg does *not* filter; the rail count equals the default-visible
set across all five statuses plus archived; an empty project counts `0`
rather than going missing; and both spellings in the attachment message,
with the non-positive id falling back to decimal alone.

`just check` green. Playwright run against a restored production dump
(`korg-20260819-032116`): 78 passed, 1 flaky (`filter-dismiss`, passes on
re-run). Two failures triaged — `project-scope-shared` was this sprint's
own a11y-name regression, fixed above and re-run green;
`cards-dnd` fails its own guard ("the board outgrew what one pointer drag
can span") because a production-sized Cards board exceeds the 720px
viewport, which is unrelated drift.

## Verified live

Against the restored dump, before teardown:

- all 50 rollup rows carry a category —
  `{"project":"agent-skills","status":"active","category":"Ops","wi_total":9}`
- the rail rendered all three figures, and the definition agreement was
  visible on screen: `kfdc 11` in the rail, "11 items" in the list.

## Follow-ups

- kfdc/kfo adopting the EVAL filter is their work; the datum they needed
  now exists (#1414 step 3).
- The category subtotals sit flush right at the rail edge while the
  per-project counts sit left of the pencil, so the two do not form one
  column. That is Ken's spec read literally ("right justified" for the
  group rows, "to left of the pencil" for projects). If he wants them
  aligned instead it is a one-line pad on the group rows.

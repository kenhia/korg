# 088 — correct the records that say kfdc/korg-dash render `due_schedules`

Proposal korg:3235, covering **WI 3085**. Slice 5 of program korg:3245
("Low-hanging fruit, run 3"), run as an overseen karc leg on kai. Docs only.

## Goal

Four records said kfdc and korg-dash "already render `due_schedules`" and
"inherit it for free". Neither does, nor ever has. WI 3085 measured it on
2026-09-22, and the overseer's ruling on it (comment 2896) split the fix into
three weights:

1. WI 1644's decision comment: already corrected by an erratum comment (2895).
2. `docs/api.md` § "In-flight schedules (#1644)" and the
   `Board::in_flight_schedules` doc comment: **correct in place**, because
   they are live documentation.
3. korg+ `PLAN.md`'s 1393 row: an **erratum to the rationale** under the
   plan's amend rule, not a contradiction of the decision.

## Premise check

The premise holds, re-measured 2026-09-25 on kai. `due_schedules` appears in
kfdc only in prose, plus a `board.ts` comment that says it is deliberately
*not* declared. korg-dash contains no occurrence of it at all. The only other
hit fleet-wide under `~/src` is the korg+ row.

The re-measure also found **one more wrong claim than the WI named**. WI 3085
says "korg's own Today page is its only consumer". It is not a consumer at
all. The Today pill (`web/src/routes/+page.svelte`) calls
`api.schedules({ due_only: true })`, which is `list_schedules`, and never
reads the board. So `get_board.due_schedules` has **no known consumer**. The
corrections say that rather than repeating the WI's softer version.

## What shipped

- `crates/korg-core/src/repo/board.rs`: the `in_flight_schedules` doc
  comment now justifies the sibling field on its own terms: additive, costs
  any consumer nothing, and let kfdc #1645 opt in. The old claim is kept as a
  parenthetical correction citing #3085, so a reader of the older decision
  records can see where it went. `just gen` carried the change into
  `web/src/lib/generated/korg.ts`.
- `docs/api.md` § In-flight schedules: the same correction in the first
  bullet. The closing paragraph also said "korg's own Today page shows the
  count as a pill; kfdc and korg-dash get the block for free". Both halves
  were wrong. The Today pill counts *due* schedules, and the "in flight" pill
  lives per row on `/schedules`, from `list_schedules`' `outstanding` field.
  Neither reads the board. It now says that kfdc renders the block as its
  Standing Orders panel and korg's UI does not read it.
- korg+ `PLAN.md`, the 1393 row of the 2026-08-17 queue sweep: the
  parenthetical is struck and a dated erratum appended, with the verdict
  unchanged. Committed straight to main and pushed per that repo's README
  (`02495ee`).

## Fifth place

I searched for a fifth copy and found none that makes the claim. Two places
in kfdc *report* the claim as present-tense fact: `docs/design.md` ("No
due-schedules surface, and the record says otherwise") and the comment in
`src/lib/panels/StandingOrders.svelte` ("though four decision records say
both do"). After this ship both describe a state that no longer holds. They
are left alone here. They are kfdc's own navigational prose, and kfdc has no
documented procedure a korg sprint could execute there. The correct text now
lives in korg and korg+, so kfdc's next sprint can change "says" to "said".

The get_board tool description in `crates/korg-mcp/src/tools.rs` says "a
consumer already rendering due-ness needs no change". That is conditional and
true, so it is left as is.

## Not decided here

Whether kfdc or korg-dash *should* render due schedules is a product question
nobody has asked. Per the proposal notes it is not filed.

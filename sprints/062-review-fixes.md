# 062 — Re-review fixes: parked buckets, description drift, lane ghosts

**Proposal:** korg:1392 (rank 4) · **Work items:** #1386–#1391, #1411
**Branch:** `062-review-fixes` · Run 2026-08-17.

Sprint 061's re-review filed seven mechanical findings and one rider; this is
the sprint that lands them. Nothing here is a design change — every item is
"the code grew and a hand-written copy of it did not", which is why five of the
seven ship with a fence rather than just a fix.

**Cross-project plan:** korg is mapped to `korg+/` in the
cross-project-planning index. The 2026-08-17 queue sweep verdicted 1392
*proceed — plan-independent mechanical fixes*, and nothing here contradicted a
`GP-n`. #1411 **completes substrate-queue item 1**, so the plan is amended in
this ship (see below). GP-3 was live throughout #1411's design and is recorded
in the field's own doc comment: `updated` is a staleness signal only while it
stays work activity, so no future priority write may ride `touch_node`.

## What shipped

### #1386 — the parked bucket, and a fence for bucket sets

`parked` reached both vocabulary partitions in sprint 054 and neither
covered-work rollup, so a parked covered item counted toward `covered_count`
and toward no bucket. Both aggregates now **derive their `FILTER` list from
`WI_STATUSES`** (`covered_bucket_filters` / `covered_bucket_coalesce` in
`repo/programs.rs`) rather than spelling four literals, and `BoardProposal` /
`ProgramSlice` carry a fifth count. `BoardProposal`'s doc comment claimed "the
four counts sum to `covered_count` — `WI_STATUSES` is exactly these four"; it
had been false for three sprints and now says what the fence keeps true.

The durable half is `rollup_buckets_cover_wi_statuses`, which reads the
*serialized* row — the field names are the wire contract, and the SQL aliases
from the same list — plus a DB-backed test that a proposal covering one item of
every status still sums.

### #1411 — `updated` on the lean work-item row (rider)

One column in the lean SELECT, one field on `WorkItemSummary`, `just gen`.
kfo's Scouting Report proxied age with `wi_number` and said so; last-touched
cost one `get_work_item` per row. `updated` only — creation age already has its
proxy.

### #1387 — description sweep, and a fence over tool prose

`relate` claimed "these eight labels and no others" against a registry of nine
(`materializes`, sprint 051): the runtime error message was registry-derived
and right the whole time, only the prose lied. The numeral is gone —
`relate_names_every_registered_label` keeps the list whole instead — and the
endpoint-validation sentence now matches what the registry actually pins.

`list_work_items`, `update_work_item` and `list_schedules` described the
four-status world; swept, along with `list_comments` ("work item or card" →
any kind), `get_program`/`get_board`'s rollup sentences, both awaiting
descriptions, `ScheduleRow.outstanding` and `ScheduleOmitted.not_due` (which
documented itself as counting *returned* rows when it counts hidden ones).

Structural half: `value_runs_in_tool_descriptions_are_complete_sets`. The 050
fence reads `docs/` prose; tool descriptions were outside it, and they are the
prose agents actually read. A run of values joined by `/`, `|` or `,` whose
members all belong to one vocabulary must **be** a set korg has — a whole
vocabulary or a named partition, the new `vocab::PARTITIONS` registry.

Two decisions worth keeping:

- **`PARTITIONS` exists because tool prose legitimately names subsets.** "a
  proposal reaching `done`/`declined`" is true and is not `PROPOSAL_STATUSES`.
  Without a registry of the legal partitions the fence could only reject
  correct sentences or accept stale ones, and F-4 *is* a stale one.
- **The extractor is fenced too** (`the_extractor_reads_the_spellings_
  descriptions_use`). A scanner that quietly matches nothing passes for free,
  so the four real spellings are pinned, and so is the assertion that
  `open/resolved` — the stale spelling — matches no registered set.

### #1388 — docs/api.md

Three disposal rows (program, schedule, attachment) and the sentence naming
**attachments as the one deliberate exception** to refuse-if-referenced: an
image is bytes, not history, so `DELETE /api/img/:id` cascades rather than
refusing, and an agent applying the doctrine to it predicts the wrong outcome.
`the_disposal_table_covers_every_node_kind` closes the class the way the WI
suggested — a kind absent from the table is a kind whose disposal gets guessed.

Also: the registry module doc's "free-form labels stay legal" (closed at LB-2,
the text older than the closure it described), the images example's
`http://kubsdb:5674` → the tailnet FQDN, and one paragraph on staleness using
UTC `current_date` while flow buckets in `KORG_TIMEZONE`.

### #1389 — the awaiting lane clears for cards

`set_awaiting` invited marking a card from the start; `settle_awaiting` and
`list_awaiting` knew three kinds. Both now read `CARD_TERMINAL_STATUSES` —
`Done`/`Cut`, a named constant precisely because hand-copied status sets are
what this whole sprint is about. D-7's rationale applies unchanged: the kanban
is Ken's board, so reaching its last column is him answering, and `Cut` retires
the ask the way archiving does.

### #1390 — the materialize gates run under the lock

The three gates ran on a pre-transaction read; the transaction took
`FOR UPDATE` and never asked again. Extracted to `materialize_gate` and called
twice — cheap fail-fast before the transaction, then again on the locked row.

**The test reproduces the window rather than approximating it.** The WI allowed
asserting the check merely exists, but two sequential calls fail at the
*pre*-check, so such a test passes with the fix reverted. Instead: hold the
schedule row, poll `pg_locks` until the call under test is demonstrably blocked
on it (every pre-check behind it), commit what the winner would have committed,
and assert the refusal comes anyway — and that the refused call wrote nothing.

### #1391 — the plan payload is complete, and says so

`GET /api/projects/:name/plan` took one 500-row page and discarded the
envelope. It now walks to `total` and carries `total`, so the payload can be
checked against itself. Seeded 501 items in one statement to fence it.

**Archived inclusion decided, not inherited**: they stay in. It was an
undeclared `archived: None`; it is now a decision with a reason — an archived
item is still a dependency others were satisfied by, and dropping it from the
graph turns a met dependency into a missing node. What to *show* is the
consumer's call, and `/plan` already filters archived out of its open lanes.

## Not in scope

F-9.5 (`get_attachment`'s not-found message names the decimal node id even when
the caller passed `img-<hex>`) was never filed as a work item — the review
listed it under nits and #1388 covers F-9.2/3/4 only. Left as-is deliberately;
it is a message-cosmetics nit, not a wrong answer.

## Plan amendment

`korg+/PLAN.md` substrate queue item 1 is done. Amended in this ship per the
plan's own amend rule (GP-8's other half).

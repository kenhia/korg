# 042 — Proposal membership is invisible outside a proposal

**Proposal:** korg:825 · **Covers:** #824, #813, #823 · **Branch:** `042-proposal-membership-surface`

## Goal

`covers` only ever read one direction. A proposal could name its work items;
a work item could not name the proposal that had claimed it, and a project
could not say how much of itself was spoken for. The 2026-07-31 backlog review
answered *"which of the 135 open items are already in a live proposal?"* with
17 `get_proposal` calls and a script.

Three WIs, one mechanism: the same aggregate evaluated per row (#824, #813) and
grouped by project (#823). The proposal's argument for bundling them was to pay
the contract cost **once** — one revision of `WorkItemRow`/`WorkItemSummary`,
one `just gen`, one measurement.

## Decisions

**Only *live* proposals mark an item as spoken for.** `proposal_node_id` is
null when the only covering proposal is `done` or `declined`. The question the
marker answers is "is this already claimed", and a declined proposal claims
nothing — showing it would be a false positive on exactly the question being
asked. The rail's counts use the identical predicate, because the rail and the
rows sit on one screen and two definitions of "spoken for" there is a bug
report waiting to happen.

**The row carries the proposal's id, not a boolean.** #824 wanted to render
`<id>/<proposal_id>`; a nullable id answers both that and "is it covered". It
became its own `Prop` column rather than `<id>/<prop>` in the ID cell — the ID
cell is what you type into the find-by-ID jump, and it should stay a single
number.

**Both orientation facts in the WIs' draft SQL were wrong**, so tests assert
them rather than assuming. The `relationship` columns are
`left_id`/`right_id`/`relationship`, not `left_node_id`/`label`. And the labels
point opposite ways: `covers` is proposal → work item (item is the **right**
end), `has_handoff` is node → handoff (item is the **left** end). #813 warned
"don't assume the coverage edge mirrors `has_handoff`'s shape" and was right.

**The handoff glyph is 📄, not the 🫱 the WI proposed.** 🫱 is Unicode 14 and
#813 flagged it as the least portable of the three. korg already renders a
handoff as 📄 on the Planning card's related-edge chips — matching the page
that exists beats introducing a second symbol for the same thing, and it
sidesteps the font question entirely.

**The rollup is REST-only** (`GET /api/proposals/rollup`). It renders a rail;
the MCP catalogue is a surface every agent pays to read, and the same
information is already reachable per row there.

## The measurement — and the shape it changed

#813 asked for "a measurement rather than an assumption", and Ken's note on
#823 was *"the right SQL query makes the first one quick enough, but timing the
op beats guessing"*. `crates/korg-core/tests/sprint042_measure.rs` (`#[ignore]`d;
run with `-- --ignored --nocapture`) seeds 1000 work items / 100 proposals /
60 handoffs across 30 projects — production shape, rounded up, with the 78%
`closed` skew #861 measured.

Both WIs sketched `EXISTS (SELECT 1 FROM relationship …)` correlated
subqueries. **The measurement rejected that shape.** 500-row page, median of 15:

| read | baseline | correlated | joined (shipped) |
|---|---|---|---|
| `list_work_items` (REST) | 2249µs | 8900µs (+296%) | **4529µs (+101%)** |
| `list_work_items` (lean) | 1131µs | 4028µs (+256%) | **2781µs (+146%)** |

A 500-row page does 500 index lookups plus 500 joins to `sprint_proposal`.
Pre-aggregating instead — both edge sets are small, hundreds of rows and dozens
— lets Postgres scan each once and hash-join, turning per-row work into
per-query work. Roughly 2× on both reads. `planning_rollup` got the same
treatment (three per-project subqueries → two grouped CTEs): **7141µs → 949µs**,
7.5×.

What a caller actually waits for, with the untouched count statement held
constant: `list_work_items` 9.1ms → 11.4ms (+25%), lean 6.1ms → 7.7ms (+27%).
Single-digit milliseconds on a homelab LAN — the cost is real and it is fine.

**All three of Ken's rail figures shipped.** `<proposals> | <wi_in_proposal> /
<wi_total>` at 0.95ms for 30 projects; no need for either degraded form.

One harness bug worth recording, because it nearly became the headline: the
first cut compared the old raw SQL against `list_work_items()` and reported a
+960% regression. Artefact — the repo functions also run a second statement for
`total`/`omitted`, so it was timing two queries against one. Every comparison
above is raw SQL against raw SQL, same rows, same session.

## Shipped

**Server** (`korg-core/src/repo.rs`)
- `membership_columns!` / `membership_joins!` — one SQL fragment pair spliced
  into both row tiers via `concat!`, so they cannot drift on what "spoken for"
  means.
- `WorkItemRow` += `has_handoff`, `proposal_node_id`.
- `WorkItemSummary` += `has_details`, `has_handoff`, `proposal_node_id`.
  `has_details` is the marker the Review page could not show at all — the
  projection carries no bodies, so it showed no 📝 rather than a false negative.
- `planning_rollup()` + `PlanningRollupRow`, and `GET /api/proposals/rollup`.

**Web**
- `RowTickers` gains the handoff marker; both lists now pass all three.
- Work Items: `Prop` column, title in the Ops colour when covered, bold dropped
  from list titles.
- Review: details and handoff tickers, honest now rather than absent.
- Planning: the dropdown is a project rail — same grouping, same alphabetical
  toggle, "All Projects" default, counts instead of the edit-project pencil and
  add-area diamond. The "Proposals agents (or you)…" line moved beside the
  header. `IN_PROPOSAL_COLOR` is exported from `domain.ts` so the row title,
  the Prop column and the rail cannot drift.

**Tests** — `korg-core/tests/sprint042.rs` (9), `korg-api/tests/sprint042.rs`
(3), two e2e specs. Written against behaviour, not SQL shape: every one passed
against both implementations, which is what let the shape change mid-sprint on
the strength of the measurement.

## Bugs caught in the sprint

- **Planning filter silently broke.** `pickProject` stopped calling `rebuild()`
  when it stopped refetching. `active`/`done`/`declined` are `$derived` and
  re-filter themselves, but the two drag zones are plain state that `rebuild`
  populates — so the queue kept showing the previous project's proposals. Only
  the e2e caught it; the unit tests never touch the buffers.
- **The rail legend was stranded.** First placed under the rail with the
  checkboxes, which is off-screen past thirty-odd projects. Moved above.

## Follow-ups

- `wi_counts` is now the dominant cost of both list reads (~6.8ms of the REST
  call path, untouched by this sprint). Worth its own look.
- #810 (parked status) and #582 (screen captures) stayed out, as the proposal
  intended.

# Sprint 015 — response contracts: envelopes, two-level reads, get_proposal

Proposal `korg:555` — the third bundle (B3) of the 2026-07 deep-review cleanup
(`sprints/review/REVIEW.md`). Covers WIs #534–#538 and #565, closing findings
F-09, F-10, F-11, F-19, F-20 and implementing decisions D-3 and D-10. Exact
contracts: REVIEW.md §4.3.

This is the deliberate breaking sprint (D-3): shapes, defaults and the skills
that read them all move together, with no dual-shape transition period.

## Collection reads are enveloped, bounded and filtered (#534)

`list_work_items`, `list_cards`, `list_links` and `list_topics` were unbounded
full-table reads that silently included archived rows — and `list_work_items`
carried full `content`+`details` per row, the exact payload problem
`survey_work_items` was invented to dodge.

All four now return `{items, total, limit, offset}` where `total` is the full
**filtered** count before the limit, so `truncated`-style guesswork is
unnecessary and paging is possible. `limit` defaults to 200, clamped to 500.

Filters are server-side: work items by `project`, cards by `status`/`project`,
links by `disposition`/`read`, topics by `q`. `search_topics` became an alias —
it was always `list_topics` with a filter — and both names stay registered.

**Archived rows are excluded by default**, the deliberate default change. The
tri-state is spelled once per transport: omit for live only, `true` for
archived only, `?archived=all` (REST) or `"archived": null` (MCP) for both.
An unrecognised value is a 400, not a silent reinterpretation.

The default lives in exactly one place — `korg_core::repo::archived_default()`
— because during this sprint core briefly defaulted to "both" while the
transports defaulted to "false", and the topic tests caught the disagreement.
One declaration, consulted by core and both transports, is the fix.

Ordering gained id tie-breakers throughout (F-19), so equal-rank cards and
proposals stop shuffling between calls.

## Two-level reads, generalized (#535)

Sprint 012's contract — collections *signal* discussion, focused reads
*inline* it — stopped at work items. `CardRow`, `ProposalRow`, `ReportRow` and
`Topic` now all carry an exact `comment_count`; `ProposalRow` also carries
`covered_count`.

`GET /api/work-items/:wi` returns the same detail shape as the MCP
`get_work_item` tool. They were one operation under one name with two shapes,
which is how the Sprint-012 fix reached agents but never reached the REST
surface. REST also gained the work-item `category` patch it had hardcoded to
`None`.

## get_proposal (#536)

There was no authoritative proposal detail read anywhere. The Planning page
fetched every proposal *and every work item in the instance*, then called
`neighbors` once per proposal and joined client-side; `start-sprint` did the
same dance over MCP in three tools.

`get_proposal` (REST `GET /api/proposals/:node_id` + MCP tool) returns the
proposal, its `covered` work items (`wi_number`, `node_id`, `title`,
`wi_status`, `wi_tshirt`, `project`, `comment_count`, ordered by `wi_number`)
and its capped comments — one call. It reads the `covers` edge in the semantic
orientation sprint 014 established, which is what made it straightforward.

The Planning page and the `start-sprint` skill both consume it now. The page
only issues a detail read for proposals whose `covered_count` is non-zero;
the count on the row answers the rest.

## MCP truthfulness sweep (#537)

- `survey_work_items` advertised `archived` default `false` while the server
  treated omitted as *both*. It now advertises no default, because it applies
  none.
- Every list tool documents its envelope, ordering, filters and the archived
  default in its description.
- **`update_card` now takes `project_id` on both surfaces** (agreed with Ken).
  REST used to accept a project *name* and create the project as a side effect
  of a card edit — a hidden write inside an update. Creating a project from a
  typed name is a UI affordance, so the cards page does it explicitly before
  patching.
- New parity test asserts the advertised defaults match korg-core's constants
  for every collection tool, and that `survey_work_items` advertises none.
  B4 makes this structural by generating the schemas; until then it's a fence.

## Atomic link updates (#538)

REST `PATCH /api/links/:id` ran up to three independent repo calls, so a
mid-sequence failure left a partial write. `update_link` is now one core
function in one transaction, with validation before the transaction opens — a
rejected disposition cannot leave the tags from the same patch applied.

MCP gained an `update_link` tool covering disposition + read + tags, restoring
the workflow migration 0004 intended: agents could record *nothing* about a
link except read/unread. `mark_link_read` stays registered, marked deprecated,
pointing at `update_link`.

## Proposals by project (#565)

`list_proposals` filters by `project` on both surfaces, and the Planning page
shows a project chip per card plus a filter control whose choice survives a
reload (same sticky-localStorage convention as the work-items rail).

## Verified

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo test --workspace` — **58 passed, 0 failed**.
- New REST contract tests cover the envelope, the archived tri-state, paging,
  per-entity filters, `comment_count` on every commentable row, the REST
  detail shape, `get_proposal` (including empty covers and a missing id), and
  that a rejected link patch leaves *nothing* behind. New MCP tests cover the
  same over that transport plus the schema-parity check.
- `pnpm check` (319 files, 0 errors) / `lint` / `build` clean.
- Playwright **28 passed, zero flaky** on a fresh database — including a new
  `planning-project.spec.ts` that seeds two projects and asserts the chip, the
  filter and its stickiness. (Two drag specs failed first on a database I had
  seeded repeatedly during development; 28/28 on a clean one confirmed
  accumulation, not regression — the same effect seen in sprint 014.)
- Production untouched: all verification ran against scratch containers, which
  were destroyed afterwards.

## Deployed

Deployed to `kubsdb` 2026-07-22 (post-merge, from `main` @ `677b609`) via
`/sprint-ship`'s new Phase 7 — the first run of the deploy-from-sprint-ship
step, driven by this repo's `.sprint-deploy`. Image `sha256:a35cf1bc…`; prior
production image `sha256:b5d220a5…` (sprint 014) retained for rollback.
Container healthy, 0 restarts, on the loopback+LAN binding.

No schema change this sprint, but the archived default changes what the lists
*return*, so counts were captured before the deploy and reconciled after:

| | baseline (pre-deploy) | live after |
|---|---|---|
| work items | 375 total, 16 archived | `total=360` default, `376` with `archived=all`, `16` with `archived=true` |
| cards | 26 | `total=26` |
| links | 4 | `total=4` |
| proposals | 56 | 56 |

The work-item totals are +1 against the baseline because **Ken created WI #572
("Post deploy health check?") at 18:47:07, while the image was building**.
376 − 16 archived = 360, which is exactly what the default envelope reports —
nothing was lost, only filtered.

Verified live over `https://kubsdb.encke-wahoo.ts.net:5674`: the envelope with
`limit=200`/`offset` paging and the `archived` tri-state (including a 400 on
`archived=maybe`); `project` filters; `get_proposal` on `korg:557` returning
its four covered work items with status, size and comment counts in one call;
`GET /api/work-items/544` returning the detail shape with its comment inlined;
`list_proposals?project=korg` carrying `covered_count`; 44 tools over MCP with
the enveloped lists, the `archived: null` escape hatch, `get_proposal`, and
`update_link` refusing a bad disposition with `invalid_input`; `/plan` deep
link 200; `scripts/mcp-roundtrip-check.sh` green.

## Lock-step changes outside this repo

The `start-sprint` skill now resolves a proposal with one `get_proposal` call
instead of three tools plus a join, with the old sequence documented as the
fallback.

## Notes for what follows

- **B4** should generate these shapes rather than hand-mirror them; the parity
  test here is the stopgap it replaces.
- **The handoff plan** extends `get_proposal`'s shape
  (`handoff_count`/`handoffs`/`handoffs_truncated`), which now exists.
- The eval-tagging question (WI #466) is *not* implemented: the filter
  conventions here are shaped so a `tag`/`exclude_tag` filter can join them
  without re-litigating parameter style, but no tag filter ships in this sprint.

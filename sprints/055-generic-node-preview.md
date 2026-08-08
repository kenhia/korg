# 055 — Close the #870 audit punch list: `NodePreview` becomes the generic node renderer

Proposal: korg:1108 (rank 1). Items: #1104 (S), #1105 (S), #1106 (XS), #1107 (XS).

## Goal

All five items on sprint 054's node-shape punch list, in one sprint, because
four of them were one fix.

The proposal made the argument up front and it held: `add_comment`/
`list_comments` accept any node id, and `node.tags`/`created`/`updated` exist for
every kind — **the data model was already generic and only the UI was per-kind.**
That mismatch is what filed #1104, #1106 and #1107 as three separate items, and
`schedule` is the proof it regenerates on its own: it landed in sprint 051 brand
new and arrived with every one of these gaps.

So: make `NodePreview` render the node-level columns for every kind, then decide
separately what each *list* surface additionally needs.

## Decisions

**D-1 — comments and timestamps are generic, in `NodePreview`.** One mount of
`Comments`, one timestamp line, and #1104 (proposals, schedules), #1107 (links)
and the detail half of #1106 close together. The alternative — wiring
`Comments` onto Planning and Schedules as two more per-kind sections — closes
the same items this sprint and files punch-list items 6–10 the next time a kind
is added. Nothing about the data model made the per-kind version necessary; it
was just how the UI had grown.

**D-2 — list rows are a density decision, taken per surface and argued.** The
preview makes every field *answerable*; putting all of them on every row of
every surface is density with no reader. Two exceptions earned their place:

- `queued <date>` on a Planning card — the queue is rank-ordered, so nothing
  else on that surface says how long something has been sitting in it.
- `updated` on a program header — a program is long-lived enough that how
  recently it moved is part of reading it.

Program *list* rows got neither: `/programs` is a routing surface, and its
detail page is the reading one. Recorded in the Hidden table of
`docs/node-shapes.md`, so this reads as a decision rather than an oversight —
which is that document's whole thesis.

**D-3 — schedules keep their run history on the row, not in the preview.**
#1105 went the other way from the rest, on purpose. `materialized[]` is read
*against the cadence sitting beside it* ("quarterly · due 2026-11-08 · 1 run"),
and the judgement it supports — three runs on schedule vs three runs in one
week — needs that context. The count is now the disclosure that opens it,
fetched per row from `get_schedule` on demand rather than N+1'd into the list
read.

**D-4 — `created`/`updated` on a schedule are preview-only, and that is a
payload fact rather than a choice.** `ScheduleRow` carries neither, nor `tags`.
The row *cannot* render them without widening the list read — which is precisely
the argument for having one generic surface that can.

## What shipped

- **`NodePreview.svelte`** — a `Comments` section and a `created`/`updated` line,
  for every kind. `updated` renders only when it differs from `created`; a second
  identical timestamp is noise that reads as information.
  - Keyed on `nodeId`. `Comments` already guards its own *fetch* across a reused
    instance, but not its *editor*: `newComment` and `editingId`/`editBuf` are
    plain state, and this panel changes node in place (Planning switches straight
    from one chip to another without closing). Without the key a half-typed draft
    follows you to the next node, and "Save" on an in-progress edit PATCHes a
    comment belonging to the node you just left.
- **Reading list** — the row id opens the preview (#1107). The id and not the
  title: on a reading list the title has one job, following the link, and taking
  that back would re-make the mistake WI #549 exists to record.
- **Schedules** — the run count expands into the history (#1105): date, work-item
  id (clickable into its preview), title, current status. Cache dropped and
  refilled on Materialise, since that is the one thing that adds to it.
- **Planning** — tag chips beside the project chip, and `queued <date>` (#1106).
- **Programs** — tags and `updated` in the detail header (#1106).
- **`domain.ts`** — `stamp` (lifted from the handoff page, which had the only
  copy) and a new `stampDay`.
- **`docs/node-shapes.md`** — per-kind tables re-checked, punch list struck
  through with what closed each item, new Hidden row for D-2.
- **`docs/usage.md`** — a fifth entry in "Behaviour common to every page". The
  generic comment surface belongs there rather than in a per-page bullet, which
  is the same argument the code makes.

### `stampDay` — the one non-obvious line

Deliberately **not** `iso.slice(0, 10)`, which is what the schedules page does to
`due_at`. That is correct there and would have been wrong here: `due_at`/
`anchor_at` arrive as `to_char(…, 'YYYY-MM-DD')` date text with no zone to get
wrong, whereas `created`/`updated`/`materialized` are RFC3339 instants in UTC.
Slicing one renders the **UTC** day — off by one for anybody west of Greenwich
for part of every day, on a feature whose entire purpose is judging whether
maintenance happened on time.

## Verification

`just check` green. Two new e2e specs (`node-preview-generic`, `schedule-runs`),
and the **full 65-test e2e suite run against a real korg-api + Postgres** — not
just the new specs, because `NodePreview` is shared by six surfaces and the
a11y, dialog-focus and theme-contrast suites are what would notice a slide-over
regression.

Both new surfaces were also confirmed visually rather than only by assertion:
the preview at `max-w-md` with a comment thread and an add box in it, and an
expanded run history on a schedules row.

Unlike sprint 054's headline fix, **#1105 was exercised against real data** — a
quarterly schedule seeded, materialised, and its history read back. That gap is
now closed for the schedules feature generally: 054 could not verify its preview
arm live because production held zero schedules.

## Follow-ups

- **Production still holds zero schedules** (054's closing note). #1105 renders
  evidence that, in production, has nothing to render — the quarterly restore
  drill `docs/operations.md` treats as the migration backstop still does not
  exist as a schedule. Ken's call, not a defect, and now the surface is ready
  for it.
- **Proposal 818 should stop sliding.** Carried forward from 1108's notes
  verbatim, because it is measured rather than cosmetic: ~400 of 532
  `search_sample` rows in klams-mind are the eval suite's own runs, so mining
  that log feeds the eval its own queries back. It ranks low only because it is
  legacy cross-project shape and cannot be started as-is — a tooling obstacle,
  not a low priority. Worth converting to a program.
- The `\x00none` sentinel in `planning/+page.svelte` (054's note) is still
  there; this sprint touched that file and did not trip over it, but it still
  makes plain `grep` skip the file silently.
- **`docs/usage.md`'s Web UI section has no bullet for Schedules, the Reading
  list, or Daily Reports.** Found while shipping this sprint, not caused by it:
  Schedules shipped in sprint 051 undocumented, and this sprint's run history
  had nowhere to be described. Not written here — three page bullets means
  documenting materialise semantics, dispositions and report findings from
  scratch, which is its own item rather than a ship-time drive-by. Worth
  filing.

## Deployed 2026-08-08

Image `kubsdb.encke-wahoo.ts.net:5000/korg:01de7bd78c96`
(digest `sha256:8ea6466a5fd6…`, revision
`01de7bd78c965d5b32cc0fec6c0a0980507fd043` — the squash-merge of PR #58).
Rollback target: `korg:9ef654695280` (sprint 054), confirmed present in the
registry before building.

Revision assertion passed in-deploy: the running container's
`org.opencontainers.image.revision` matches the commit built, so the
`compose pull` was real and not a cached no-op.

`post-deploy-check.sh --compare` clean. **Every row count unchanged** and
`migrations` still **26** — this sprint is pure rendering of payloads that
already existed, and the unchanged migration count is the deploy-side evidence
of exactly that.

Verified live beyond the fixed check, with a **read-only** browser pass — no
comment was posted and nothing was created, because the target is the real
database:

- **A proposal's preview carries its comments and timestamps** (#1104, #1106).
  Checked on proposal #439 in the live queue: tags `kyac`/`#post-mvp-review`/
  `#autonomy` render, `created 7/13/2026, 5:59:50 PM` renders, and the Comments
  section with its input is present. This is the sprint's headline and it is
  live on real data.
- **Planning cards carry `queued <date>` and tag chips** (#1106), on the live
  queue rather than a fixture.
- **A reading-list link opens a preview with a comment surface** (#1107), on one
  of the four real links.
- `/`, `/planning`, `/schedules`, `/reading-list`, `/programs` all 200.

**Not verified live: the schedule run history (#1105)** — the sprint's other
substantial change. Production now holds exactly one schedule (node 1109,
created earlier today, `cadence: once`) and its `materialized_count` is **0**,
so there is no history to expand and the disclosure correctly does not render.
The feature is covered by two e2e specs against a real korg-api — including the
materialise-with-history-open cache path — but it has not been exercised
against production data, and this record should not imply otherwise.

That is the second sprint running where a schedules change could not be
confirmed live. 054 noted production held *zero* schedules; there is now one,
never materialised. The quarterly restore drill that motivated #581 — and that
`docs/operations.md` treats as the backstop for a bad migration — still does not
exist as a schedule. Ken's call, not a defect.

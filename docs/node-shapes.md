# Node-shape audit — is every field viewable somewhere?

WI #870 asked for a sweep: *"review shapes of all nodes to ensure that we have
UI showing them (will likely be some exceptions, but in general, most fields
should be viewable somehow)"*. Run in sprint 054, deliberately last in its
program so it audited the shape sprints 050–053 left behind rather than one
about to change. Its punch list was worked and closed in sprint 055; this
document is kept current with both.

**The deliverable is not "everything is visible".** It is "we know what isn't,
and why" — Ken's own hedge about exceptions is the point. The column that earns
this document is **Hidden**: those decisions previously existed only as an
absence, indistinguishable from an oversight. The **Missing** column is the
punch list.

Node kinds come from `vocab::NODE_KINDS`, which since this sprint is pinned to
the `node_kind_check` constraint by `node_kinds_match_the_check_constraint`
(`crates/korg-core/tests/schema.rs`). Enumerate from there, never from memory —
that is the whole lesson below.

## How this audit found its own cause

`schedule` was added by migration 0025 in sprint 051. `get_node_preview` never
got an arm for it, so for three sprints every find-by-ID and every `materializes`
edge rendered **`schedule #1042`** — the newest node type in korg, and the only
one with no detail rendering at all.

Two guards should have caught it and both reported green:

1. **`every_node_kind_resolves_to_a_real_title`** (sprint 049) was written for
   precisely this failure, and its doc comment claimed it covered "every kind the
   `node.kind` check constraint admits". It iterated a hardcoded array of seven
   ids. The constraint had eight entries.
2. **`a_schedule_is_an_ordinary_node_for_the_generic_surfaces`** (sprint 051)
   called `get_node_preview` on a schedule and asserted `preview.kind ==
   "schedule"` — a field the *fallback path* sets too, from the base query. The
   assertion could not fail. It read as coverage and proved nothing.

The root cause is one sentence: **node kinds were the only closed set in korg
not in `vocab.rs`.** Every other vocabulary has lived there since #526 behind a
partition test. Kinds lived in whichever migration last touched the CHECK, plus
hand-written copies in two tests — and the copies did not grow when the
constraint did.

The fix is a chain of three failures, so the next kind cannot arrive quietly:

```
migration widens node_kind_check
   └─ node_kinds_match_the_check_constraint  fails until vocab::NODE_KINDS grows
        └─ every_node_kind_resolves_to_a_real_title  fails until it can build one
             └─ …and until get_node_preview renders it with a real title
```

Verified by removing the new `schedule` arm and watching the fence fail with
`left: "schedule #10"` — the exact string that shipped for three sprints.

## The generic surface

Most "is this viewable?" questions resolve to `get_node_preview`
(`repo/preview.rs`) → `NodePreview.svelte`, the slide-over reached from find-by-ID on
Work Items, from any edge chip, and from the id/title affordances on Planning,
Schedules and the Reading list. It renders, for every kind: `kind`, `node_id`,
`title`, `project`, `tags`, `archived`, `badges[]`, `fields[]` (label/value),
`body` + `body_label` (markdown), `details`, **`created`/`updated`**, and
**comments**.

The last two arrived in sprint 055, and they are the point of this section
rather than a detail of it. Punch-list items 1, 3, 4 and 5 below were four
symptoms of one mismatch: `add_comment`/`list_comments` accept any node id and
`node.tags`/`created`/`updated` exist for every kind — the **data model was
already generic and only the UI was per-kind**. `schedule` is the proof that the
mismatch regenerates: it landed in sprint 051 brand new and arrived with every
one of these gaps. Rendering the node-level columns once, here, means the next
kind gets them by existing rather than by being wired up.

Where that reasoning stops: what a *list row* shows is a density decision, not a
correctness one, and is taken per surface. See the per-kind tables.

Node-level `category` is deliberately absent from it — see Hidden, below.

## Per-kind field coverage

Legend: **✓** rendered on a dedicated page · **≈** rendered in the generic
preview only · **H** deliberately hidden (reason given) · **✗** missing.

### workitem — `/work-items`

| Field | | Where |
|---|---|---|
| `wi_number`, `title`, `wi_type`, `wi_status`, `wi_tshirt` | ✓ | list + detail |
| `project`, `area`, `sprint`, `parent` | ✓ | detail |
| `content`, `details` | ✓ | detail (markdown) |
| `tags` | ✓ | detail + filter chips |
| `archived` | ✓ | row styling + badge |
| `comment_count`, comments | ✓ | `RowTickers` + `Comments.svelte` |
| `has_handoff` | ✓ | `RowTickers` |
| `proposal_node_id` | ✓ | "Prop" column (#824) |
| `created`, `updated` | ✓ | detail |
| `category` | H | node-level column; work items route by `project`/`area`, and the rail colours from the *project's* category. A per-item category would be a second, competing routing story. |

The most-covered kind in korg, and the only one where every field is on one page.

### sprint_proposal — `/planning`

| Field | | Where |
|---|---|---|
| `title`, `summary`, `status`, `pinned`, `project` | ✓ | card |
| `notes` | ✓ | card, in a `<details>` (#860) |
| `rank` | ✓ | implicitly — the card's position, and drag reorders it |
| `covered` work items | ✓ | chips, each opening its own preview |
| `related` edges (`has_handoff`, `depends_on`) | ✓ | chips (LB-3) |
| `node_id` | ✓ | "Sprint Plan ID" |
| `covered_count` | ≈ | `Covers` field in the preview (sprint 054) |
| `archived` | ≈ | preview badge |
| `comment_count`, comments | ✓ | preview (generic, sprint 055) |
| `tags` | ✓ | card chips + preview (sprint 055) |
| `created` | ✓ | card, as `queued <date>` (sprint 055) |
| `updated` | ≈ | preview, when it differs from `created` |

Sprint 054 added the **proposal's own detail pop-out**: every *other* node on a
Planning card could be previewed — covered items, handoffs, related edges — and
the proposal could not. The one shape the page is about was the one with no way
to open it. The title is now the affordance.

Sprint 055 hung comments off it (#1104), which the audit called its largest real
gap and it was: the pre-sprint premise check leaves its findings here, so korg
was accepting writes on the assumption someone would read them and offering
nobody a way to. `created` is on the *card* and not only in the preview because
the queue is ordered by rank, so nothing else on the surface says how long
something has been sitting in it.

### schedule — `/schedules`

| Field | | Where |
|---|---|---|
| `preview_title`, `cadence`, `anchor_mode`, `due`, `due_at`, `status` | ✓ | row |
| `last_wi_number`, `outstanding`, `materialized_count` | ✓ | row |
| `title` (raw template), `template`, `notes`, `wi_type`, `wi_tshirt`, `anchor_at` | ≈ | preview (sprint 054) |
| `project` | ✓ | row chip |
| `archived`, `tags` | ≈ | preview |
| `materialized[]` history | ✓ | row, expanded from the count (sprint 055) |
| `comment_count`, comments | ✓ | preview (generic, sprint 055) |
| `created`, `updated` | ≈ | preview |

Was the worst-covered kind in korg; is now roughly average. `due`/`due_at` are
deliberately **not** recomputed in the preview — the derivation lives in
`SCHEDULE_DUE_SQL` and rendering a second thinner copy in a slide-over is exactly
the drift that constant exists to prevent.

`created`/`updated` are preview-only here and that is a payload fact, not a
choice: `ScheduleRow` carries neither, nor `tags`, so the row *cannot* render
them without widening the list read — which is precisely the argument for having
one generic surface that can. The run history went the other way, onto the row,
because it is read against the cadence sitting beside it.

### program — `/programs/[node_id]`

| Field | | Where |
|---|---|---|
| `title`, `aim`, `notes`, `status`, `pinned` | ✓ | page |
| `slices`, `span` | ✓ | page (span derived, D-6) |
| `comment_count`, comments | ✓ | page |
| `rank` | ✓ | ordering |
| `tags`, `updated` | ✓ | page header (sprint 055) |
| `created` | ≈ | preview |
| `category` | ✗ | — |
| `archived` | ≈ | preview badge |

Tags and `updated` are on the detail page and deliberately **not** on the list
rows: `/programs` is a routing surface — pick one and go — where the detail page
is the reading one, and a program is long-lived enough that how recently it moved
is part of reading it.

### report — `/daily-reports`

| Field | | Where |
|---|---|---|
| `source`, `report_date`, `status`, `summary`, `body` | ✓ | page |
| `model`, `escalated` | ✓ | page |
| `findings[]` | ✓ | page, linked to work items |
| comments | ✓ | page |
| `tags`, `category` | ✗ | — |

Report **sources** (`SourceHealth`: `freshness`, `asserts`, `cadence_days`) are
rendered on the board rollup, not here. Note the deliberate absence of
`last_status` from that projection entirely (#950) — a field that does not exist
so that it cannot be rendered.

### handoff — `/handoffs/[node_id]`

| Field | | Where |
|---|---|---|
| `title`, `summary`, `body` | ✓ | page + preview |
| `related` (what it is attached to) | ✓ | page |
| comments | ✓ | page |
| `updated`, `category` | ✓ | page |
| `tags` | ✗ | — |

### card — `/cards`

| Field | | Where |
|---|---|---|
| `title`, `description`, `status`, `rank` | ✓ | board |
| `project`, `category`, `tags`, `archived` | ✓ | board |
| `created`, `updated`, comments | ✓ | board |

Fully covered.

### link — `/reading-list`, `/link-up`

| Field | | Where |
|---|---|---|
| `url`, `title`, `read`, `disposition` | ✓ | list |
| `category`, `tags`, `archived`, `created` | ✓ | list |
| comments | ✓ | preview, opened from the row id (sprint 055) |

The **id** opens the preview, not the title. On a reading list the title has one
job — following the link (#549) — and taking that back to open a panel would
re-make the mistake that WI exists to record.

### attachment — no page yet (sprint 056)

| Field | | Where |
|---|---|---|
| `filename`, `img_id` | ≈ | preview title + badge |
| `state` (`pending`/`linked`) | ≈ | preview badge |
| `mime`, `width`/`height`, `byte_size` | ≈ | preview fields |
| `url`, variant urls | ≈ | preview fields (as links, not as images) |
| `project`, `tags`, `archived`, `created`, `updated`, comments | ≈ | generic preview |
| `content_hash`, `owner_node_ids` | ✗ | API only — the hash is recorded for future dedup *detection* and has no reader; owners are the `has_attachment` edges, which the preview already shows |
| inline rendering, thumbnails, lightbox | ✗ | **slice 2** (`korg:1124`, #1120/#1121) |

The ninth kind, and the first to arrive knowing this document exists — which is
the section's only real finding. `schedule` landed in sprint 051 with every
generic gap open; `attachment` landed with none of them, because sprint 055 made
them generic. Everything above the line came free.

What is deliberately **not** here is an image. `NodePreview` renders markdown
bodies, and a screenshot is not markdown; giving it an `<img>` affordance is
slice 2's design call, made once, with the lightbox that has to sit behind it.
Until then the variant URLs are fields — enough to open one in a browser tab,
and honest about being a stopgap rather than a half-built viewer.

## Deliberately hidden — the column that matters

These are decisions, not gaps. Changing one is a design change.

| What | Why |
|---|---|
| Node-level `category` in `NodePreview` | Category is a *routing* attribute consumed by the rails and their colours (#678). In a slide-over it would be a chip with no filter behind it — decoration that implies a control. |
| `due`/`due_at` in the schedule preview | One definition (`SCHEDULE_DUE_SQL`), rendered where it has context. A second derivation is how the two go out of step. |
| `last_status` on a report source | Not hidden — *absent by design* (#950). korg served a stale GREEN for eleven days in July 2026 because the thing checking the fleet was broken. A consumer holding the field will eventually render it. |
| Raw `title` on a schedule row | `preview_title` is what Materialise will actually create. The template is shown in the preview only when the two differ, so the substitution is never invisible but never noise either. |
| `rank` as a number, everywhere | Rank is a `Decimal` for midpoint insertion. Its *value* is meaningless to a reader; its *order* is the whole content, and drag-and-drop renders it. |
| `created`/`updated` on most list rows | Generic in the preview since sprint 055, so the question is always answerable; putting both on every row of every surface is density with no reader. The exceptions are argued, not defaulted: `queued` on a Planning card (the queue is rank-ordered, so nothing else says how long something has waited) and `updated` on a program header (long-lived enough that recency is part of reading it). |
| `closed` work items by default | `WI_TERMINAL_STATUSES` — 78% of the corpus, Ken's own "I am finished" marker. Reachable via the status filter. |
| `wi_number` vs `node_id` | The same number since 0009. The UI shows `#N` for work items and `node #N` otherwise, so the identity is one fact with two spellings, not two facts. |

## Punch list — closed in sprint 055

Filed as korg work items rather than fixed in sprint 054 — the notes for that
sprint timeboxed the enumeration and let the punch list carry the remainder.
All five items shipped together, because four of them turned out to be one fix.

1. ~~**Comments are unreachable for proposals and schedules.**~~ #1104 — closed
   by mounting `Comments` in `NodePreview` for every kind, not by wiring two more
   pages. The largest real gap the audit found.
2. ~~**A schedule's materialisation history is count-only.**~~ #1105 — closed on
   the row: the count is now the disclosure that opens it.
3. ~~**`tags` are invisible on proposals, programs and handoffs.**~~ #1106 —
   handoffs already had them; proposals got card chips, programs a header, and
   every kind the preview.
4. ~~**`created`/`updated` are absent from proposals, programs and schedules.**~~
   #1106 — generic in the preview, plus `queued <date>` on the Planning card.
5. ~~**Links have no comment surface.**~~ #1107 — closed by item 1, which is why
   the four were bundled into one proposal rather than worked separately.

The lesson worth keeping is the shape of the list, not its contents: **four of
five items were the UI failing to be as generic as the data model already was.**
A punch list that reads like this next time is one fix, not *n*.

## Maintaining this document

`docs_drift` (korg-mcp) does **not** check this file — it asserts the tool
catalogue, the README tool count and the collection-shape table, and extending it
to prose tables of UI coverage would be a fence with a much worse
false-positive rate than value.

What is machine-checked is the part that failed silently: the kind vocabulary,
by the three-failure chain above. Adding a node type will force you here; the
per-field rows are a periodic sweep, and dating the sweep is honest where
pretending to automate it would not be.

*Swept: sprint 054 (2026-08-08), against `NODE_KINDS` as of migration 0026.*
*Punch list closed: sprint 055 (2026-08-08); per-kind rows re-checked with it.*

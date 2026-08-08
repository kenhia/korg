# Node-shape audit — is every field viewable somewhere?

WI #870 asked for a sweep: *"review shapes of all nodes to ensure that we have
UI showing them (will likely be some exceptions, but in general, most fields
should be viewable somehow)"*. Run in sprint 054, deliberately last in its
program so it audited the shape sprints 050–053 left behind rather than one
about to change.

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
(`repo.rs`) → `NodePreview.svelte`, the slide-over reached from find-by-ID on
Work Items, from any edge chip, and from the id/title affordances on Planning and
Schedules. It renders, for every kind: `kind`, `node_id`, `title`, `project`,
`tags`, `archived`, `badges[]`, `fields[]` (label/value), `body` + `body_label`
(markdown) and `details`.

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
| `covered_count` | ≈ | `Covers` field in the preview (added this sprint) |
| `archived` | ≈ | preview badge |
| `comment_count`, comments | ✗ | **no UI anywhere.** See punch list. |
| `tags` | ✗ | on the node, rendered on no proposal surface |
| `created`, `updated` | ✗ | — |

Sprint 054 added the **proposal's own detail pop-out**: every *other* node on a
Planning card could be previewed — covered items, handoffs, related edges — and
the proposal could not. The one shape the page is about was the one with no way
to open it. The title is now the affordance.

### schedule — `/schedules`

| Field | | Where |
|---|---|---|
| `preview_title`, `cadence`, `anchor_mode`, `due`, `due_at`, `status` | ✓ | row |
| `last_wi_number`, `outstanding`, `materialized_count` | ✓ | row |
| `title` (raw template), `template`, `notes`, `wi_type`, `wi_tshirt`, `anchor_at` | ≈ | preview (added this sprint) |
| `project` | ✓ | row chip |
| `archived`, `tags` | ≈ | preview |
| `materialized[]` history | ✗ | `ScheduleDetail` carries it; only the *count* is rendered |
| `comment_count`, comments | ✗ | **no UI anywhere** |
| `created`, `updated` | ✗ | — |

Was the worst-covered kind in korg; is now roughly average. `due`/`due_at` are
deliberately **not** recomputed in the preview — the derivation lives in
`SCHEDULE_DUE_SQL` and rendering a second thinner copy in a slide-over is exactly
the drift that constant exists to prevent.

### program — `/programs/[node_id]`

| Field | | Where |
|---|---|---|
| `title`, `aim`, `notes`, `status`, `pinned` | ✓ | page |
| `slices`, `span` | ✓ | page (span derived, D-6) |
| `comment_count`, comments | ✓ | page |
| `rank` | ✓ | ordering |
| `category`, `tags` | ✗ | — |
| `archived` | ≈ | preview badge |

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
| comments | ✗ | `list_comments` accepts any node; no link surface offers them |

## Deliberately hidden — the column that matters

These are decisions, not gaps. Changing one is a design change.

| What | Why |
|---|---|
| Node-level `category` in `NodePreview` | Category is a *routing* attribute consumed by the rails and their colours (#678). In a slide-over it would be a chip with no filter behind it — decoration that implies a control. |
| `due`/`due_at` in the schedule preview | One definition (`SCHEDULE_DUE_SQL`), rendered where it has context. A second derivation is how the two go out of step. |
| `last_status` on a report source | Not hidden — *absent by design* (#950). korg served a stale GREEN for eleven days in July 2026 because the thing checking the fleet was broken. A consumer holding the field will eventually render it. |
| Raw `title` on a schedule row | `preview_title` is what Materialise will actually create. The template is shown in the preview only when the two differ, so the substitution is never invisible but never noise either. |
| `rank` as a number, everywhere | Rank is a `Decimal` for midpoint insertion. Its *value* is meaningless to a reader; its *order* is the whole content, and drag-and-drop renders it. |
| `closed` work items by default | `WI_TERMINAL_STATUSES` — 78% of the corpus, Ken's own "I am finished" marker. Reachable via the status filter. |
| `wi_number` vs `node_id` | The same number since 0009. The UI shows `#N` for work items and `node #N` otherwise, so the identity is one fact with two spellings, not two facts. |

## Punch list

Filed as korg work items rather than fixed here — the notes for this sprint
timeboxed the enumeration and let the punch list carry the remainder.

1. **Comments are unreachable for proposals and schedules.** Both kinds carry
   `comment_count` and inline comments in their focused reads; neither has any UI
   for them. `Comments.svelte` already exists and is used by five other surfaces,
   so this is wiring, not design. The largest real gap found.
2. **A schedule's materialisation history is count-only.** `ScheduleDetail
   .materialized[]` answers "when was this drill *actually* run?" — the question
   `last_wi_number` cannot — and nothing renders it.
3. **`tags` are invisible on proposals, programs and handoffs.** Rendered for
   work items, cards and links; the node column exists for all of them.
4. **`created`/`updated` are absent from proposals, programs and schedules.**
   Minor, but "when was this queued?" currently needs an MCP call.
5. **Links have no comment surface.**

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

# 034 — Stop the bleeding: survey's archived default, list_proposals' unbounded read

**Proposal:** korg:865 (rank 4.1) — covers **#851** and **#852**.
**Program:** agent-surface (`sprints/planning/2026-08-korg-agent-surface.md`), §4 rank 1.

## Goal

The two measured T2 traps, fixed at source. Both are cases where korg's own
MCP `instructions` mislead an agent that cannot check them, so neither can be
left to the schemas.

- **#851** — `survey_work_items` includes archived rows when `archived` is
  omitted, while the instructions say *"All exclude archived rows unless you
  ask for them."* Measured on project `klams`: omitted → 105, `archived:false`
  → 104. Silent over-count in the exact tool the instructions recommend for
  sizing a backlog.
- **#852** — `list_proposals` returns every row's full `summary` with no
  `limit` and no projection: 110 rows / ~185,500 chars / **~46k tokens**
  unfiltered, 71% of it `done`.

## Decisions

### D-1 — the survey gets the standard `ArchivedFilter`, not a documented exception

#851 offered three options; the plan's §4 picks "flip the default" and the WI
argues (1)+(3). `SurveyWorkItems.archived` becomes the same
`ArchivedFilter` + `archived_default()` every sibling read uses. The point is
not just a better default — it is that the survey stops being a *special case*.
The doc comment claiming the difference was "on purpose" goes with it.

The REST half was the same bug: `SurveyQuery.archived` was a raw
`Option<bool>` and the only collection read not going through
`parse_archived`. Now it does, so `?archived=all` works there like everywhere
else.

### D-2 — `omitted` on the survey, because the failure inverts

Flipping the default converts a silent over-count into a silent *under*-count.
`WorkItemSurvey` gains `omitted: { archived }` — the archived rows the filter
hid, matching `list_projects`' `ProjectOmitted` (#828). A survey used to decide
"is this project drained?" can now tell a narrowed answer from a complete one.

`omitted.archived` counts **archived rows the filter excluded**, so it is 0
under `archived: true` and `archived: null`. Naming it for what it counts keeps
the field honest under every setting.

### D-3 — `list_proposals` gets a lean projection *and* the terminal-status default

#852's options (2) and (3) compose, and the plan's §8 answer 1 already decided
the vocabulary: proposals exclude `done` + `declined` by default. Doing only
the status default leaves ~13k tokens on the table; doing only the projection
still ships every `done` row's title. Together the common call is ~1k tokens.

Defaults, mirroring `project_status_predicate` (#828) exactly:

| `status` | rows |
|---|---|
| absent | `proposed` + `active` — the live queue |
| `"all"` | every status |
| one of the four | exactly that status |

`archived` is the standard `ArchivedFilter` (default excludes). `list_proposals`
had **no** archived predicate at all — the second read the instructions'
"all exclude archived" sentence was false about.

### D-4 — `detail: "lean" | "full"`, not a new `survey_proposals` tool

#852's option 2 named a new tool. `list_projects` already solved the same
problem with a `detail` parameter on the existing read (#828), and reusing that
shape costs no catalogue entry, no README tool-count change, and no second name
for agents to learn. `full` returns today's `ProposalRow` — nothing becomes
unreachable, it just stops being the default.

The lean row is #852's list verbatim: `node_id`, `title`, `status`, `project`,
`rank`, `pinned`, `covered_count`, `comment_count`. No `summary` — `get_proposal`
is the full read and `start-sprint` calls it immediately after the pick anyway.

### D-5 — `omitted` cascades, so nothing is double-counted

`ProposalOmitted { done, declined, archived }` is computed as a cascade over the
`project`-filtered rows: `archived` first (what the archived filter hid), then
`done`/`declined` counted **only over rows that passed the archived filter**. An
archived `done` proposal therefore lands in `archived` and nowhere else.

### D-6 — MCP goes lean; REST stays as it is

`repo::list_proposals` is untouched and still serves `GET /api/proposals`. The
Planning page deliberately fetches unfiltered (WI #622) and renders `done` and
`declined` columns from summaries — it is a browser, not a token budget. This
is the same split `list_projects` already has (`repo::list_projects` for REST,
`list_projects_lean/full` for MCP), and `docs/api.md`'s shape table already
describes the MCP surface.

### D-7 — make the instructions sentence *true*, and fence it

#851 closes with "the server-instructions sentence must stop being categorically
false." After D-1 and D-3 the remaining liars are the bare-array reads:
`list_reports` (reports are nodes and can be archived), `list_daily_plan`,
`list_areas` and `list_comments` (no archived state to filter). Rather than
quietly widen this sprint into a fifth read's behaviour change, the sentence
now names that class:

> the unpaginated ones (`list_reports`, `list_areas`, `list_comments`,
> `list_daily_plan`) return a bare array and have no archived filter; … every
> other collection read excludes archived rows by default

And a new `docs_drift` test asserts it against the **derived schemas**: a
collection read named in that list must have no `archived` property, and every
other one must have it. The one-time correction is worth less than the fence —
that is the failure mode #851 is really about.

`list_proposals` moving to `{items, omitted}` also moves it out of the bare-array
class in both texts, which the existing `docs_drift` /  `dispatch` fences require
to happen together.

## What shipped

**#851 — `survey_work_items`**

- `ops::SurveyWorkItems.archived` is now `ArchivedFilter` + `archived_default()`,
  identical to `list_work_items`' — the doc comment defending the special case is
  gone with it.
- `WorkItemSurvey` gains `omitted: {archived}`, counted in the same round trip
  and skipped entirely when the filter can't hide anything.
- REST `SurveyQuery.archived` moved from a raw `Option<bool>` to
  `parse_archived`, so `?archived=all` works and omitting it means live-only.
  The Review page already passed `archived: false` explicitly, so no UI change.

**#852 — `list_proposals`**

- `repo::list_proposals_lean` / `list_proposals_full` + `ProposalLeanRow`,
  `ProposalOmitted`, `proposal_status_predicate`. MCP dispatches on `detail`.
- `ops::ListProposals` gains `archived` and `detail`; its `status` enum gains
  `"all"`.
- `vocab::PROPOSAL_LIVE_STATUSES` / `PROPOSAL_TERMINAL_STATUSES`, with
  `proposal_statuses_partition_cleanly` fencing that a fifth status can't be
  added into neither half — invisible *and* uncounted is the failure `omitted`
  exists to stop.

**Contract texts and their fences**

- `server_instructions()`: `list_proposals` moved to the filtered class,
  `survey_work_items`' `omitted` stated, and the archived sentence rewritten from
  the categorically-false "All exclude archived rows unless you ask for them".
- `docs/api.md`: shape table, filters table, a new "The proposal queue narrows by
  default" section, and the archived-affordance split. `docs/usage.md`: the
  survey's REST filters and the second `{items, omitted}` read.
- New `docs_drift::the_archived_affordance_matches_the_instructions` — asserts
  the split against the derived input schemas. `list_projects` is handled
  explicitly rather than exempted: a project is not a node, so it spells the same
  default through `status`, and the test asserts that enum still offers
  `archived` and `all`.
- `docs_drift` now compares the `{items, omitted}` row too (it only checked
  paginated and bare), and `dispatch` asserts the filtered class's wire shape and
  the survey's `omitted` rather than only classifying them.
- `advertised_defaults_match_server_behaviour`: the F-11 assertion is inverted.
  It used to require the survey advertise *no* archived default — reconciling
  schema to odd behaviour. It now requires the survey's `archived` schema be
  byte-identical to `list_work_items`'.
- `schema.rs`: the two narrow-by-default status filters assert vocabulary + `all`
  (`list_projects`' was previously unfenced).

**Behaviour tests** (`korg-mcp/tests/server.rs`): both new suites drive the real
tool surface — the survey's three archived settings against `total` and
`omitted`, and the queue's default/`all`/single-status/`full`/archived-only paths
against a fixture with one of each status plus an archived one.

`just check` green: fmt, gen-check, web checks, clippy `-D warnings`, full suite.

## Deployed 2026-08-01

- PR **#38**, squash-merged as `58afde4`. Image `korg:58afde4af036`
  (`org.opencontainers.image.revision` confirmed on the running container).
  Rollback target: `korg:c5f756f3d90a`.
- `post-deploy-check.sh --compare`: every count identical to the pre-deploy
  baseline (`node_count` 782, `node_max` 869, work items 597, proposals 119,
  migrations 20 → 20). **No schema migration in this deploy** — the whole sprint
  is MCP surface.
- Backups current at deploy time: `korg-20260801-032006.sql.gz` (685 KB, larger
  than the prior night's 589 KB), timer active.

### Verified live, against production data

**#851** — `survey_work_items { project: "klams" }`, the WI's own measurement,
re-run on the deployed instance (the corpus has grown since it was filed, so the
numbers are 109/108 rather than 105/104 — the *relationship* is the fix):

| call | `total` | `omitted.archived` |
|---|---|---|
| `archived` omitted | **108** | 1 |
| `archived: false` | 108 | 1 |
| `archived: null` | 109 | 0 |
| `archived: true` | 1 | 0 |
| `list_work_items` (for reference) | 108 | — |

Omitting the filter now agrees with `archived:false` and with `list_work_items`,
which is exactly the disagreement #851 was. `omitted.archived` accounts for the
one row the default hides, and is 0 under both settings that hide nothing.

**#852** — `list_proposals` payload, same instance:

| call | bytes |
|---|---|
| default (lean, live queue) | **4,001** |
| `status:"all"` (lean) | 21,781 |
| `status:"all", detail:"full", archived:null` | **234,580** |

The default call is **~59× smaller** than the equivalent old read — bigger than
the ~46k-token figure #852 measured, because the corpus grew in the meantime.
20 live rows returned; `omitted` was `{done: 91, declined: 3, archived: 5}`, and
20 + 91 + 3 + 5 = 119 = the count `GET /api/proposals` still returns, so the
cascade adds up with nothing double-counted or lost.

Also confirmed live: the lean row's key set is exactly D-4's list; `detail:"full"`
restores `summary`; the advertised schemas carry `archived.default: false` on both
tools, `detail: ["lean","full"]`, and `status` with `"all"`; the `initialize`
instructions no longer contain "All exclude archived rows unless you ask for
them" and do name the no-archived-filter class; `/plan` returns 200; REST
`/api/proposals` still returns its 119-element bare array; and
`GET /api/work-items/survey?archived=all` now answers 200 where it used to be a
400.

## Follow-ups

- #861 (rank 4.3) retires `survey_work_items` in favour of a lean
  `list_work_items`. #851 is fixed here in the transitional tool by design.
- #860 (rank 4.3) bounds proposal `summary` and adds `notes`; the lean
  projection stands regardless — the pick-one-from-a-list case never wanted
  prose.
- `list_reports`' missing archived filter is now *stated* rather than fixed.
  Worth a WI if archived reports ever accumulate.

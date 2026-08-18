# API contracts

Normative reference for korg's agent-facing surfaces. Where this document and
a tool description disagree, this document is right and the description is a
bug.

Error codes and the mutation contract live in
[usage.md](usage.md#response-and-error-contract); the tool catalogue, collection
reads and the relationship model are below.

## Tool catalogue

Every tool the MCP endpoint exposes, by category. Names only — each tool's
input schema and description are derived from `korg-core` and delivered by
`tools/list`, which is the place to read them rather than a copy here that can
disagree.

This table is the **one** normative list. `README.md` states the count and
points here; the MCP server `instructions` name the categories; nothing
enumerates the tools a third time. All three are drift-tested against
`korg_mcp::tools::tools()` by `crates/korg-mcp/tests/docs_drift.rs`.

| Category | Tools |
|---|---|
| Work items | `create_work_item`, `get_work_item`, `update_work_item`, `list_work_items`, `work_item_flow` |
| Cards | `create_card`, `update_card`, `list_cards` |
| Comments | `add_comment`, `list_comments`, `update_comment`, `delete_comment` |
| Reading-list links | `create_link`, `list_links`, `update_link`, `delete_link`, `mark_link_read` |
| Relationships | `relate`, `unrelate`, `neighbors` |
| Sprint proposals | `propose_sprint`, `list_proposals`, `get_proposal`, `update_proposal` |
| Programs | `create_program`, `get_program`, `list_programs`, `update_program` |
| Awaiting Ken | `set_awaiting`, `list_awaiting` |
| Board | `get_board` |
| Search | `search` |
| Reports | `create_report`, `list_reports`, `get_report`, `list_report_sources`, `set_report_source` |
| Schedules | `create_schedule`, `get_schedule`, `list_schedules`, `update_schedule`, `materialize_schedule` |
| Handoffs | `create_handoff`, `get_handoff`, `update_handoff` |
| Attachments | `get_attachment`, `list_attachments` |
| Projects and areas | `list_projects`, `get_project`, `create_project`, `update_project`, `list_areas`, `create_area`, `update_area`, `delete_area` |

Two tools are not what their names suggest:

- `mark_link_read` is **deprecated** — `update_link` sets the read flag,
  disposition and tags in one transaction, which is what an agent recording a
  decision about a captured URL actually wants.
- `create_report` upserts: a re-run for the same `(source, date)` replaces the
  previous run's `finding` edges transactionally rather than accumulating them
  (D-7).

`survey_work_items` was a third until sprint 038. #861 folded its projection
into `list_work_items` and kept the name as an alias for one deprecation
window; #867 moved the last caller (`refill-queue`) off it, and #871 deleted
it. Three work-item reads are two again. The REST `GET /api/work-items/survey`
route is unaffected — it is the Review page's endpoint and never shared the
alias's argument type.

`update_project` takes `status`, `machines`, `deploy_to`, `category`,
`description`, `gh_repo` and `src_path` — everything but the name. `src_path` is
load-bearing: it is how an agent finds a project's working copy on disk.

`src_path` was called `cn_path` until sprint 031 (WI #675). The rename settles a
question the old name left open: a bare path means nothing without a host, and
once `machines` and `deploy_to` split development from production, `cn_path`
never said which one it described. **`src_path` is the working copy on the
project's development machine** — its `machines` entry, never its deploy target.
korg's own row is the worked example: `machines: ["kai"]`, `deploy_to:
["kubsdb"]`, and `src_path` is the kai checkout.

Its canonical form is `~`-relative, no trailing slash, no whitespace and no
parentheses — a path and nothing else. Migration 0019 enforces that with the
`project_src_path_canonical` CHECK constraint, added `NOT VALID` so that one
legacy row holding a sentence of archive history is tolerated until the pass
that owns it resolves the prose; every write from sprint 031 on is checked.

## Where the contract lives

Since sprint 016 there is **one** definition of every request shape, and the
other surfaces are derived from it:

| Surface | Source |
|---|---|
| REST request bodies | the `korg-core` struct itself — `NewWorkItem`, `WorkItemPatch`, `NewCard`, `CardPatch`, `NewLink`, `LinkPatch`, `NewProposal`, `ProposalPatch`, `ProjectPatch`, `NewReport`, plus the operations in `korg_core::ops` |
| MCP tool input schemas | derived from those same structs via `schemars`; enum lists come from `korg_core::vocab` |
| TypeScript in `web/src/lib/generated/` | derived from the response rows via `ts-rs`, plus the vocabularies |

Both transports deserialize the *same* type, so REST and MCP cannot accept
different fields for the same operation. `korg-mcp` carries the target id in
the argument object and `korg-api` carries it in the path; the MCP schema is
the union of the derived id-selector and body schemas.

Two things are deliberately *not* shared. Collection **filters** differ by
encoding — a query string cannot carry a JSON `null`, so REST spells the
tri-state `archived` as `true|false|all` while MCP spells it as a nullable
boolean; both resolve to the same core query type. And `additionalProperties:
false` in the tool schemas remains advisory: the server ignores unknown fields
rather than rejecting them, as it always has.

Regenerate with `just gen`; `just check` fails if the committed output is
stale. See [setup.md](setup.md#generated-files).

## Selecting a project or an area

Writes that target a project take **either** `project_id` **or** `project` (the
name); writes that target an area take **either** `area_id` **or** `area`. This
applies to `create_work_item`, `update_work_item`, `create_card`, `update_card`,
`create_link` and `propose_sprint`, on both transports.

Three rules, and each exists because of a specific failure:

- **Never both.** Passing an id *and* a name is `invalid_input`, even when they
  agree. There is deliberately no precedence rule: a precedence rule silently
  discards one of two things the caller explicitly asked for.
- **Resolve, never create.** An unknown name is an error. Creating a project is
  `create_project`'s job — `update_card` used to create one as a side effect of
  a card edit, which sprint 015 removed (WI #537) and this does not bring back.
- **Errors name the remedy.** An unresolvable name points at `list_projects` /
  `list_areas`; a name that differs only in case is answered with the real one
  ("did you mean 'korg'?"). Same principle as the vocabulary errors: the error
  doubles as the documentation needed to retry.

`null` means the same thing on both spellings: on a patch, `"project": null`
unassigns exactly as `"project_id": null` does.

Area names are unique only *within* a project, so `area` resolves against the
project the row will have after the operation — pass `project`/`project_id` in
the same call when moving and re-tagging at once. An `area` with no project at
all is `invalid_input` rather than a lookup that mysteriously finds nothing.

Every unresolvable selector answers `invalid_input`, including a `project_id`
or `area_id` that does not exist. Those used to reach the foreign key and
surface as a raw database error in a 500 (`project_id`) or claim `not_found`
as though the *work item* were missing (`area_id`). The rule is uniform now:
the operation's own target missing is `not_found`; a selector that does not
resolve is `invalid_input`.

## Collection reads

The **paginated** lists return an envelope (sprint 015):

```json
{ "items": [ ... ], "total": 42, "limit": 200, "offset": 0 }
```

`total` is the full **filtered** count before `limit`/`offset`, so you can page
without guessing and can tell a complete answer from a clipped one. `limit`
defaults to 200 and is clamped to 500.

That is true on **every** page, including one whose `offset` lands past the last
row: `items` comes back empty and `total` still reports the corpus. Until sprint
038 it was not (WI #883) — `list_work_items` computed `total` with a
`count(*) OVER()` riding on the returned rows, so overshooting produced
`total: 0`, breaking `remaining = total - offset` and "trust the last page's
total" at exactly the moment a pager relies on them. `omitted` stayed correct
throughout, which is what made the lie easy to miss.

The unpaginated lists return a **bare JSON array**. Which is which, verified
against the code in sprint 020 — this table was previously "every list returns
the same envelope", which was true of four of them:

| Shape | Reads |
|---|---|
| `{items, total, limit, offset}` | `list_work_items`, `list_cards`, `list_links`, `search` |
| `{items, total, limit, truncated}` | `neighbors` (`truncated`, not `offset` — it caps rather than pages) |
| bare array | `list_reports`, `list_areas`, `list_comments`, `list_awaiting`, `list_report_sources`, `list_attachments` |
| `{items, omitted}` | `list_proposals`, `list_programs`, `list_projects`, `list_schedules` |

(`omitted` counts the rows a read's own defaults hid. `list_work_items` and
`search` carry it *in addition to* the paginated envelope. `search` adds two
fields no other read has — `relaxed`, saying whether the all-terms query was
falling back to any-term, and `parsed`, the tsquery Postgres actually ran.
Both are reported rather than inferred: khound's Gate A failure was a query
path that returned zero while reporting health, and a caller that cannot tell
a strict answer from a relaxed one cannot tell a precise result from a broad
one.)

The bare-array reads are the ones with no natural paging story — a project has a
handful of areas, a node has a handful of comments, and there is one
`list_report_sources` row per reporting source. That last one is also the one
read where a cap would be a *bug*: truncating it could push a stale source off
the end behind fresher ones, which inverts the failure #950 exists to catch.

`list_projects` and `list_proposals` are the exceptions, and for a reason that is
not paging: both **filter rows by default** (WI #828, WI #852), so a bare array
would be a narrowed view indistinguishable from a complete one. `omitted` is what
stops an agent concluding "there is no such project" from "you did not ask for
archived ones" — the same silent-truncation failure the four-shape table exists
to make visible. `list_work_items` carries `omitted` on top of its paginated
envelope for exactly the same reason (WI #851, WI #861):
`total` is the count *after* filtering, so a sweep used to decide "is this
project drained?" needs to see what the filter hid.

Note the **MCP shape is the one tabled here**, and REST can differ where the web
app's needs do: `GET /api/projects` and `GET /api/proposals` still return the
full bare arrays the UI renders from.

**Decided (WI #579, 2026-07-24): they stay bare.** Enveloping them for
uniformity would put a `total` that always equals `items.length` and a `limit`
that never clips on every one of those reads, and would touch both transports,
the generated types and every web caller to say nothing new. The cost of the
split is that an agent must know which is which — so this table and the MCP
`instructions` are now fenced against each other and against the wire shape
(`docs_drift.rs::the_collection_read_shapes_agree_across_docs_and_instructions`
and `dispatch.rs::collection_reads_return_the_shape_the_instructions_promise`).
A new collection read that is not classified here fails the build.

**Archived rows are excluded by default.** This is deliberate (D-3): the common
question is "what is live", and the old behaviour silently mixed archived rows
into every answer. To see them:

| You want | REST | MCP |
|---|---|---|
| live only (default) | omit `archived` | omit `archived` |
| archived only | `?archived=true` | `"archived": true` |
| both | `?archived=all` | `"archived": null` |

This is true of **every read that has an `archived` filter**. Until WI #851 it
was not: `survey_work_items` defaulted to "both" — against this table, against
the MCP `instructions`, and in the one tool the instructions recommended for
sizing a backlog, so any sweep over-counted with nothing in the response saying
so. `list_proposals` had no `archived` predicate at all. Both were brought into
line, which is part of why the survey alias had nothing left of its own to
justify it (deleted in #871).

The bare-array reads (`list_reports`, `list_areas`, `list_comments`) have
**no** `archived` filter — areas and comments are not nodes and have no
archived state, and reports have never exposed one.
`docs_drift.rs::the_archived_affordance_matches_the_instructions` fences that
split against the derived schemas, so a read cannot quietly join or leave the
class.

### Disposal semantics (WI #855)

The table above says how to *read past* a disposal. This says which disposal to
*write*, and it is a decision the corpus needs made once: korg had two endings
in the code and no rule saying which applied to what, and using one flag for
three different claims is how a corpus drifts.

> **`archived` means "real, but not a target for new work."** Lossless,
> reversible, keeps history. This is the ending for anything with identity.
>
> **A hard delete means "this was never real."** Irreversible, and it
> **refuses when the row is referenced**. It exists only where mis-capture is
> routine and there is no history worth keeping.
>
> **Test residue is a `category`, not a disposal.** `EVAL` (migration 0018)
> already makes harness leakage findable as a group; it needs no new ending.
> The convention that puts it there is
> [eval-convention.md](eval-convention.md) — evaluation runs write to the
> `eval` project, and that page is written to be handed to an eval agent.

| Kind | Ending | Why |
|---|---|---|
| work items, cards, proposals, reports, handoffs | `archived` only | each carries a thread and a decision history |
| programs | `archived` only (#968) | a program outlives its slices and records how a multi-repo push was sequenced; `done` is its lifecycle end, not a disposal |
| schedules | `archived` only (#581) | `paused` and `done` are lifecycle — a paused schedule is one deliberately not surfacing, a done one has fired its single occurrence — and the materialisation history hangs off it either way |
| attachments | **hard delete** (`DELETE /api/img/:id`), and it does **not** refuse when referenced | the one exception below |
| projects | `archived` only (#828) | a project is the routing target for real work; `archived` already means "not a target for new work", so a delete would have nothing left to mean |
| links | `archived` **or** `delete_link` | a capture is either something you decided about or something you mistyped |
| areas | `delete_area` only | an area is a label with a description, not a record — nothing accumulates on it, so there is no history to archive |
| comments, edges | delete only | correcting them *is* the edit |

**Attachments are the one deliberate exception to refuse-if-referenced** (#1388
recorded it here; the behaviour is sprint 056's). Deleting an image cascades its
`has_attachment` edges and its comments rather than refusing, because an image
is bytes, not history — discarding it *is* the point, and a screenshot that
cannot be removed because something links to it is the failure that design
avoided. Do not predict `conflict` for `DELETE /api/img/:id` from the doctrine
above; it will delete.

**The refuse-if-referenced clause is the load-bearing half** everywhere else.
Without it the
schema answers the question instead, and answers it wrongly in both directions:
`relationship` and `comment` both cascade from `node`, so deleting a link would
take its edges and its whole thread with it; `workitem.area_id` is `ON DELETE
SET NULL`, so deleting an area would silently unfile every item under it.
Neither is a decision a caller made. So both deletes count what points at the
row, refuse with `conflict` naming it, and leave the resolution — move the
items, drop the edges, or just archive it — to the caller.

Both deletes return `{deleted: bool}` where `false` means there was no such
row, matching `delete_comment`; a wrong-kind `node_id` is `invalid_input`, not
a silent cross-kind delete.

Per-entity filters, all applied server-side — prefer them to fetching
everything and sifting:

| Operation | Filters | Ordering |
|---|---|---|
| `list_work_items` | `project` (name), `wi_status` (+ `"all"`), `archived` | `wi_number` |
| `list_cards` | `status`, `project`, `archived` | `status`, `rank`, `node_id` |
| `list_links` | `disposition`, `read`, `archived` | `node_id` |
| `list_proposals` | `status` (+ `"all"`), `project`, `archived`, `detail` | pinned, `rank`, `node_id` |
| `neighbors` | `label`, `kind` | `node_id`, `rel_id` |

Every ordering carries an id tie-breaker, so equal ranks no longer shuffle
between calls.

### The work-item list is lean and terminal-excluded (WI #861)

`list_work_items` used to return full `content`+`details` for every row —
~890k characters across the corpus, of which 78% were `closed` items. Over MCP
it is now the slim projection (`wi_number`, `node_id`, `project`, `title`,
`wi_type`, `wi_status`, `wi_tshirt`, `comment_count`) and it narrows twice:

| `wi_status` | rows returned |
|---|---|
| omitted | `open` + `resolved` + `done` + `parked` — everything not terminal |
| `"all"` | every status |
| one of the five | exactly that status |

`resolved` and `done` stay visible deliberately: `done`'s visibility is a
promise `update_work_item`'s own schema makes, and `resolved` is the
implemented-but-Ken-may-still-want-to-see state. `closed` is the terminal one,
and hiding it is what finally makes that schema's "hidden by default" sentence
true — nothing on the MCP surface hid anything before this.

`parked` (#810, sprint 054) is visible for a different reason than the other
three: it is work deferred until a *condition* fires rather than work in
progress, and the whole point of the status is keeping it in view — hiding it
would make `parked` a slower spelling of `closed`. The web UI sorts parked rows
below a divider instead. Note that it is **unfinished**, so a dependency on a
parked item still reads as an unmet blocker in `get_board`'s deconfliction
(#978): deferred is not done.

`omitted` is `{closed, archived}`, a cascade like `list_proposals`': `archived`
counts what the archived filter hid, `closed` is counted only over rows that
passed it, so no row lands in both. A field is `0` when you asked to see that
class.

There is **no `detail` flag**, unlike `list_proposals`. A proposal's full row is
one field of prose, so `full` had to keep something reachable; a work item's full
row is `content`+`details` across hundreds of rows, which is the payload problem
itself. The full tier is `get_work_item`, one row at a time, and it already
inlines comments and edges.

REST keeps the full rows at `GET /api/work-items` — the Work Items page walks
that read to completion and then searches `content` and tickers `details` in
memory.

### A work-item row says whether anything covers it (sprint 042)

`covers` only ever read one direction: a proposal could name its work items, a
work item could not name the proposal that had claimed it. Answering "which of
the 135 open items are already in a live proposal?" during the 2026-07-31
backlog review took 17 `get_proposal` calls and a script. Both row tiers now
carry the answer:

| field | on | means |
|---|---|---|
| `proposal_node_id` | both | node id of the **live** proposal covering this item, else `null` |
| `has_handoff` | both | a `has_handoff` edge leaves this item — durable context is waiting |
| `has_details` | lean only | non-empty `details`; the full row carries `details` itself |
| `updated` | both | when the item last changed (#1411) — the staleness signal, `updated` only: `wi_number` already proxies *creation* age, and the missing half was "has anyone come back to this" |

**`proposal_node_id` is null for `done` and `declined` proposals**, by design.
The question it answers is "is this already spoken for", and a declined
proposal speaks for nothing. Pass the id to `get_proposal` for the bundle.

`has_handoff` is the cheap one to care about: it is the same "read the handoff
before acting" contract the focused reads enforce, hoisted onto a list so you
can see which of fifty rows has context waiting without fifty `neighbors`
calls.

Two things worth knowing if you extend this. The `relationship` table's columns
are `left_id` / `right_id` / `relationship` — not `left_node_id` / `label`. And
the two labels point opposite ways: `covers` is proposal → work item (the item
is the **right** end), `has_handoff` is node → handoff (the item is the
**left** end).

The per-project rollup of the same aggregate is REST-only, at
`GET /api/proposals/rollup` — it renders the Planning rail and is not something
an agent needs a tool for.

### The proposal queue narrows by default (WI #852)

`list_proposals` measured 110 rows / ~185,500 chars / **~46k tokens** unfiltered,
71% of it `done` proposals no caller had ever wanted. Over MCP it now narrows
twice, and reports both:

| `status` | rows returned |
|---|---|
| omitted | `proposed` + `active` — the live queue |
| `"all"` | every status |
| one of the four | exactly that status |

`detail` picks the projection: `"lean"` (default) is `node_id`, `title`,
`status`, `project`, `rank`, `pinned`, `covered_count`, `comment_count` —
**no `summary`**, which is the payload. `"full"` is every column, so nothing
became unreachable; it just stopped being the default. `get_proposal` is still
the authoritative single-proposal read, and it carries the covered work items
too, which is what a caller wants immediately after picking one.

### A proposal's summary is a routing contract; `notes` holds the analysis (WI #860)

Proposals are written as plans, so `summary` had become the longest prose in
korg: 189,471 characters across 119 rows, p90 of the `proposed` queue at 5,389.
Migration 0021 applies the shape 0020 gave projects, one tier up in size:

| Field | Bound | Returned by |
|---|---|---|
| `summary` | **≤500 chars**, CHECK-enforced | every read that returns a proposal at all |
| `notes` | unbounded | `get_proposal`, `list_proposals detail:"full"`, REST |

`summary` answers *should I pick this?* — what the bundle is, why now, roughly
how big. `notes` answers everything after that: measurements, dependencies,
sequencing, what was considered and rejected.

Over-length is `invalid_input` naming the cap and the remedy, **never** a
truncation — checked in `korg-core` so the caller gets a typed error rather than
a raw constraint violation. The same rule as `project.description` (#828), and
the same reason: a silently-trimmed contract is one nobody wrote.

The migration **moved** every over-cap summary into `notes` verbatim and derived
the new one from its first paragraph, marked ` […]` where anything was dropped.
Nothing was lost, and nothing was invented: see
`crates/korg-core/migrations/0021_proposal_notes_and_routing_summary.sql`, whose
postcondition refuses to apply if a marked summary has no `notes` beside it.

`omitted` is `{done, declined, archived}`, computed as a cascade — `archived`
counts what the archived filter hid, and the two terminal counts are taken only
over rows that passed it, so no proposal is counted twice. A field is `0` when
you asked to see that class.

## Search (#1177)

One lexical read over the whole corpus: every node kind that carries prose,
plus **one document per comment**. Comments are the point. A resolution
comment, a review verdict, a bring-up recon note — none of that is reachable by
scanning titles, and on the frozen acceptance suite
(`kubs0:~/khound-eval/suite-002.toml`, 12 korg-content queries) full-content
search answers 12/12 at rank 1 where a title-only scan answers 9/12 at rank 3.

`search` (MCP) and `GET /api/search` (REST) take the same knobs and return the
same rows.

### Query semantics

**All terms are required.** When that matches nothing the query is relaxed to
**any term**, and `relaxed: true` says so. Both halves matter:

- The relaxation is WI 1001's contract — "all-terms-required by default, relax
  when empty" — and prose questions land on it almost every time, because a
  ten-word question rarely has a document containing all ten words.
- Reporting it is korg's addition. khound shipped the relaxation on one query
  path and not the other; the un-relaxed path returned zero for every prose
  query while reporting `ok, hits=0`, and nothing noticed for a whole gate run.
  A caller that cannot tell a strict answer from a relaxed one cannot tell a
  precise result from a broad one — and under relaxation `total` counts
  any-term matches, which is frequently most of the corpus.

`"quoted phrases"` and a leading `-` to exclude both work. **A query carrying an
exclusion is never relaxed**: `foo -bar` relaxed to `foo | !bar` would match
every document *without* `bar`, inverting the request. Someone who typed an
exclusion meant a precise query, so the honest answer to "nothing matched" is
nothing.

`parsed` reports the tsquery that actually produced the results — stemming
applied, stop words dropped, and the any-term form when `relaxed` is true.
Reporting the parse that was *abandoned* would describe a search that did not
happen.

### Scoping

By default only **live** rows are searched: each kind's own terminal state is
excluded, matching what its list read hides (`closed` work items,
`done`/`declined` proposals, `done` programs, `Done`/`Cut` cards), and archived
rows are excluded too. A comment inherits its parent's state, so closing an item
takes its discussion out of the default view with it — otherwise the default
would hide a decision and show the argument about it.

`omitted` is `{terminal, archived}`, counted as a cascade so nothing is
double-counted.

**Pass `scope: "all"` whenever you are chasing something already decided.** Every
one of the acceptance suite's keyed nodes is closed or resolved, which is the
shape of most historical questions; the default is tuned for "what is in
flight", not "what did we conclude". This is why scoping is a **filter** and
never a ranking term — a not-done *discount* would make the same answers
merely harder to find, which is worse than absent because it looks like working.

### What it is not

Lexical, not semantic: it matches words, not meaning. A query sharing no
vocabulary with its answer misses, and no ranking knob fixes that. Measured on
an independent query set whose wording deliberately avoids the target titles,
the hit rate is 3/5 rather than the suite's 12/12 — the suite's queries were
written by someone looking at the target node. Phrase the query in the words the
record itself would use.

There is also **no index to maintain**. The tsvectors are generated columns
(migration 0029), so Postgres keeps them current inside the writing
transaction: a write is searchable the moment it lands, there is no refresh, no
rebuild, and no staleness for a hit to report.

## Two-level reads

Collections say **whether** there is discussion; focused reads **inline** it.
Every commentable row (`WorkItemRow`, `CardRow`, `ProposalRow`, `ReportRow`)
carries an exact `comment_count`, and `ProposalRow` also carries
`covered_count`.

Focused reads inline up to 10 comments with a `comments_truncated` flag; page
the tail via `list_comments`:

- `GET /api/work-items/:wi` and the `get_work_item` tool — **the same shape**.
  They were one operation under one name with two shapes until sprint 015.
  The tool takes `wi_number` **or** `node_id` (WI #966): the 0009 identity
  migration made a work item's node id equal its `wi_number`, so an agent
  holding either — and `related`, `covered` and `list_awaiting` all hand out
  `node_id` — is holding the right number. Passing both is `invalid_input`,
  matching the project/area selectors: korg does not choose between two ids the
  caller passed on purpose. REST keeps the single path segment, which is
  unambiguous by construction.
- `GET /api/proposals/:node_id` and the `get_proposal` tool — the proposal,
  its `covered` work items (`wi_number`, `node_id`, `title`, `wi_status`,
  `wi_tshirt`, `project`, `comment_count`, ordered by `wi_number`), and its
  comments. This is the authoritative "what is this sprint" read: it replaces
  `list_proposals` + `neighbors` + `list_work_items` and a client-side join.

Missing single-item reads are 404 / `isError` `not_found`, never `200 null`
(D-6).

### Related context (LB-3)

The same two-level rule extends from comments to **edges**: a focused read
inlines the node's relationships so an agent reading a work item cannot silently
miss that it is covered, depended on, or handed off — the invisible-edge
failure of review L-6.

`get_work_item` and `get_proposal` carry `related: RelatedRef[]` and an exact
`related_truncated` flag. Each `RelatedRef` is a compact neighbor:
`rel_id`, `node_id`, `wi_number` (when the neighbor is a work item), `kind`,
`title` (resolved across kinds), `label`, `direction` (`"out"`/`"in"`), and
`directed`.

- The list is **capped at 25** and ordered by `(label, node_id)`, so if a node
  ever exceeds the cap the structural labels (`covers`, `depends_on`, `finding`)
  are inlined before `related-to`; `related_truncated` is exact, and the caller
  falls back to `neighbors` for the whole set.
- `get_work_item.related` carries **all** of the item's edges — the `covers`-IN
  ref is how the reader learns which proposal covers it.
- `get_proposal.related` **excludes `covers`** (already inlined as `covered`)
  and carries the proposal's other edges.
- `neighbors` stays the generic floor: unfiltered, higher-limit, for everything
  the inlined block deliberately caps or omits.

### Handoffs

A **handoff** is a first-class node — title, summary, Markdown `body` — carrying
durable, cross-machine context (a contract, a state dump, a "here's where I left
off") for the work it describes. It is deliberately **not** a work item: it has
no status, size, or lifecycle to leak into backlog, survey, or planning.

- `create_handoff(title, summary, body, related_node_ids, …)` writes the node
  and one `has_handoff` edge per owner in one transaction (owner → handoff). It
  **rejects** an empty `related_node_ids` unless `allow_standalone` is set — a
  forgotten link must not silently orphan a handoff — and rejects the whole
  create if any owner id does not resolve (`not_found`, no partial insert).
- Handoffs need **no bespoke read field**. A `has_handoff` edge surfaces in its
  owners' `get_work_item` / `get_proposal` `related` block like any other edge
  (titled, so the reader learns *what* was handed off without a fetch). Once a
  ref points you at one, `get_handoff(node_id)` returns the full `body` plus the
  nodes it is attached to. Relationship changes go through `relate`/`unrelate`.

## The board rollup (#970)

`get_board` / `GET /api/board` returns **the whole state of the work in one
call** — the read that exists so nobody crawls. The 2026-07-31 backlog review
assembled a fraction of it with 17 `get_proposal` calls plus a script.

It is **not a collection read** and is deliberately absent from the shape table
above: it is one composite object, not `{items, …}`.

| Field | What it is |
|---|---|
| `generated` | when the board was assembled, from **Postgres's** clock — the same one every timestamp here came from, so `generated - awaiting_since` is a correct age |
| `active` | proposals in `active`, pinned first then rank — each with `summary`, the work-item rollup `covered_count` plus one count per `WI_STATUSES` value (they sum to it — #1386), and `synopsis` (korg #1003): the newest comment opening with the `⟦curator⟧` marker as `{body, updated}`, or `null` |
| `queue` | proposals in `proposed`, same row type, same order |
| `proposals_omitted` | `{done, declined, archived}` — the same envelope, meaning the same thing, as `list_proposals` |
| `proposal_edges` | korg #1003: every edge whose **both** endpoints are `active`/`queue` rows — `{left, right, label, directed, origin, created}`. `directed` comes from the registry (read undirected labels symmetrically); `origin`/`created` are the first read surface for D-17's write-side edge provenance, which is how curated edges (`origin: "kfdc-curator"`) stay distinguishable from human ones |
| `blocked` | #978: every **unmet** `depends_on` holding up a live row — `{proposal, via, dependent, dependent_wi_number, blocker, blocker_kind, blocker_wi_number, blocker_title, blocker_project, blocker_status, sequenced_by}`. See below |
| `programs` | live programs, each carrying `slices` exactly as `get_program` returns them |
| `programs_omitted` | `{done, archived}` |
| `awaiting` | `list_awaiting`'s lane, unchanged |
| `depth` | per-project queue depth — every project, with its `status` |
| `reports` | the newest 5 |
| `events` | #977: the newest 20 status transitions, newest first — `{node_id, kind, wi_number, title, project, from_status, to_status, at}`. See below |
| `sources` | #950: every reporting source with its `freshness` and what it `asserts`. **Uncapped** — see below |
| `due_schedules` | #1385: what is **due right now**, soonest-due first — `list_schedules(due_only)`'s `ScheduleRow`s, uncapped. See below |

**It takes no arguments**, for the reason `list_awaiting` takes none: it is one
screen's state, and every filter it could offer is already decided by what the
consumer renders. A `project` filter would produce a board of one repo, which is
the opposite of the question it answers. Caps are fixed policy; a consumer that
wants more of one corpus calls that corpus's own read, and one proposal's covered
items or one program's detail are still `get_proposal` / `get_program`.

**There is no counters block, deliberately.** Every summary figure a header bar
wants is derivable from what is already here — live proposals is
`active.length + queue.length`, shipped is `proposals_omitted.done`, awaiting is
`awaiting.length`, active projects is `depth` filtered by `status`. A counter
that can disagree with the list printed beside it is a bug generator, and adding
one per panel is the aggregate creep #976 filed a warning about. `depth` carries
`status` (sprint 045) precisely so the one non-derivable figure became derivable
instead of becoming a counter.

### The ticker (#977)

`events` is the board's event feed, and it exists because korg finally has
something honest to build one from. #970 asked for "recent events/reports";
sprint 045 shipped the reports half and deferred this one (D-7) rather than fake
it, because the only available substitute was `node.updated` — which advances on
any edit, so a "recently shipped" list built from it dates a proposal by its last
tag edit. On a surface whose promise is that it renders korg deterministically, a
plausible wrong date is worse than an absent panel.

Migration 0026 added an append-only `transition` table, written by korg-core's
three update paths — never by a trigger, per the rule 0023 set. The scoping rule
is worth stating because it is what keeps the feed small and complete at once:
**korg logs the events that have no honest timestamp anywhere else.** A node's
creation has `node.created`, a report landing has `report_date`, a handoff has
its own `node.created`. A status change had nothing, and it is the one that says
"shipped".

Two properties a consumer must know:

- **A write that does not change the status produces no event.** korg's own
  post-deploy check re-PATCHes a status to the value it already holds, and agents
  re-set statuses routinely. Logging those would rebuild `node.updated`'s lie one
  table over, so the write path compares before it appends and migration 0026
  carries a CHECK as the backstop.
- **The log starts at 0026 and was not backfilled.** There was no honest history
  to backfill from. A freshly migrated corpus therefore has an empty ticker that
  fills forward — which means empty says "nothing has moved since the migration",
  not "nothing has ever moved". A panel that cannot express that difference
  should render nothing rather than "no recent activity".

Schedules are deliberately not logged. `materialize_schedule` moves a `once`
schedule to `done` outside `update_schedule`, so hooking only the update path
would give that kind a half-recorded history — and a feed that is silently
partial for one kind is worse than one that never claimed to cover it. A
schedule's real history is its `materializes` edges, which record every firing
already.

### Due schedules (#1385)

`due_schedules` is what is due **right now**, soonest-due first. It exists
because the schedules feature computed due-ness correctly and then put it
nowhere anybody looks: only `/schedules` and `list_schedules`, the two places
you go when you are *already* thinking about schedules. Schedule 1112 sat due
for nine days, invisible — #950's own lesson ("a channel nobody looks at is not
a channel") reappearing in the feature built in its honour.

Three properties, and each is a decision:

- **It is `list_schedules(due_only: true)`'s rows, not a second predicate.**
  Due-ness has exactly one definition — the three clauses under "Schedules"
  below — and the board calls the list read to get them. A board query that
  re-derived due-ness would agree on the day it was written and drift after,
  and the clause it would most likely drop is the outstanding-item one, which is
  what stops the block nagging about a drill whose work item is already open.
- **Uncapped**, for the reason `sources` is: the block is sorted by due-ness, so
  a cap drops the row that has been waiting longest — the failure it exists to
  catch, inverted. It is bounded in practice by construction: a schedule that
  fires leaves the block until its item is finished.
- **Full `ScheduleRow`s**, the type `list_schedules` and `get_schedule` already
  return — the same reuse `programs` makes with `ProgramRow`. `preview_title` is
  what materialising would create right now, substitutions applied, so a panel
  renders a due row without a follow-up read.

Every consumer of the board inherits it (GP-1 in the korg+ guiding plan: agents
curate korg, the board renders korg — a panel that needs data korg cannot hold
is a korg work item, never a side store). korg's own Today page shows the count
as a pill; kfdc and korg-dash get the block for free.

### Deconfliction (#978)

`blocked` answers "is this queue row blocked, and by what" — deterministically,
with no curator and no LLM, because `depends_on` edges are already real and
typed. One entry per (row, blocker) pair; an empty list means nothing live is
waiting on anything.

| Field | What it is |
|---|---|
| `proposal` | the `active`/`queue` row that cannot proceed |
| `via` | `proposal` when the row itself carries the edge, `covered` when one of its covered work items does — both count, and they are distinguished because they mean different things: a sequencing decision about the sprint versus one task inside it waiting on something outside |
| `dependent` / `dependent_wi_number` | the node carrying the edge |
| `blocker` + `blocker_kind` / `blocker_wi_number` / `blocker_title` / `blocker_project` / `blocker_status` | what is depended on, resolved enough to render a card naming both sides without a follow-up read |
| `sequenced_by` | the live `program` whose ordered slices already express this dependency, or `null` |

The rules, each a decision rather than an accident:

- **Unfinished is derived from the vocabulary, per kind** — `WI_FINISHED_STATUSES`
  for work items, the terminal sets for proposals and programs — so a future
  status is handled here for free. Note that work items have **two** partitions
  over one vocabulary and this is not the other one: `WI_TERMINAL_STATUSES` is a
  *list-visibility* split holding `closed` alone, while a dependency on a `done`
  item is satisfied. Reading the visibility split here would report every `done`
  dependency as an unmet blocker.
- **One hop.** A closure is a different query and a different UI. A blocked
  blocker still appears in its own right, so a chain is visible without being
  computed.
- **An archived blocker does not block**, and neither do the dependencies of a
  covered item that is itself finished. A row held blocked forever by something
  nobody will ever do is worse than one that quietly frees up; a dangling
  dependency is a different card, and nothing has asked for it.
- **A blocker whose kind has no lifecycle korg tracks** (a link, a report, a
  handoff) is not a blocker. korg cannot say whether such a node is finished, and
  inventing an answer is the failure this panel exists to avoid.

`sequenced_by` is kfdc #1070's answer. The objection was that for
program-ordered work Deconfliction "is essentially showing the same data twice,
in Operations and Deconfliction" — correct, but the fix cannot live in kfdc,
because kfdc cannot filter what korg did not label. korg names the program that
is already drawing the dependency as sequence; the render decides whether to show
it. Neither side guesses, and korg does not drop a true fact to make one panel
tidier.

`blocked` is not a superset of `proposal_edges` and does not replace it: that
list is *every* edge between two live rows whatever it says, this one is the
subset meaning "cannot start yet", widened to blockers that are not proposals at
all.

Consumers: **kfdc** (`kai:~/src/tools/kfdc`, the widescreen overseer board) and
**korg-dash** (the kdeskdash Pi feed, which derives its panel counts from this
read rather than growing its own queries).

## The flow series (#1318)

`work_item_flow` (REST: `GET /api/work-items/flow?days=N`) is korg's one
time-series read: one row per day in the board's timezone, oldest first,
ending today — `{day, added, closed, backlog, added_durable, closed_durable}`
per row, with `{horizon, timezone, durable_after_days, generated}` on the
envelope. It exists because the raw open count is the wrong instrument for the
backlog: measured 2026-08-15, 77.6% of all closed work items lived under one
day (sprint-internal task lists opened and closed in one session), so a plain
added-vs-closed chart reads ~1:1 forever whatever the backlog does. The
durable split — an item is durable once it lived, or has so far lived, more
than `durable_after_days` days — is where the signal is.

Three source decisions carry the contract:

- **`added` comes from `node.created`, never the log.** Creating a work item
  writes no transition row, so an arrivals series derived from transitions is
  silently all zeros. `node.created` is complete for every item.
- **`closed` and the backlog reconstruction come from the transition log**,
  which begins at `horizon` (2026-08-08) and was deliberately not backfilled.
  A window reaching past the horizon is **clamped** to it: the series starts
  at the horizon and is simply shorter than asked. Days the log cannot answer
  are absent, never zero-filled — a zero meaning "no data" renders exactly
  like "nothing closed", which is the lie this read exists to end. Consumers
  therefore take the series length from the response, not from the `days`
  they passed.
- **Archived items are excluded throughout**, so today's `backlog` equals what
  `list_work_items` reports as `total` — two reads that disagree about "how
  many are open" would be a bug, and there is a test pinning them together.

`backlog` at a day's end is the current status un-walked: the status at
instant T is the `from_status` of the first transition after T, or the current
status when nothing has moved since. A close counts on the day it happened
(once per day, even if an item closes twice that day); a reopened item rejoins
the backlog on the day of its reopening.

One property worth knowing before rendering: `added_durable` is structurally
zero for the newest `durable_after_days` days of any series — an arrival's
durability is only knowable in retrospect — so at the launch default
(`days` = 6, threshold 7) that column is all zeros until the window widens.
`closed_durable` has no such lag: an item closing today may have lived months.

The default window is 6 days, chosen so the whole window sits inside the
transition horizon at ship time (Ken, 2026-08-15); widening it to 10 after
2026-08-18 is a one-constant change (`FLOW_DAYS_DEFAULT` in
`crates/korg-core/src/repo/flow.rs`).

This is a rollup like the board, not a collection read: it returns no
`{items, total, limit, offset}` envelope and takes no `archived` filter (the
exclusion is fixed), so it appears in neither the collection-shape table nor
the instructions' read lists.

Consumer: **kfdc**'s Rate of Fire panel (kfdc #1319).

## Time-derived surfacing (#581, #950)

korg holds two kinds of state that a **date** changes rather than a write:
a schedule that comes *due*, and a report source that goes *stale*. Both are
**read-time predicates**. korg runs no scheduler, no timer and no daily tick, and
that is a contract rather than an omission — #950 exists precisely because an
unattended scheduled thing died quietly for eleven days, so korg's answer to time
must not itself be one. It also removes missed-tick recovery and the
duplicate-materialisation failure a double firing creates.

### Schedules

A `schedule` node is a work-item template plus a `cadence` and an `anchor_at`.

```
due  ⟺  status = 'active'
    ∧  no outstanding materialisation
    ∧  anchor_at + interval(cadence) ≤ now()
```

- **`once` is not a special case.** It is the cadence whose interval is zero, so
  `anchor_at` *is* the fire date. The one-shot ("recheck the timers after the DST
  transition") and the quarterly restore drill are one node shape.
- **The cadence set is two families, and picking the wrong one loses a weekday.**
  `weekly` and `fortnightly` are whole weeks, so they hold the day a schedule
  fires on; every calendar-month cadence advances by month and drifts off it
  (2026-08-08 is a Saturday, 2026-09-08 is a Tuesday). "Every other Saturday" is
  `fortnightly` — which was added in sprint 063 (#1113) after the feature's first
  live use found the week family had exactly one member and one of the two
  recurring requests could only be approximated. A third week interval would be
  the point to reconsider a counted `every_n_weeks`; two is not.
- **The outstanding clause is the anti-duplicate rule**, and the honest one:
  while the drill's work item is open, that item *is* the schedule's surface.
  `resolved` counts as outstanding here specifically — a drill that "may still
  need a user test" has not been performed.
- **`anchor_mode`** picks which event advances the anchor: `created` (at
  materialisation) or `completed` (when the item reaches `done`/`closed`, a
  korg-core write rule, never a trigger). One column serves both styles.
- **Seed `anchor_at` with the real last-done date.** A quarterly drill last run
  2026-07-08 anchored there comes due 2026-10-08 rather than the day it is filed.
- **Substitutions** are a closed set — `{YEAR}`, `{MONTH}`, `{DAY}`, `{DATE}`,
  `{QUARTER}` — rendered from Postgres's clock. An unrecognised placeholder is
  refused at write time rather than rendering literally into a title months
  later.
- `materialize_schedule` writes a `materializes` edge, so "when was this drill
  actually run?" is answerable from the graph. `schedule.last_wi_id` is only the
  pointer to the newest one.

### Report-source staleness

A source that filed on a cadence and then stopped is itself the alert, because
the absence of a write is exactly what a broken tool can never report about
itself. In July 2026 kmon crashed 1s into collection for 11 consecutive days and
korg kept serving its last GREEN, so the fleet read healthy *because* the thing
checking it was broken.

`list_report_sources` derives, per source: the cadence (the **median** gap
between its own `report_date`s, or a declared override), a grace window
(defaulting to the cadence, floor one day), and:

| `freshness` | meaning | alerts |
|---|---|---|
| `fresh` | filed within cadence + grace | no |
| `stale` | overdue | **yes** |
| `retired` | declared ended (`set_report_source`) | no |
| `unrated` | no believable cadence, and none declared | no |

**The rule the feature exists for: anything not `fresh` asserts `unknown`.**
Never the last known status — there is deliberately no field on the row that
could carry it. Restating a stale GREEN is the exact failure this ends.

**Which "today" this counts against.** Staleness is `current_date` — Postgres's
session date, UTC — while the flow series buckets days in `KORG_TIMEZONE`. A
source can therefore flip stale up to ~8h off the board's local day boundary.
Immaterial while the grace window is ≥ 1 day, which it always is (the floor),
and recorded here only so that a "why is this stale already?" chase ends in one
paragraph rather than in the SQL.

**A cadence has to be earned.** korg infers one only when the history both holds
at least three reports *and* **spans at least seven of the inferred cadences**
(#1097). The count alone is not enough, and the reason is worth keeping:

`kyac` filed four times over five days — 2026-07-09, 07-11, 07-12, 07-14, gaps of
2, 1, 2 — and korg called it 21 days overdue on a 2-day cadence it had invented.
kyac is interactive; it files when prompted, and its silence means nothing.

A variance check would not have caught that: gaps of 2, 1, 2 are as regular as a
series gets. **A healthy episodic burst and a broken daily source are the same
shape**, and no statistic recovers the difference, because history cannot tell
you a source is *scheduled*. Span can — measured live, kmon's history spanned 34
cadences and kyac's spanned 2.5. `history_span_days` is on every row so an
`unrated` verdict is explicable rather than mysterious.

This is a trade, not a proof. Requiring an explicit declaration before alerting
would be airtight and would reintroduce #950's original failure: a genuinely new
scheduled source nobody declared, silently unwatched. A **declared** cadence
bypasses the span gate entirely — declaring one is a statement that a cadence is
*expected*, which is the thing inference can never establish.

`unrated` is a fourth value because both available guesses are bad: `fresh`
rebuilds the July failure, and `stale` cries wolf on every one-off report, which
trains people to ignore the panel — and a channel nobody looks at is not a
channel. Declaring a cadence is how a real source leaves that state on day one;
declarations may be written before a source's first report.

`retired` is what lets a deliberately-stopped source go quiet **without** the
same mechanism silencing a broken one. It is an explicit declaration, so "we
turned this off" stays distinguishable from "this died", which silence alone
never can.

Both surfaces ride `get_board`: `sources` sits beside `reports` in Sensor Net,
uncapped, alert-first. Rendering on korg-dash's Today page is a separate
project's concern — the derivation lives here and korg-dash consumes it.

## Images (#582, #1119)

An uploaded image is an `attachment` node (migration 0027). The metadata lives
in Postgres; the bytes live on disk under `KORG_IMG_ROOT`, one directory per
attachment. The design record is the handoff on the "Images in korg" program
(`korg:1127`); what follows is the contract.

### Bytes over REST, metadata over MCP

This split is the whole shape of the surface, and it is deliberate.

| Operation | Where | Call |
|---|---|---|
| Upload | REST | `POST /api/img[?owner=<node_id>]`, multipart, ≤32 MB |
| Fetch | REST | `GET /api/img/<img-id>[/<variant>]` |
| Discard | REST | `DELETE /api/img/<img-id>` |
| Attach on save | REST | `POST /api/img/<img-id>/link` with `{"owner_node_id": N}` |
| Read metadata | MCP | `get_attachment`, `list_attachments`, and `get_work_item`'s inlined `attachments` |
| Attach later | MCP | `relate` with `has_attachment` |
| Store size | REST | `GET /api/img/stats` |

There is deliberately **no** base64-over-MCP path and **no** `delete_attachment`
tool. Inflating image bytes by a third to carry them through a JSON-RPC channel
serves nobody, and a delete that reached only the row would strand the blobs
forever — the sweeper finds orphans *through* the row, so a row deleted without
its bytes is bytes nothing will ever collect. One path owns disk.

An agent reads an image by fetching the `agent` variant to a temp file and
reading that file natively:

```sh
korg=https://kubsdb.encke-wahoo.ts.net:5674
curl -sf "$korg/api/img/img-c2a/agent" -o /tmp/shot.png
curl -sf -F file=@/tmp/shot.png "$korg/api/img?owner=582"
```

### The display id

`img-<hex>`, where the hex **is** the node id. `img-c2a` is node 3114. It is
parsed case-insensitively, so the `IMG-C2A` spelling resolves to the same
image. There is no separate id sequence: deriving it buys uniqueness and
comment-relatability for free, at the price of gaps nobody can see.

### Variants

Generated eagerly at upload and never on demand — korg has no resize-on-read
path and no cache layer:

| Variant | Long edge | For |
|---|---|---|
| *(none — the bare id)* | original | the archival copy, byte-exact, EXIF intact |
| `thumb` | 400 px | inline display and the attachment list |
| `agent` | 1568 px | agent reads — Anthropic vision downscales past this, so larger is pure waste |

Neither variant upscales. A variant that is encoded is encoded from decoded
pixels, which is what strips EXIF; the encoding follows the *pixels* rather than
the source format, so anything with an alpha channel stays PNG and everything
else becomes JPEG. korg accepts PNG, JPEG, GIF, WebP and BMP, sniffed from the
bytes — the upload's declared `Content-Type` is ignored, because a claim about a
file is not a file.

**A variant is not always a separate file (#1146).** When the original already
fits a variant's long edge *and* re-encoding it comes out no smaller, korg keeps
one copy instead of two and that variant's URL serves the original's bytes. Both
conditions are measured, not predicted: an opaque screenshot PNG encodes to JPEG,
a different type entirely, and loses badly to PNG on flat UI colour — so "is it a
different mime family" is the wrong question and only the byte count answers it.

Nothing changes for a caller fetching images. `/api/img/:id/agent` still resolves
and still serves that size; **an agent told to fetch the `agent` variant never has
to know how korg stored it.** The read surface says so anyway, in the variant's
`is_original` flag, for the two callers that care: byte accounting must not add
such a variant's `byte_size` to the original's, and a caller already holding the
original can skip the fetch. Two consequences worth stating:

- that variant carries whatever **EXIF** the original carried, since no re-encode
  stripped it. The original has always been served EXIF-intact at the bare id, so
  this exposes nothing a caller could not already fetch.
- in practice only `agent` is ever affected. `thumb` is 400 px, so an original
  small enough to qualify is tiny either way.

Serve responses carry `Cache-Control: public, max-age=31536000, immutable`. That
is a statement of fact, not a gamble: an id is a node id, and a re-upload is a
different node, so an attachment's bytes never change.

### Lifecycle, and the one rule that matters

An attachment is `pending` until something owns it and `linked` after. **That
state is derived from the `has_attachment` edge on every read, never stored.**

The sweeper deletes every `pending` attachment older than 24 hours, and that
sweeper is **the entire garbage-collection story**. There is no retention
policy, no delete-on-close and no delete-on-archive: closing and archiving a
work item never touches its screenshots. Growth is watched by kmon milestones
instead, and fast growth is the trigger to revisit.

Deriving the state rather than storing it is what makes that safe. With a stored
column, an edge written through the generic `relate` — the documented way to
attach an existing image — would leave the column reading `pending`, and the
sweeper would then delete an image a work item points at, silently and
permanently. With the edge as the only source of truth, that is unrepresentable:
anything reachable by a `has_attachment` edge is never a candidate.

Two consequences follow, and both are correct:

- **Deleting an owner orphans its images**, and they are swept 24 hours later.
  That is the only path that reaches it — discarding an attachment deletes it
  outright, and deleting the markdown token that displays one never touches the
  edge.
- **`unrelate` on a `has_attachment` edge is a decision to let the image go**,
  not merely a detach.

### Placement is markdown; existence is not

An image is displayed by a `![img-<hex>](…)` token in a body or a comment.
The token is *placement only*: deleting it removes the image from the prose and
leaves the attachment in the node's attachment list. Existence is never parsed
out of prose, which is what keeps garbage collection derivable rather than
guessed.

A korg comment is not a node — it is a row keyed to the node it hangs off — so
it cannot own an edge. An image pasted into a comment is owned by the **node**
that comment is on, and appears in that node's `attachments`; the comment body
carries the token.

**The token an agent should write** is `![img-<hex>](/api/img/img-<hex>/thumb)`
— the alt text is the id, and the URL points at the thumbnail. The web UI
writes exactly this on paste (sprint 057) and renders any such token as a
thumbnail that opens full resolution. It recognises the image by the **alt
text**, so a token whose URL points somewhere else still displays, but writing
the `thumb` URL is what keeps a body of a dozen screenshots cheap to render.

### Storage accounting

`GET /api/img/stats` reports `count`, the `pending`/`linked` split,
`original_bytes`, `variant_bytes`, `total_bytes` and `oldest_pending`. The byte
totals are summed from the recorded sizes — what korg *believes* the store
holds — rather than measured off the volume. The gap between `total_bytes` and
`du` on the store is stranded blobs, and having both numbers is what makes that
gap visible: writes land the row before the bytes and deletes remove the row
before the bytes, so a crash strands bytes rather than leaving a row promising
bytes that are not there.

## Relationships

Any node can link to any other through a single `relationship` edge:
`(left_id, right_id, relationship)`, unique on that triple. The label reads
**left to right**.

### Label registry

korg declares — and, since LB-2, **enforces** — the labels it writes or
interprets. The registry lives in `korg_core::relationships`, is the source
this table is written from, and is the **complete set**: `relate` rejects any
label not in it.

| Label | Direction | Reads | Endpoints | Same project? |
|-------|-----------|-------|-----------|---------------|
| `covers` | directed | proposal **covers** work item | `sprint_proposal` → `workitem` | **required** |
| `includes` | directed | program **includes** proposal as a slice | `program` → `sprint_proposal` | no — this is the layer where cross-project is legal |
| `finding` | directed | report **reported** work item | `report` → `workitem` | no |
| `depends_on` | directed | dependent **depends on** dependency | any → any | no |
| `related-to` | **undirected** | the two nodes are related | any → any | no |
| `collides-with` | **undirected** | the two nodes collide (same contract / fold on landing) | any → any | no |
| `has_handoff` | directed | node **has** handoff | any → `handoff` | no |
| `materializes` | directed | schedule **materialized** work item | `schedule` → `workitem` | no |
| `has_attachment` | directed | node **has** attachment | any → `attachment` | no |

**Directed** means the stored orientation carries meaning, so the reverse edge
is a *different* fact: `A depends_on B` and `B depends_on A` together are a
cycle, not a duplicate.

**Undirected** (`related-to`, `collides-with`) means korg stores whichever
order the caller happened to pass and readers must ignore it — treat the edge
as symmetric. There is no canonicalization: two nodes related to each other
have one edge in some arbitrary orientation. `collides-with` (korg #1002) is
the kfdc curator's mined-collision label — "same contract", "fold with
whichever lands second" — kept distinct from `related-to` so a Deconfliction
card is distinguishable from generic relatedness without reading prose.

**The vocabulary is closed.** An unregistered label is `invalid_input` whose
message names the registry and the near-miss (`unknown label 'related'; …; did
you mean 'related-to'?`); a label with declared endpoint kinds (`covers`,
`finding`) also validates both ends. This is enforced in `korg_core` — the
single write path both transports share — deliberately **not** a DB trigger or
CHECK, which would duplicate the vocabulary outside Rust and re-invite the drift
the registry exists to prevent.

**`covers` is single-project** (sprint 043, #967). A proposal and the work item
it covers must be in the same project; a cross-project edge is `invalid_input`
naming both projects and pointing at the program layer (#968), which is where
cross-project work is bundled. `depends_on` deliberately carries no such rule —
the homelab-ai plan's whole structure is dependencies between repos. A work item
with *no* project is unfiled rather than filed elsewhere and is not refused;
production holds none (measured 2026-08-05).

**`has_attachment` carries liveness, not just reference** (sprint 056). It is
the only label whose *absence* destroys the node on its right end: an attachment
with no `has_attachment` edge is `pending`, and the sweeper deletes it 24 hours
after upload. Writing the edge rescues an image; `unrelate`-ing it is a decision
to let that image go. See [Images](#images-582-1119).

**Extending it** is one registry entry in `korg_core::relationships` plus
`just gen`, which propagates the new label to the API, the MCP tool
descriptions, and the web picker in one step. `has_handoff` (sprint 025) was the
first to travel this path.

### Reading edges

`GET /api/nodes/:id/neighbors` and the `neighbors` tool return:

```json
{ "items": [ { "rel_id": 42, "node_id": 17, "kind": "workitem",
               "label": "covers", "direction": "in", "directed": true } ],
  "total": 1, "limit": 100, "truncated": false }
```

- `direction` — `"out"` when the queried node is the edge's **left** (the
  label reads queried → neighbor), `"in"` for the reverse.
- `directed` — whether `direction` means anything, straight from the registry.
  **When `directed` is `false`, ignore `direction`.**
- `total` counts every match before the limit, so `truncated` is exact rather
  than inferred from a full page.
- Filter server-side with `label` and/or `kind` instead of fetching every edge:
  `?label=covers&kind=workitem` is a proposal's work items.
- Ordering is neighbor `node_id`, then `rel_id` — stable even when two edges
  connect the same pair.
- `limit` defaults to 100, clamped to 500.

### Writing edges

`relate(left, right, label, origin?)` requires that both endpoints exist
(`not_found` otherwise), that `label` is registered (`invalid_input` naming the
registry and the near-miss otherwise), that endpoint kinds match a
kind-constrained label, and that the endpoints differ — self-edges are rejected
(`invalid_input`), and the schema enforces it with `relationship_no_self_edge`.
Re-creating an existing edge is a no-op that returns the same `rel_id`; relating
the reverse of an **undirected** edge dedups to the existing one rather than
storing a mirror.

`covers` and `finding` edges are written for you by `propose_sprint` /
`create_report`; both insert the semantic orientation (proposal → work item,
report → work item).

Each edge records **provenance** (LB-2): `created` (now, on insert) and an
optional self-reported `origin` — the web client sends `"web"`, `propose_sprint`
and `create_report` stamp their own operation name, a skill sends its name.
korg is no-auth HTTP, so `origin` is recorded, not verified; the re-relate no-op
preserves the originals. The first read surface is the board's `proposal_edges`
(korg #1003) — elsewhere provenance is still write-side only.

Every `covers` edge is `sprint_proposal → workitem` — there is no exception.
(One used to exist: pre-0008 bundles were work items titled `Sprint: …`,
standing in for a proposal kind that did not yet exist. Migration 0016
converted those five into real archived-done proposals and re-pointed their
edges, so the dual shape is gone — see History.)

### One proposal, one project

A sprint proposal carries a project (**required** since sprint 043 — `#967`,
enforced by `create_proposal` and by the `node_sprint_proposal_has_project`
CHECK), and every work item it covers is in that project. Both write paths
refuse a cross-project `covers` edge: `propose_sprint`, which inserts the
bundle directly, and `relate`, which is how a work item is added to a proposal
later.

This was convention until #967 made it a rule, and the convention leaked: at the
time of writing, 13 legacy proposals still cover work across projects — four of
them live in the queue. Migration 0022 moved the four that a rule could resolve
(unanimous covered-work-item project) and reports the residual rather than
guessing at the rest. They are split by hand, or absorbed by the program layer.

The refusal names both projects and the program layer, because the answer to
"my sprint spans two repos" is a program, not a wider proposal.

### Programs — the cross-project layer (#968)

A **program** is what a proposal is not allowed to be: work that spans repos. It
`includes` sprint proposals, ordered, and is the sanctioned answer to the
question the rule above refuses.

**A program carries no project at all**, enforced by the
`node_program_has_no_project` CHECK — the exact mirror of the proposal rule, and
for the opposite reason. A program filed under one project rebuilds the
mis-routing #967 cured: proposal 818 is the worked example, covering one work
item each in kagent, klams-mind and korg while filed under "korg", because the
writer had to pick one.

Instead, `span` is **derived**: the distinct projects of the included proposals,
on every program row. The fact has one home, so it cannot drift when a slice is
added. `create_program` refuses a `project`/`project_id` rather than dropping it
— a caller who passed one believes the program is filed somewhere, and a silent
drop leaves that belief intact.

| | proposal | program |
|---|---|---|
| project | **required** | **refused** — `span` is derived from the slices |
| bundles | work items, via `covers` | proposals, via `includes` |
| order | queue `rank` on the node | position `rank` on the **edge** |
| statuses | `proposed` → `active` → `done`/`declined` | `active` ⇄ `holding` → `done` |

**Order lives on the edge.** Position is a property of the containment, not of
the proposal — a proposal included by two programs has two positions, and only
`relationship.rank` can hold both. `create_program` writes the caller's `slices`
order directly; afterwards, `relate` with a `rank` **reorders in place**, keeping
the edge's `created`/`origin`. Re-relating *without* a rank leaves the position
alone, so an unrelated re-relate cannot shuffle a program.

**`get_program` rolls up so nobody crawls.** It returns the program, its `span`,
and `slices` — the included proposals in program order, each with its status,
project, `covered_count` and per-status work-item counts — plus comments and the
`related` block. Rendering a program takes one call, not one per slice.

`holding` is deliberately its own status rather than a shade of `active`: a
program whose current slice has shipped and whose next has not started is
neither finished nor in flight, and collapsing the two would have a board claim
work is underway when nobody is on it.

### Awaiting Ken (#969)

`node.awaiting_since` (+ `awaiting_note`) says **"this moves only when Ken
acts"** — a decision, an ops action no agent can perform, a review. It sits on
`node`, so a work item, a proposal and a program can all carry it, and
`list_awaiting` returns the whole lane in one call, oldest ask first, with each
row's own status resolved per kind.

**Why a column and not a reserved tag.** The tag was the cheaper mechanism and
it is unsound here: tags are written *wholesale* (`UPDATE node SET tags = $2`),
so an agent editing tags for an unrelated reason silently clears the marker —
and 76% of nodes carry tags. The corpus had already tried and failed at the tag
approach, holding one `decision` and one `needs-decision` node: two spellings,
one concept, neither converging.

A timestamp rather than a boolean because the lane's value is "this has been
waiting nine days". **Re-asserting the marker preserves `awaiting_since`** and
updates only the note — otherwise every agent that touched a stale ask would
make it look fresh.

`set_awaiting` both sets and clears, and clearing is deliberately
agent-reachable: an agent that raised a question and got its answer in-session
retracts its own marker rather than leaving an answered ask for Ken to click
away. The web UI's one-click clear is the same core call.

**The lifecycle rule is "a state only Ken sets", not "terminal":**

| transition | clears the marker? | why |
|---|---|---|
| work item → `closed` | **yes** | `closed` is Ken-only, so the ask is answered by definition |
| work item → `resolved` / `done` | **no** | "implemented, may still need a user test" is *the* canonical awaiting-Ken state |
| proposal → `done` / `declined` | **yes** | both are Ken's call on the bundle |
| program → `done` | **yes** | same |
| card → `Done` / `Cut` | **yes** (#1389) | the kanban is Ken's board end to end, so reaching its last column is him answering; `Cut` retires the ask the way archiving does |
| card → any other column | **no** | a card still moving is work in motion, not an answer |
| any node → `archived` | **yes** | the ask is moot |

Getting that backwards — clearing on `resolved`/`done` — would empty the lane of
precisely the rows it exists to show. `list_awaiting` additionally filters
archived and Ken-set-status rows on read, so a marker that escaped the write
rules still cannot haunt the board.

### Lifecycle invariants

Two further properties hold across the `covers` corpus and are maintained by
**convention, not constraint** — recorded here so a reader knows what keeps them
and what would break them (D-19; both zero-violation, verified 2026-07-23):

- **At most one live covering proposal per work item.** A WI is covered by at
  most one proposal in a non-terminal state — `refill-queue`'s survey skips WIs
  already covered by a live proposal.
- **Done proposals cover only terminal work items.** `sprint-ship`'s close-out
  resolves a proposal's covered WIs as it marks the proposal done.

These are deliberately not enforced in code: the write path stays a thin,
single-edge operation, and the invariants are cheap to check and cheaply
restored if a manual edit breaks one. If a future consumer needs them as a hard
guarantee, that is the point to revisit — not before.

### History

Migration 0014 backfilled orientation for edges written before this contract
existed. `create_proposal` and `upsert_report` had inserted
`(least(id), greatest(id))`, so their edges recorded node-id ordering rather
than meaning; `depends_on` was corrected by hand after sprint 008. The
migration asserts its own postcondition — no `covers` edge may point at a
proposal, no `finding` edge at a report — and refuses to apply if it cannot
reach that state.

Migration 0016 (LB-1 corpus true-up) finished the job on the read side: it
converted the five pre-0008 `Sprint: …` work-item bundles into archived-done
`sprint_proposal` nodes and re-pointed their 27 `covers` edges, retiring the
one legacy `covers` shape whose left endpoint was a work item. It also
consolidated the off-registry `related` / `follows_from` labels into
`related-to` and the `part_of` label into the built-in `parent_node_id`, so
every stored label is now one the registry declares — and added the nullable
`created` / `origin` provenance columns (NULL = predates provenance) that LB-2
begins stamping on new edges. Like 0014, it asserts its own postcondition and
refuses to half-apply.

Migration 0022 (sprint 043) closed the project half. 0016 §5 gave every
project-*less* proposal the unanimous project of the work items it covered;
0022 applies the same rule to proposals filed under the *wrong* project — five
pre-project-model bundles under the retired `Agent-Plan` umbrella and one
kprojects proposal under `claude-cleo`, four rows in all — and adds the CHECK
that stops a project-less proposal being written again. It reports, rather than
asserts, the residual: proposals that genuinely span two projects have no
mechanical answer and are left for a human.

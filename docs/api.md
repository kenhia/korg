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
| Work items | `create_work_item`, `get_work_item`, `update_work_item`, `list_work_items` |
| Cards | `create_card`, `update_card`, `list_cards` |
| Comments | `add_comment`, `list_comments`, `update_comment`, `delete_comment` |
| Reading-list links | `create_link`, `list_links`, `update_link`, `delete_link`, `mark_link_read` |
| Relationships | `relate`, `unrelate`, `neighbors` |
| Topics | `create_topic`, `get_topic`, `update_topic`, `list_topics`, `search_topics`, `archive_topic` |
| Daily planning | `create_daily_plan_item`, `list_daily_plan`, `move_daily_plan_item`, `reorder_daily_plan`, `set_daily_plan_completion`, `delete_daily_plan_item`, `daily_plan_history` |
| Sprint proposals | `propose_sprint`, `list_proposals`, `get_proposal`, `update_proposal` |
| Programs | `create_program`, `get_program`, `list_programs`, `update_program` |
| Awaiting Ken | `set_awaiting`, `list_awaiting` |
| Board | `get_board` |
| Reports | `create_report`, `list_reports`, `get_report` |
| Handoffs | `create_handoff`, `get_handoff`, `update_handoff` |
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
| REST request bodies | the `korg-core` struct itself — `NewWorkItem`, `WorkItemPatch`, `NewCard`, `CardPatch`, `NewLink`, `LinkPatch`, `NewProposal`, `ProposalPatch`, `ProjectPatch`, `NewTopic`, `TopicPatch`, `NewReport`, plus the operations in `korg_core::ops` |
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
`create_link`, `create_topic` and `propose_sprint`, on both transports.

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
| `{items, total, limit, offset}` | `list_work_items`, `list_cards`, `list_links`, `list_topics` |
| `{items, total, limit, truncated}` | `neighbors` (`truncated`, not `offset` — it caps rather than pages) |
| `{from, to, total, completed, items}` | `daily_plan_history` |
| bare array | `list_reports`, `list_areas`, `list_comments`, `list_daily_plan`, `list_awaiting` |
| `{items, omitted}` | `list_proposals`, `list_programs`, `list_projects` |

(`omitted` counts the rows a read's own defaults hid. `list_work_items` carries
it *in addition to* the paginated envelope.)

The bare-array reads are the ones with no natural paging story — a project has a
handful of areas, a node has a handful of comments, a day has a handful of plan
items.

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

The bare-array reads (`list_reports`, `list_areas`, `list_comments`,
`list_daily_plan`) have **no** `archived` filter — areas and comments are not
nodes and have no archived state, and reports and plan items have never exposed
one. `docs_drift.rs::the_archived_affordance_matches_the_instructions` fences
that split against the derived schemas, so a read cannot quietly join or leave
the class.

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

| Kind | Ending | Why |
|---|---|---|
| work items, cards, proposals, topics, reports, handoffs | `archived` only | each carries a thread and a decision history |
| projects | `archived` only (#828) | a project is the routing target for real work; `archived` already means "not a target for new work", so a delete would have nothing left to mean |
| links | `archived` **or** `delete_link` | a capture is either something you decided about or something you mistyped |
| areas | `delete_area` only | an area is a label with a description, not a record — nothing accumulates on it, so there is no history to archive |
| comments, daily-plan items, edges | delete only | correcting them *is* the edit |

**The refuse-if-referenced clause is the load-bearing half.** Without it the
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
| `list_topics` | `q` (name/description), `archived` | `name`, `node_id` |
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
| omitted | `open` + `resolved` + `done` — everything not terminal |
| `"all"` | every status |
| one of the four | exactly that status |

`resolved` and `done` stay visible deliberately: `done`'s visibility is a
promise `update_work_item`'s own schema makes, and `resolved` is the
implemented-but-Ken-may-still-want-to-see state. `closed` is the terminal one,
and hiding it is what finally makes that schema's "hidden by default" sentence
true — nothing on the MCP surface hid anything before this.

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

## Two-level reads

Collections say **whether** there is discussion; focused reads **inline** it.
Every commentable row (`WorkItemRow`, `CardRow`, `ProposalRow`, `ReportRow`,
`Topic`) carries an exact `comment_count`, and `ProposalRow` also carries
`covered_count`.

Focused reads inline up to 10 comments with a `comments_truncated` flag; page
the tail via `list_comments`:

- `GET /api/work-items/:wi` and the `get_work_item` tool — **the same shape**.
  They were one operation under one name with two shapes until sprint 015.
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
| `active` | proposals in `active`, pinned first then rank — each with `summary`, the work-item rollup `covered_count` + `open`/`resolved`/`done`/`closed`, and `synopsis` (korg #1003): the newest comment opening with the `⟦curator⟧` marker as `{body, updated}`, or `null` |
| `queue` | proposals in `proposed`, same row type, same order |
| `proposals_omitted` | `{done, declined, archived}` — the same envelope, meaning the same thing, as `list_proposals` |
| `proposal_edges` | korg #1003: every edge whose **both** endpoints are `active`/`queue` rows — `{left, right, label, directed, origin, created}`. `directed` comes from the registry (read undirected labels symmetrically); `origin`/`created` are the first read surface for D-17's write-side edge provenance, which is how curated edges (`origin: "kfdc-curator"`) stay distinguishable from human ones |
| `programs` | live programs, each carrying `slices` exactly as `get_program` returns them |
| `programs_omitted` | `{done, archived}` |
| `awaiting` | `list_awaiting`'s lane, unchanged |
| `depth` | per-project queue depth — every project, with its `status` |
| `reports` | the newest 5 |

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

**The board has no event feed.** #970 asked for "recent events/reports";
`reports` is the reports half, and `report_date` is the only date in korg that
records when something *happened*. There is no transition log, and the available
substitute — `node.updated` — advances on any edit, so a "recently shipped" list
built from it would date a proposal by its last tag edit. On a surface whose
promise is that it renders korg deterministically, a plausible wrong date is
worse than an absent panel.

Consumers: **kfdc** (`kai:~/src/tools/kfdc`, the widescreen overseer board) and
**korg-dash** (the kdeskdash Pi feed, which derives its panel counts from this
read rather than growing its own queries).

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

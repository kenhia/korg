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
| Work items | `create_work_item`, `get_work_item`, `update_work_item`, `list_work_items`, `survey_work_items` |
| Cards | `create_card`, `update_card`, `list_cards` |
| Comments | `add_comment`, `list_comments`, `update_comment`, `delete_comment` |
| Reading-list links | `create_link`, `list_links`, `update_link`, `mark_link_read` |
| Relationships | `relate`, `unrelate`, `neighbors` |
| Topics | `create_topic`, `get_topic`, `update_topic`, `list_topics`, `search_topics`, `archive_topic` |
| Daily planning | `create_daily_plan_item`, `list_daily_plan`, `move_daily_plan_item`, `reorder_daily_plan`, `set_daily_plan_completion`, `delete_daily_plan_item`, `daily_plan_history` |
| Sprint proposals | `propose_sprint`, `list_proposals`, `get_proposal`, `update_proposal` |
| Reports | `create_report`, `list_reports`, `get_report` |
| Projects and areas | `list_projects`, `create_project`, `update_project`, `list_areas`, `create_area` |

Two tools are not what their names suggest:

- `mark_link_read` is **deprecated** — `update_link` sets the read flag,
  disposition and tags in one transaction, which is what an agent recording a
  decision about a captured URL actually wants.
- `create_report` upserts: a re-run for the same `(source, date)` replaces the
  previous run's `finding` edges transactionally rather than accumulating them
  (D-7).

`update_project` takes `status`, `machines`, `deploy_to`, `category`,
`description`, `gh_repo` and `cn_path` — everything but the name. `cn_path` is
load-bearing: it is how an agent finds a project's working copy on disk.

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

Every list returns the same envelope (sprint 015):

```json
{ "items": [ ... ], "total": 42, "limit": 200, "offset": 0 }
```

`total` is the full **filtered** count before `limit`/`offset`, so you can page
without guessing and can tell a complete answer from a clipped one. `limit`
defaults to 200 and is clamped to 500.

**Archived rows are excluded by default.** This is deliberate (D-3): the common
question is "what is live", and the old behaviour silently mixed archived rows
into every answer. To see them:

| You want | REST | MCP |
|---|---|---|
| live only (default) | omit `archived` | omit `archived` |
| archived only | `?archived=true` | `"archived": true` |
| both | `?archived=all` | `"archived": null` |

Per-entity filters, all applied server-side — prefer them to fetching
everything and sifting:

| Operation | Filters | Ordering |
|---|---|---|
| `list_work_items` | `project` (name), `archived` | `wi_number` |
| `list_cards` | `status`, `project`, `archived` | `status`, `rank`, `node_id` |
| `list_links` | `disposition`, `read`, `archived` | `node_id` |
| `list_topics` | `q` (name/description), `archived` | `name`, `node_id` |
| `list_proposals` | `status`, `project` | pinned, `rank`, `node_id` |
| `neighbors` | `label`, `kind` | `node_id`, `rel_id` |

Every ordering carries an id tie-breaker, so equal ranks no longer shuffle
between calls. `list_proposals` is not enveloped — the queue is small and
ordered by hand.

`survey_work_items` remains the cross-project sweep: same envelope, no
`content`/`details`. Reach for `list_work_items` when you want one project's
items in full, and the survey when you want many projects' shape (D-10).

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

## Relationships

Any node can link to any other through a single `relationship` edge:
`(left_id, right_id, relationship)`, unique on that triple. The label reads
**left to right**.

### Label registry

korg declares the labels it writes or interprets. The registry lives in
`korg_core::relationships` and is the source this table is written from.

| Label | Direction | Reads | Endpoints |
|-------|-----------|-------|-----------|
| `covers` | directed | proposal **covers** work item | `sprint_proposal` → `workitem` |
| `finding` | directed | report **reported** work item | `report` → `workitem` |
| `depends_on` | directed | dependent **depends on** dependency | any → any |
| `related-to` | **undirected** | the two nodes are related | any → any |
| *anything else* | caller-defined | — | any → any |

**Directed** means the stored orientation carries meaning, so the reverse edge
is a *different* fact: `A depends_on B` and `B depends_on A` together are a
cycle, not a duplicate.

**Undirected** (`related-to`) means korg stores whichever order the caller
happened to pass and readers must ignore it — treat the edge as symmetric.
There is no canonicalization: two nodes related to each other have one edge in
some arbitrary orientation.

**Free-form labels** are legal and korg stores your order faithfully. It just
doesn't know what your label means, so it reports the direction as meaningful
and leaves the interpretation to you.

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

`relate(left, right, label)` requires that both endpoints exist (`not_found`
otherwise) and that they differ — self-edges are rejected (`invalid_input`),
and the schema enforces it with `relationship_no_self_edge`. Re-creating an
existing edge is a no-op that returns the same `rel_id`.

`covers` and `finding` edges are written for you by `propose_sprint` /
`create_report`; both insert the semantic orientation (proposal → work item,
report → work item).

### One legacy shape

`covers` edges predating migration 0008 join two **work items**: before the
`sprint_proposal` node kind existed, a work item titled `Sprint: …` stood in
for the bundle. Migration 0014 orients those bundle → member, so `direction`
is meaningful for them too, but they are the one `covers` shape whose left
endpoint is not a proposal. A reader that filters `covers` by
`kind=sprint_proposal` will not see them; one that filters by
`kind=workitem` from a proposal is unaffected.

### History

Migration 0014 backfilled orientation for edges written before this contract
existed. `create_proposal` and `upsert_report` had inserted
`(least(id), greatest(id))`, so their edges recorded node-id ordering rather
than meaning; `depends_on` was corrected by hand after sprint 008. The
migration asserts its own postcondition — no `covers` edge may point at a
proposal, no `finding` edge at a report — and refuses to apply if it cannot
reach that state.

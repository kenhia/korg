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
| Handoffs | `create_handoff`, `get_handoff`, `update_handoff` |
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

The **paginated** lists return an envelope (sprint 015):

```json
{ "items": [ ... ], "total": 42, "limit": 200, "offset": 0 }
```

`total` is the full **filtered** count before `limit`/`offset`, so you can page
without guessing and can tell a complete answer from a clipped one. `limit`
defaults to 200 and is clamped to 500.

The unpaginated lists return a **bare JSON array**. Which is which, verified
against the code in sprint 020 — this table was previously "every list returns
the same envelope", which was true of four of them:

| Shape | Reads |
|---|---|
| `{items, total, limit, offset}` | `list_work_items`, `list_cards`, `list_links`, `list_topics`, `survey_work_items` |
| `{items, total, limit, truncated}` | `neighbors` (`truncated`, not `offset` — it caps rather than pages) |
| `{from, to, total, completed, items}` | `daily_plan_history` |
| bare array | `list_proposals`, `list_reports`, `list_projects`, `list_areas`, `list_comments`, `list_daily_plan` |

The bare-array reads are the ones with no natural paging story — the proposal
queue is short and hand-ordered, a project has a handful of areas, a node has a
handful of comments, a day has a handful of plan items. Whether they *should*
be enveloped anyway for uniformity is open: **WI #579**.

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
between calls.

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

## Relationships

Any node can link to any other through a single `relationship` edge:
`(left_id, right_id, relationship)`, unique on that triple. The label reads
**left to right**.

### Label registry

korg declares — and, since LB-2, **enforces** — the labels it writes or
interprets. The registry lives in `korg_core::relationships`, is the source
this table is written from, and is the **complete set**: `relate` rejects any
label not in it.

| Label | Direction | Reads | Endpoints |
|-------|-----------|-------|-----------|
| `covers` | directed | proposal **covers** work item | `sprint_proposal` → `workitem` |
| `finding` | directed | report **reported** work item | `report` → `workitem` |
| `depends_on` | directed | dependent **depends on** dependency | any → any |
| `related-to` | **undirected** | the two nodes are related | any → any |
| `has_handoff` | directed | node **has** handoff | any → `handoff` |

**Directed** means the stored orientation carries meaning, so the reverse edge
is a *different* fact: `A depends_on B` and `B depends_on A` together are a
cycle, not a duplicate.

**Undirected** (`related-to`) means korg stores whichever order the caller
happened to pass and readers must ignore it — treat the edge as symmetric.
There is no canonicalization: two nodes related to each other have one edge in
some arbitrary orientation.

**The vocabulary is closed.** An unregistered label is `invalid_input` whose
message names the registry and the near-miss (`unknown label 'related'; …; did
you mean 'related-to'?`); a label with declared endpoint kinds (`covers`,
`finding`) also validates both ends. This is enforced in `korg_core` — the
single write path both transports share — deliberately **not** a DB trigger or
CHECK, which would duplicate the vocabulary outside Rust and re-invite the drift
the registry exists to prevent.

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
preserves the originals. Provenance is write-side only — no read surface exposes
`created`/`origin` yet (there is no consumer; the handoff flow is the likely
first).

Every `covers` edge is `sprint_proposal → workitem` — there is no exception.
(One used to exist: pre-0008 bundles were work items titled `Sprint: …`,
standing in for a proposal kind that did not yet exist. Migration 0016
converted those five into real archived-done proposals and re-pointed their
edges, so the dual shape is gone — see History.)

### Lifecycle invariants

Two properties hold across the `covers` corpus and are maintained by
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

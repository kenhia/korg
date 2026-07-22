# API contracts

Normative reference for korg's agent-facing surfaces. Where this document and
a tool description disagree, this document is right and the description is a
bug.

This file starts with the relationship model (sprint 014). The full response
and error contract currently lives in
[usage.md](usage.md#response-and-error-contract) and moves here as the
remaining cleanup bundles land.

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

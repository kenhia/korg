# Usage

korg unifies work items and kanban cards into one typed-node + generalized-edge
data model, served by a single `korg-api` process. Once it is running (see
[setup.md](setup.md)) you can drive it three ways: the web UI, the REST API, and
the MCP endpoint.

## Web UI

Browse to the address `korg-api` is serving (e.g. `http://<host>:8090`). The UI
covers:

- **Work Items** — create, edit, archive, set parent/area, and manage
  relationships and comments. Project selection is sticky across navigation.
- **Cards** — kanban cards with status, rank, tags, comments, and clickable
  launch links for URL fields.
- **Link Up** — relate any node to any other across kinds via the generalized
  `relationship` edge.
- **Planning** — the agent-planning queue: `sprint_proposal` nodes (a title +
  summary bundled with the work items they cover), drag-orderable by rank,
  with pin-to-top. Start/Decline/Done buttons drive the status lifecycle; a
  copy icon copies a `/start-sprint korg:<node_id>` prompt.

## REST API

All endpoints are unauthenticated (single-user, trusted-network posture) and
rooted at `/api`. Responses are JSON.

| Method & path                          | Description                          |
| -------------------------------------- | ------------------------------------ |
| `GET    /api/health`                   | Liveness check.                      |
| `GET    /api/projects`                 | List projects.                       |
| `POST   /api/projects`                 | Create a project.                    |
| `GET    /api/projects/recent`          | Most recently used project.          |
| `GET    /api/work-items`               | List work items: `{items,total,limit,offset}`; filters `project`, `archived`, `limit`, `offset`. |
| `POST   /api/work-items`               | Create a work item.                  |
| `GET    /api/work-items/survey`        | Slim, paginated work-item projection (no content/details) for cross-project surveys. |
| `GET    /api/work-items/:wi_number`    | Fetch a work item with inlined comments (same shape as the MCP tool). |
| `PATCH  /api/work-items/:wi_number`    | Update a work item.                  |
| `GET    /api/areas`                    | List areas.                          |
| `POST   /api/areas`                    | Create an area.                      |
| `GET    /api/cards`                    | List cards (enveloped); filters `status`, `project`, `archived`. |
| `POST   /api/cards`                    | Create a card.                       |
| `PATCH  /api/cards/:node_id`           | Update a card.                       |
| `GET    /api/nodes/:node_id/comments`  | List a node's comments (work item or card). |
| `POST   /api/nodes/:node_id/comments`  | Add a comment to a node.             |
| `DELETE /api/comments/:id`             | Delete a comment.                    |
| `GET    /api/links`                    | List links (enveloped); filters `disposition`, `read`, `archived`. |
| `POST   /api/links`                    | Create a link.                       |
| `PATCH  /api/links/:node_id`           | Update a link.                       |
| `GET/POST /api/topics`                 | List/search (`?q=`, enveloped) or create topics. |
| `GET/PATCH /api/topics/:node_id`       | Fetch or update a topic.             |
| `POST /api/topics/:node_id/archive`    | Archive or restore a topic.          |
| `GET/POST /api/daily-plan`             | List an inclusive range or plan a source node. |
| `PATCH /api/daily-plan/:node_id/completion` | Complete/uncomplete an item using server time. |
| `DELETE /api/daily-plan/:node_id`      | Delete an item from an open day.     |
| `PUT /api/daily-plan/:date/order`      | Replace an open day's item order.    |
| `POST /api/daily-plan/:node_id/move`   | Move an open item or copy a past item. |
| `GET /api/daily-plan/history`          | Historical range or `week`, `month`, `90days`, `year` preset. |
| `POST   /api/relationships`            | Create a generalized relationship.   |
| `DELETE /api/relationships/:id`        | Delete a relationship.               |
| `GET    /api/nodes/:id`                | Kind-agnostic preview of any node by id (powers find-by-ID + the preview panel); 404 if none. |
| `GET    /api/nodes/:id/neighbors`      | A node's edges: `{items,total,limit,truncated}`, optional `label`/`kind`/`limit` (see [api.md](api.md#relationships)). |
| `GET    /api/proposals`                | List sprint proposals (filters `status`, `project`). |
| `POST   /api/proposals`                | Propose a sprint: title + summary + covered `work_item_numbers` in one call. |
| `GET    /api/proposals/:node_id`       | Proposal detail: covered work items + comments. |
| `PATCH  /api/proposals/:node_id`       | Update a proposal (status, rank, pinned, archive). |

Example:

```bash
curl -s http://localhost:8090/api/work-items | jq
```

See [api.md](api.md) for the normative contracts: the collection-read
envelope and filters, the two-level read contract, and the relationship label
registry with its direction semantics.

### Response and error contract

Two rules hold across REST and MCP alike (WI #524, #525):

**Mutations validate, acknowledge, and return the entity.** Every create and
update checks that its target exists *and is the right kind*, then returns the
full row a read would return — no bare `{"ok": true}`. Deletes return
`{"deleted": true|false}`, so "nothing to delete" is distinguishable from
"deleted". A missing or wrong-kind target is a 404: `PATCH /api/cards/<id>`
where `<id>` is a work item's node fails and changes nothing, rather than
archiving the work item and reporting success.

**Errors are typed.** Error bodies are
`{"error": "<message>", "code": "<code>"}`, where `code` is stable and
machine-readable; MCP tool errors carry the same pair as
`{"message", "code"}` in an `isError` result.

| `code` | HTTP | Meaning |
| ------ | ---- | ------- |
| `invalid_input` | 400 | A value the caller supplied is not acceptable — unknown status, t-shirt size, `wi_type`, card status, disposition, unparseable date, an area outside the work item's project, an unresolvable parent, or a selector that does not resolve (unknown `project`/`project_id`/`area`/`area_id`, or an id *and* a name passed together). |
| `not_found` | 404 | The named or keyed entity does not exist — including single-item reads, which 404 rather than answering `200 null`. |
| `conflict` | 409 | Well-formed but at odds with server-enforced state (frozen past, stale reorder). |
| `internal` | 500 | A genuine server fault. Only this class should ever be retried blindly. |

Vocabularies are validated in korg-core, so an unknown value comes back as a
400 naming the whole allowed set rather than a 500 carrying raw Postgres text:

- `wi_status`: `open`, `resolved`, `done`, `closed`
- `wi_type`: `task`, `bug`, `chore`, `feature`, `research`, `tweak`, `brainstorm`
- `wi_tshirt`: `XS`, `S`, `M`, `L`, `XL`, `Huge`, `Unknown`
- card `status`: `Backlog`, `Research`, `OnDeck`, `Active`, `Done`, `Cut`
- link `disposition`: `Unread`, `Done`, `Revisit`, `Summarized`, `VaultSaved`
- proposal `status`: `proposed`, `active`, `done`, `declined`
- report `status`: `ok`, `attention`, `problem`
- project `status`: `active`, `maintenance`, `inactive`, `archived`

## MCP endpoint

The MCP server is mounted inside `korg-api` at `POST /mcp` using rmcp's
Streamable-HTTP transport in **stateless JSON mode** — each POST is an
independent JSON-RPC request/response, so no SSE session needs to be managed.
Host validation is disabled, matching the REST API's no-auth posture on a
trusted network.

Point any MCP client at:

```
http://<host>:8090/mcp
```

It exposes tools for work items, cards, reading-list links, generalized
cross-kind relationships, topics, source-linked daily planning, and agent-planning sprint
proposals, backed directly by `korg-core`.

List the available tools with a raw request:

```bash
curl -s -X POST http://localhost:8090/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'mcp-protocol-version: 2025-06-18' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | jq
```

## Work-item status lifecycle

Statuses are validated server-side (WI #285): `open`, `resolved`, `done`,
`closed` — anything else is rejected on create/update.

| Status | Set by | Meaning | Default list visibility |
|--------|--------|---------|------------------------|
| `open` | anyone | not started / in progress | visible |
| `resolved` | agent (or Ken) | implemented; may still need a user test or PR | visible |
| `done` | agent (or Ken) | agent satisfied with the implementation | visible |
| `closed` | **Ken only** (or at his direction) | out of sight unless searched for | hidden (filter unchecked) |

Typical flows: `open → resolved → done` (agent lifecycle), `→ closed` when
Ken sweeps; straight `open → done` for small verified agent work.

## Project metadata

Projects carry lifecycle metadata (WI #246): `status`
(`active | maintenance | inactive | archived`), `machines` (where the
working copy lives), `deploy_to` (where it deploys), `category`. The Work
Items rail shows only active+maintenance projects unless "show all" is
checked, renders names in stable per-name colors, and the ✎ control opens
an edit panel (everything editable except the name). Agents:
`update_project` MCP tool / `PATCH /api/projects/:name`.

**Writes take a project by name or by id** (WI #575): pass either `project`
(the name) or `project_id`, never both — and likewise `area` or `area_id`.
Names are resolved, never created, and an unknown one is a 400 that names
`list_projects` as the remedy. Full rules in
[api.md](api.md#selecting-a-project-or-an-area).

## Comments

Comments are editable (WI #232): ✎ in the UI, `update_comment` MCP tool,
`PATCH /api/comments/:id`. `created` is preserved; `updated` advances.

## Data model in brief

Everything is a **node** sharing one surrogate id space, so any kind can link to
any other through a single generalized `relationship` edge:

- **work item** — keeps a stable, user-facing serial `wi_number` (referenced by
  external project docs) that is *not* the primary key.
- **card** — kanban card (status, rank, tags).
- **link** — reading-list URL.
- **topic** — reusable planning identity with searchable name/description.
- **daily_plan_item** — ordered local-date occurrence linked to a work item,
  card, or topic; keeps an immutable display snapshot and optional completion
  timestamp. Past structure is frozen, while completion can be corrected.
- **sprint_proposal** — an agent-planning proposal (title, summary, status,
  drag-orderable `rank`, `pinned`); covers work items via the same
  `relationship` mechanism, label `covers`, rather than a dedicated join
  table.

Cross-cutting attributes (project, category, tags, archived, timestamps) live on
`node`. Projects are a unified taxonomy; areas stay project-scoped; tags and
category are shared across kinds.

## Importing existing kwi / kcard data

If you are migrating from the legacy `kwi` and `kcard` tools, see the
[migration guide](migration.md).

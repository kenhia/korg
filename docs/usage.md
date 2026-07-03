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
| `GET    /api/work-items`               | List work items (optional filters).  |
| `POST   /api/work-items`               | Create a work item.                  |
| `GET    /api/work-items/survey`        | Slim, paginated work-item projection (no content/details) for cross-project surveys. |
| `GET    /api/work-items/:wi_number`    | Fetch a work item by serial number.  |
| `PATCH  /api/work-items/:wi_number`    | Update a work item.                  |
| `GET    /api/areas`                    | List areas.                          |
| `POST   /api/areas`                    | Create an area.                      |
| `GET    /api/cards`                    | List cards.                          |
| `POST   /api/cards`                    | Create a card.                       |
| `PATCH  /api/cards/:node_id`           | Update a card.                       |
| `GET    /api/nodes/:node_id/comments`  | List a node's comments (work item or card). |
| `POST   /api/nodes/:node_id/comments`  | Add a comment to a node.             |
| `DELETE /api/comments/:id`             | Delete a comment.                    |
| `GET    /api/links`                    | List reading-list links.             |
| `POST   /api/links`                    | Create a link.                       |
| `PATCH  /api/links/:node_id`           | Update a link.                       |
| `GET    /api/slots`                    | List calendar timebox slots.         |
| `POST   /api/slots/generate`           | Generate slots from templates.       |
| `PATCH  /api/slots/:node_id`           | Update a slot.                       |
| `GET    /api/slot-templates`           | List slot templates.                 |
| `PUT    /api/slot-templates`           | Replace slot templates.              |
| `POST   /api/relationships`            | Create a generalized relationship.   |
| `DELETE /api/relationships/:id`        | Delete a relationship.               |
| `GET    /api/nodes/:id/neighbors`      | List a node's related neighbors.     |
| `GET    /api/proposals`                | List sprint proposals (optional `status` filter). |
| `POST   /api/proposals`                | Propose a sprint: title + summary + covered `work_item_numbers` in one call. |
| `PATCH  /api/proposals/:node_id`       | Update a proposal (status, rank, pinned, archive). |

Example:

```bash
curl -s http://localhost:8090/api/work-items | jq
```

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
cross-kind relationships, calendar timebox slots, and agent-planning sprint
proposals, backed directly by `korg-core`.

List the available tools with a raw request:

```bash
curl -s -X POST http://localhost:8090/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'mcp-protocol-version: 2025-06-18' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | jq
```

## Data model in brief

Everything is a **node** sharing one surrogate id space, so any kind can link to
any other through a single generalized `relationship` edge:

- **work item** — keeps a stable, user-facing serial `wi_number` (referenced by
  external project docs) that is *not* the primary key.
- **card** — kanban card (status, rank, tags).
- **link** — reading-list URL.
- **slot** — calendar timebox slot.
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

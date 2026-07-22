# korg — Ken's organizer

`korg` unifies `kwi` (work items) and `kcard` (kanban) into a single self-hosted
app: one typed-node + generalized-edges data model, a headless Linux backend,
and a web UI reachable from Windows browsers.

> Status: **Milestone 1 — faithful data foundation.** kwi and kcard remain the
> live tools and are left **frozen / read-only** until korg can demonstrably
> take over.

## Documentation

- [docs/setup.md](docs/setup.md) — install, configure, build, and run.
- [docs/usage.md](docs/usage.md) — web UI, REST API, and MCP endpoint.
- [docs/api.md](docs/api.md) — normative agent-facing contracts (relationship
  label registry, direction semantics).
- [docs/migration.md](docs/migration.md) — import legacy kwi + kcard data.

## Model

Everything is a **node** sharing one surrogate id space, so any kind can link to
any other through a single generalized `relationship` edge:

- **work item** — keeps a stable, user-facing serial `wi_number` (referenced by
  external project docs) that is *not* the primary key.
- **card** — kanban card (status, rank, tags).
- **link** — reading-list URL.
- **topic** — reusable source for daily planning.
- **daily plan item** — ordered source occurrence with a historical display snapshot.

Cross-cutting attributes (project, category, tags, archived, timestamps) live on
`node`. Projects are a unified taxonomy (kwi + kcard merged by name); areas stay
project-scoped; tags/category are shared across kinds.

## Crates

- `korg-core` — schema (sqlx migrations), domain repos (work items, cards,
  reading-list links, generalized relationships), topics, and daily planning,
  plus the domain vocabulary and error taxonomy both transports present
  (see [the response and error contract](docs/usage.md#response-and-error-contract)).
- `korg-migrate` — one-shot, fidelity-verified import of kwi + kcard data.
- `korg-mcp` — MCP tool surface (rmcp) over the korg domain, served by `korg-api`.

## MCP server

The MCP server is mounted **inside `korg-api`** at `POST /mcp`, using rmcp's
Streamable-HTTP transport. A single `korg-api` binary therefore serves the web
UI, the REST API, and the MCP endpoint — dev/client machines need nothing
installed; point an MCP client at the URL:

```
http://<host>:8090/mcp
```

It exposes 44 tools for work items, cards, reading-list links, generalized
cross-kind relationships, topics, and source-linked daily planning, backed directly by
`korg-core`.

Transport notes:
- **Stateless mode** with JSON responses — each POST is an independent
  request/response (no SSE session to manage), ideal for a single-user tool.
- **Host check disabled** (`disable_allowed_hosts`) so korg is reachable via any
  hostname on the trusted network — the same no-auth posture as the REST API.

Quick smoke test against a running korg-api:

```bash
curl -s -X POST http://localhost:8090/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'mcp-protocol-version: 2025-06-18' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
```

## Web UI

`korg-api` (axum) exposes the REST API and serves the SvelteKit bundle when
`KORG_WEB_DIR` points at a build.

```bash
cd web && pnpm install && pnpm build
DATABASE_URL=... KORG_TIMEZONE=Etc/UTC KORG_WEB_DIR=$PWD/build KORG_LISTEN_ADDR=0.0.0.0:8090 \
  cargo run -p korg-api          # open http://<host>:8090

# Or hot-reload the UI against a running API:
cd web && KORG_API=http://localhost:8090 pnpm dev    # http://localhost:5173
```

End-to-end smoke tests (Playwright/Chromium) run against a running korg-api:

```bash
cd web
npx playwright install chromium                       # once
KORG_E2E_URL=http://127.0.0.1:8090 npx playwright test
```

## Migration & fidelity

Imports run off frozen, read-only `pg_dump` snapshots — the source databases
are never mutated.

```bash
# 1. Snapshot the live sources (read-only).
KCARD_DOCKER_CONTAINER=kcard-pg \
KCARD_ENV_FILE=../kcard/deploy/local/.env \
just snapshot

# 2a. Verify fidelity invariants F1-F7 (CI-style gate, throwaway DB).
just verify-import

# 2b. Or load the data into a real korg database. --reset first TRUNCATEs
# EVERY node kind (work items, cards, links, topics, daily plans, proposals,
# reports) plus projects and areas, so it demands an explicit confirmation.
KORG_DATABASE_URL=postgres://korg:korg@host:5432/korg \
KORG_RESET_CONFIRM=yes just import --reset
```

`verify-import` proves the import is faithful to both sources: count parity,
`wi_number` preservation (+ sequence continues at max+1), field-by-field
integrity, relationship and parent-hierarchy preservation, project merge, and
project-scoped areas.

## License

[MIT](LICENSE) © Ken Hiatt

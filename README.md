# korg — Ken's organizer

`korg` unifies [`kwi`](../kwi) (work items) and [`kcard`](../kcard) (kanban)
into a single self-hosted app: one typed-node + generalized-edges data model,
a headless Linux backend, and a web UI reachable from Windows browsers.

> Status: **Milestone 1 — faithful data foundation.** kwi and kcard remain the
> live tools and are left **frozen / read-only** until korg can demonstrably
> take over.

## Model

Everything is a **node** sharing one surrogate id space, so any kind can link to
any other through a single generalized `relationship` edge:

- **work item** — keeps a stable, user-facing serial `wi_number` (referenced by
  external project docs) that is *not* the primary key.
- **card** — kanban card (status, rank, tags).
- *(later)* calendar timebox slots, reading-list URLs.

Cross-cutting attributes (project, category, tags, archived, timestamps) live on
`node`. Projects are a unified taxonomy (kwi + kcard merged by name); areas stay
project-scoped; tags/category are shared across kinds.

## Crates

- `korg-core` — schema (sqlx migrations) and domain model.
- `korg-migrate` — one-shot, fidelity-verified import of kwi + kcard data.

## Migration & fidelity

Imports run off frozen, read-only `pg_dump` snapshots — the source databases
are never mutated.

```bash
# 1. Snapshot the live sources (read-only).
KCARD_DOCKER_CONTAINER=kcard-pg \
KCARD_ENV_FILE=../kcard/deploy/local/.env \
just snapshot

# 2. Import into a throwaway korg DB and verify fidelity invariants F1-F7.
just verify-import
```

`verify-import` proves the import is faithful to both sources: count parity,
`wi_number` preservation (+ sequence continues at max+1), field-by-field
integrity, relationship and parent-hierarchy preservation, project merge, and
project-scoped areas.

## License

MIT

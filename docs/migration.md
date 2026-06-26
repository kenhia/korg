# Migration (kwi + kcard → korg)

> This guide is specific to migrating from the legacy `kwi` (work items) and
> `kcard` (kanban) tools. If you are starting fresh, you can skip it — korg
> creates its own schema on first run.

korg's importer is **one-shot and fidelity-verified**. It runs entirely off
frozen, read-only `pg_dump` snapshots, so the source databases are never
mutated.

## 1. Snapshot the live sources (read-only)

`scripts/snapshot.sh` captures `pg_dump` snapshots into `snapshots/` (which is
gitignored). Configure the sources via environment variables — credentials come
from the environment / `~/.pgpass`, never from the repo:

```bash
# kwi via host pg_dump; kcard via a running Docker container
KCARD_DOCKER_CONTAINER=kcard-pg \
KCARD_ENV_FILE=../kcard/deploy/local/.env \
just snapshot
```

| Variable                 | Purpose                                                      |
| ------------------------ | ----------------------------------------------------------- |
| `KWI_DATABASE_URL`       | kwi source (default `postgresql://ken@gratch:5432/workitems`). |
| `KCARD_DOCKER_CONTAINER` | kcard Postgres container name (Docker source).              |
| `KCARD_ENV_FILE`         | optional env file sourced for `POSTGRES_USER/PASSWORD/DB`.  |
| `KCARD_DATABASE_URL`     | kcard source URL (alternative to the Docker container).     |

## 2. Verify fidelity (recommended)

Prove the import is faithful to both sources before touching a real database.
This runs against a throwaway Postgres container and asserts invariants F1–F7:

```bash
just verify-import     # cargo test -p korg-migrate --test fidelity
```

The invariants cover: count parity, `wi_number` preservation (and that the
sequence continues at max+1), field-by-field integrity, relationship and
parent-hierarchy preservation, project merge, and project-scoped areas.

## 3. Import into a korg database

```bash
KORG_DATABASE_URL=postgres://korg:korg@host:5432/korg just import --reset
```

| Variable            | Purpose                                                                 |
| ------------------- | ----------------------------------------------------------------------- |
| `KORG_DATABASE_URL` | (required) destination korg database.                                   |
| `KORG_ADMIN_URL`    | admin connection used to create scratch source DBs (defaults to the `postgres` db on the same host). |
| `KORG_SNAPSHOTS`    | directory holding `kwi.dump` / `kcard.dump` (default `./snapshots`).    |

Flags:

- `--reset` — `TRUNCATE` korg work items / cards / projects / areas before
  importing (clears any previous import).

The importer restores the snapshots into scratch source databases, reads them,
and writes into korg — the original `kwi` / `kcard` databases are never touched.

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

One field is deliberately **not** asserted byte-for-byte. kwi's `cn_path` lands
in korg's `src_path` (renamed in 0019), a column with a canonical form backed by
a CHECK constraint — so the importer canonicalizes on the way in via
`repo::canonical_src_path`, and F5 asserts *that* transform rather than the raw
string. Fidelity here means "the same path, said the one legal way"; asserting
the raw value would be asserting a bug, and importing it would simply be
rejected for the rows kwi holds un-prefixed.

## 3. Import into a korg database

```bash
KORG_DATABASE_URL=postgres://korg:korg@host:5432/korg just import
```

| Variable            | Purpose                                                                 |
| ------------------- | ----------------------------------------------------------------------- |
| `KORG_DATABASE_URL` | (required) destination korg database.                                   |
| `KORG_ADMIN_URL`    | admin connection used to create scratch source DBs (defaults to the `postgres` db on the same host). |
| `KORG_SNAPSHOTS`    | directory holding `kwi.dump` / `kcard.dump` (default `./snapshots`).    |
| `KORG_RESET_CONFIRM` | must be `yes` for `--reset` to run at all (WI #528).                   |

Flags:

- `--reset` — `TRUNCATE node, project, area … CASCADE` before importing. Read
  that literally: it destroys **every** node kind — work items, cards,
  reading-list links, sprint proposals, reports, handoffs, programs and
  schedules — not just the entities this import creates. (That list is
  `vocab::NODE_KINDS`; it had drifted three kinds behind the database until
  sprint 054, which is a poor property for a warning about irreversible
  deletion.) The import is one-shot and long
  finished, so a `--reset` against a live korg database is almost certainly a
  mistake; korg-migrate refuses unless `KORG_RESET_CONFIRM=yes` and prints the
  per-kind inventory it is about to destroy.

The importer restores the snapshots into scratch source databases, reads them,
and writes into korg — the original `kwi` / `kcard` databases are never touched.

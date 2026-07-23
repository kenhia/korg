# Operations

Running the live korg instance: where it is, how it is deployed, how it is
backed up, and how to look inside it.

This file exists because none of that was written down here. The 2026-07 deep
review's first phase concluded korg had **no database backup** — and was wrong.
Backups had been running nightly since 2026-07-08; the fact simply lived in
another repo's README and a closed work item's comment (F-24). A reviewer, or an
agent, working from this repository had no way to know. So: anything an operator
needs in an emergency is written down here, in the repo, next to the code.

## The deployment

| | |
|---|---|
| Host | `kubsdb` (192.168.1.60) |
| Container | one Docker container named `korg`, network `kubsdb-net`, `--restart unless-stopped` |
| Port | `5674`, published on `127.0.0.1` and `192.168.1.60` (**not** `0.0.0.0` — `tailscale serve` owns the tailnet listener) |
| User-facing URL | `https://kubsdb.encke-wahoo.ts.net:5674/` over Tailscale |
| Database | Postgres in the `postgresql` container on the same host, database `korg` |
| Image | built locally and shipped over SSH; there is no registry |

One process serves the web UI, the REST API and the MCP endpoint. Deploying is
therefore all-or-nothing — there is no way to ship a UI change without shipping
the API.

The deploy procedure itself lives in the `deploy-kubsdb` skill
(`.claude/skills/deploy-kubsdb/SKILL.md`), which is the executable version of
this section. It refuses to build from a dirty working tree and stamps every
image with the commit it was built from, so `docker inspect` on a running
container answers "what is actually deployed" (D-9).

### What is running right now

```bash
ssh kubsdb 'docker inspect korg --format "{{index .Config.Labels \"org.opencontainers.image.revision\"}}"'
```

That label is the git SHA the image was built from. An image built before this
convention landed has no label, which is itself an answer: it predates sprint
018.

### Logs

```bash
ssh kubsdb 'docker logs --tail 200 korg'
```

`KORG_LOG` sets the `tracing` filter (default `info`); `KORG_LOG=korg_api=debug`
turns on per-request detail. Changing it means recreating the container, so
prefer it for an investigation rather than as a standing setting.

### Rollback

Old images stay in kubsdb's local store — `docker images korg` lists them.
Recreate the container from a previous image id using the same `docker run` the
deploy skill uses. Rollback is image-only: it does **not** undo a schema
migration, and korg's migrations run automatically at startup. Rolling back
across a migration boundary needs a restore (below), not a re-tag.

## Backups

**korg's database is backed up nightly.** It is not optional and it is not
something you need to set up — it already runs.

| | |
|---|---|
| What | `pg_dump --clean --if-exists` of the whole `korg` database |
| Where | `/gratch/backups/korg/korg-<YYYYMMDD>-<HHMMSS>.sql.gz` on kubsdb — `/gratch` is NAS-backed, so the dumps **survive the loss of kubsdb itself** |
| When | `korg-backup.timer`, `OnCalendar=*-*-* 03:17`, `RandomizedDelaySec=10m`, `Persistent=true` (a missed run fires at next boot) |
| Retention | 14 days, pruned by `find -mtime +14 -delete` |
| Script | `/usr/local/bin/korg-backup.sh` on kubsdb |
| Managed by | k-homelab's `korg-backup` recipe — that repo is the source of truth for the unit files; this section describes what they do |

The script refuses to keep a bad dump: it writes to a `.partial`, verifies the
gzip stream, and rejects anything under 10 KB before renaming it into place. So
a file present in `/gratch/backups/korg/` is a file that passed those checks —
absence, not corruption, is the failure mode to watch for.

### Is the backup current?

```bash
ssh kubsdb 'ls -la /gratch/backups/korg/ | tail -3; systemctl list-timers korg-backup.timer'
```

The newest dump should be from last night and should be *larger* than its
predecessors — the database only grows. A shrinking dump is worth stopping to
understand.

Check this **before** a deploy, not after. A deploy applies pending schema
migrations automatically, and the only thing that makes a bad migration
survivable is a dump from before it ran. The deploy skill's preflight does this
check for exactly that reason.

### Restore

Dumps are gzipped plain SQL taken with `--clean --if-exists`, so restoring over
an existing (including a broken) database works without dropping it first:

```bash
# Into the live database — destructive, and the last resort.
ssh kubsdb bash -s <<'EOF'
  gunzip -c /gratch/backups/korg/korg-20260723-032356.sql.gz \
    | docker exec -i postgresql psql -U postgres -d korg
EOF
```

Stop the `korg` container first (`docker stop korg`) so nothing writes during
the restore, and start it after. Note this is `psql`, not `pg_restore` — the
dumps are plain SQL, not custom-format archives, and reaching for `pg_restore`
on one produces a confusing error at the worst possible moment.

Restoring into a **scratch** database is the same command with a different
target, and is what you almost always want first:

```bash
ssh kubsdb bash -s <<'EOF'
  docker exec postgresql createdb -U postgres korg_scratch
  gunzip -c /gratch/backups/korg/korg-20260723-032356.sql.gz \
    | docker exec -i postgresql psql -U postgres -d korg_scratch
  docker exec postgresql psql -U postgres -d korg_scratch -tAc \
    'select kind, count(*) from node group by kind order by kind'
EOF
```

### Restore drills

A backup nobody has restored is a hypothesis. The last verified restore was
2026-07-08 (WI #234), into a scratch database as above. Worth repeating roughly
quarterly, and worth repeating after any change to the schema's shape — the
drill costs a few minutes and is the only thing that converts "we have dumps"
into "we can recover".

## Querying the live database

Reads against production are legitimate and sometimes necessary — sprint 014's
relationship-orientation migration could only be designed correctly because the
live edge corpus was queried first, which is how 27 legacy `covers` edges
joining two *work items* were found. They invalidated the migration's stated
premise. No amount of reading the code would have surfaced them.

```bash
ssh kubsdb "docker exec postgresql psql -U korg -d korg -tAc \
  'select kind, count(*) from node group by kind order by kind'"
```

Two things that cost real time to discover, both worth knowing before you need
them:

- The Postgres container is named **`postgresql`**, not `korg-pg`. (`korg-pg` is
  a dead container on `kai`, left over from before the move.)
- **kubsdb's login shell is fish.** A bare `ssh kubsdb '…'` runs under fish,
  which mis-parses `$()`, `$$`-quoted SQL and `{{…}}`. Pipe anything non-trivial
  through `ssh kubsdb bash -s <<'EOF'` instead.

`-tAc` gives tuples-only, unaligned output, which is what you want when piping
into `sort`/`awk`/`jq`. Use the `korg` role for reads; `postgres` is the
superuser the backup and restore paths use.

**These are read paths.** Writes go through the REST or MCP API so that
validation, the vocabulary checks and the `updated` triggers all apply — a
direct `UPDATE` bypasses every invariant korg-core enforces, including the ones
recent sprints added specifically to stop silent wrong writes.

### Rehearsing a data migration

Any migration that touches existing rows should be rehearsed against a restored
dump before it goes near production. Sprint 014 did this and it is worth
promoting from a sprint note to a standing recipe:

1. Restore last night's dump into a scratch database (above).
2. Baseline the shape you expect to change — per-kind node counts, per-label
   edge counts, whatever the migration claims to touch.
3. Run the migration against the scratch database.
4. Diff the same counts. The interesting number is the one you did not predict.

Step 4 is the point. Sprint 014's rehearsal is what turned "orient the `covers`
edges" from a one-line assumption into a migration that asserts its own
postcondition and refuses to apply if it cannot reach it.

## Health and verification

`GET /api/health` is a liveness check: it proves the process is up and answering.
It does **not** prove the deploy did what you wanted — a container running last
week's image passes it happily.

`scripts/post-deploy-check.sh` is the real check. It exercises the read paths on
both transports, does an idempotent write, and diffs per-kind row counts against
a baseline captured before the deploy:

```bash
# Before deploying:
bash scripts/post-deploy-check.sh --baseline /tmp/korg-baseline.json

# After:
bash scripts/post-deploy-check.sh --compare /tmp/korg-baseline.json
```

The counts diff matters more than it sounds like it should. During the sprint 015
deploy it explained a +1 discrepancy that would otherwise have looked like a new
archived-rows filter dropping data; the real cause was a work item created in the
UI mid-build. Without a baseline, that is a scary unexplained number at the exact
moment you are deciding whether to roll back.

Beyond that, verify what the *sprint* changed — that is per-sprint work no fixed
script can do for you (`sprint-ship` Phase 7).

## Destructive operations

`korg-migrate --reset` runs `TRUNCATE node, project, area … CASCADE`. Read that
literally: it destroys **every** node kind — work items, cards, links, topics,
daily plan items, sprint proposals and reports — not merely the entities the
legacy import creates. The import it belongs to is one-shot and long finished, so
a `--reset` against the live database is almost certainly a mistake.

It refuses to run without `KORG_RESET_CONFIRM=yes` and prints the per-kind
inventory it is about to destroy first. See [migration.md](migration.md).

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
| Image | built on kai from committed code, pushed to the homelab docker registry `kubsdb.encke-wahoo.ts.net:5000` as `korg:<short-sha>` + `korg:latest`, pulled by kubsdb (korg #1011) |
| Run config | [`deploy/docker-compose.yml`](../deploy/docker-compose.yml), copied to `/datastore/korg/docker-compose.yml` at deploy time — the one declaration of network, ports, restart policy and env file |
| Credential | korg authenticates as the **non-superuser role `korg`**. The container reads `/datastore/korg/korg.env` (mode 0600); the recoverable copy is k-homelab's age store, `kubsdb-korg-db-password`. See [Cold start](#cold-start) |

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

Every build deployed since sprint 048 is a named tag in the registry, which is
where rollback targets come from:

```bash
curl -s https://kubsdb.encke-wahoo.ts.net:5000/v2/korg/tags/list

ssh kubsdb bash -s <<'EOF'
  cd /datastore/korg
  export KORG_IMAGE=kubsdb.encke-wahoo.ts.net:5000/korg:<short-sha>
  docker compose pull && docker compose up -d
EOF
```

The override is not sticky: the next deploy's plain `docker compose up -d`
returns to `latest`. Images predating the cutover are not in the registry but
survive in kubsdb's local store under their unqualified `korg:<short-sha>`
names (`ssh kubsdb 'docker images korg'`), and work as `KORG_IMAGE` values too.

Rollback is image-only: it does **not** undo a schema migration, and korg's
migrations run automatically at startup. Rolling back across a migration
boundary needs a restore (below), not a re-tag.

Not every migration is such a boundary, and it is worth knowing which before
you need to know it. A migration that only **changes data in existing columns**
can be rolled back across with a plain re-tag, because the old image reads the
same schema — 0018 (project categories) is one of these. A migration that
changes the **schema** cannot — 0017 (handoff) is one of those, and crossing it
backwards needs the dump restore. Each migration's header says which it is.

**0019 (`cn_path` → `src_path`) is a schema boundary**, and the cheapest kind:
an old image selects `cn_path` and fails against the renamed column, but the
reversal is two statements rather than a restore, because no data was destroyed.

```sql
ALTER TABLE project RENAME COLUMN src_path TO cn_path;
ALTER TABLE project DROP CONSTRAINT project_src_path_canonical;
```

Worth stating explicitly, because "schema boundary" and "needs a dump restore"
are not the same claim — 0017 needs the restore, 0019 does not. A rename that
loses nothing is reversible by renaming back; what makes it a boundary is only
that the rollback is not *automatic*.

**0020 (project `notes` + the capped `description`) is milder still.** Adding a
nullable column breaks nothing in the dangerous direction — an old image ignores
`notes`. What an old image *would* show is the six projects whose over-length
`description` moved into `notes`, rendered blank. Nothing is lost; some prose is
temporarily invisible.

```sql
UPDATE project SET description = notes, notes = NULL
 WHERE description IS NULL AND notes IS NOT NULL;
ALTER TABLE project DROP CONSTRAINT project_description_routing_line;
```

**0021 (proposal `notes` + the capped `summary`) is 0020's shape at larger
scale**: a nullable column plus a data move, so an old image ignores `notes` and
nothing breaks in the dangerous direction. What it *would* show is the 96
proposals whose summary is now its first paragraph plus a ` […]` marker — the
analysis is intact in `notes`, just not where a pre-036 image looks.

```sql
UPDATE sprint_proposal SET summary = notes, notes = NULL WHERE notes IS NOT NULL;
ALTER TABLE sprint_proposal DROP CONSTRAINT sprint_proposal_summary_routing_line;
```

That reversal restores the pre-036 corpus exactly, which is the point — but note
it also discards any routing contract written *after* the deploy (036's D-4 data
pass re-authors the live queue by hand). Rolling back across 0021 is therefore
cheap for history and lossy for whatever was authored since; take a dump first if
the pass has already run.

**0022 (single-project proposals) is a data move plus a CHECK.** The four
proposals it re-files (599, 601, 602 → their real projects; 747 → kprojects) are
plain column values an old image reads happily, so that half is not a boundary
at all. The constraint is: an image built before sprint 043 has no
project-required check of its own, so a `propose_sprint` with no project reaches
the database and comes back as a raw constraint violation in a 500 rather than
the `invalid_input` the current core returns.

```sql
ALTER TABLE node DROP CONSTRAINT node_sprint_proposal_has_project;
```

The re-filings are not reversed by that, and should not be — they corrected
mis-files. Undoing one is a hand-written `UPDATE node SET project_id = …`; the
sprint record (`sprints/043-single-project-proposals.md`) names all four.

0020 also narrows `PROJECT_STATUSES` to `active | archived`, converting
`maintenance` → `active` and `inactive` → `archived` on the way. That conversion
is **not** decoration: it was added after a rehearsal against a nightly dump
failed the migration's own postcondition with `off_vocab=3`. Production happened
to be clean at deploy time because the last three `inactive` rows had just been
archived by hand — but any database restored from a dump predating that would
have failed the migration and crash-looped the container at startup. If you ever
restore an old dump and roll forward, this is why it now just works.

## Cold start

Everything above assumes a running container and a populated `/datastore/korg/`.
After a kubsdb rebuild there is neither, and three things must exist before
`docker compose up -d` can work:

| | |
|---|---|
| the `korg` **role** | `pg_dump` of a database carries no `CREATE ROLE` — roles are cluster-level, and only `pg_dumpall --roles-only` would carry them. Restoring last night's dump therefore does **not** recreate the role korg authenticates as, and every `GRANT`/`ALTER … OWNER` in the dump fails without it. |
| the `korg` **database** | Created empty; korg applies its migrations at startup. If you are restoring a dump, create it here and restore *before* starting the container. |
| `/datastore/korg/korg.env` | Mode 0600, holding `DATABASE_URL` and `KORG_TIMEZONE`. |

[`deploy/cold-start.sh`](../deploy/cold-start.sh) does all three, idempotently.
It is the **only** korg procedure that reads the password out of k-homelab's age
store — a routine deploy never moves the value off kubsdb.

```bash
ssh kubsdb 'mkdir -p /datastore/korg'
scp deploy/docker-compose.yml deploy/cold-start.sh kubsdb:/datastore/korg/

# The password travels over the pipe and nowhere else: not argv (visible in
# `ps`), not the environment (visible in /proc), not a file, not shell history.
ssh kubs0 'cd ~/k-homelab && bin/secret get kubsdb-korg-db-password' \
  | ssh kubsdb 'bash /datastore/korg/cold-start.sh'

ssh kubsdb 'cd /datastore/korg && docker compose pull && docker compose up -d'
```

The `pull` is what makes this work on a host with no korg image at all. Before
sprint 048 the image reached kubsdb only as a `docker load` from a kai build,
so a rebuilt host silently needed a *fourth* thing this section never listed —
someone to run a deploy first. Pulling a registry-qualified image closes that:
cold start now needs only the registry, and the registry's blobs are restored
by k-homelab's own DR before korg's.

That is also the new dependency to know about. korg's deploy needs the
`registry` container up on kubsdb and tailscaled serving `:5000`; a kubsdb loss
that takes out `/datastore` takes out both korg and the registry, so k-homelab
restores the package store first. The blobs live in `/datastore/packages` and
are mirrored to the NAS nightly with the rest of it.

The k-homelab checkout lives on **kubs0**, which is why the first half of that
pipeline runs there. `bin/secret` decrypts with the local machine's own SSH key
— there is no passphrase to supply.

The script prints two truncated sha256 fingerprints and never the value, which is
enough to confirm the deployed credential matches the store without putting the
password in your scrollback or an agent transcript:

```bash
ssh kubs0 "cd ~/k-homelab && bin/secret get kubsdb-korg-db-password" \
  | tr -d '\n' | sha256sum | cut -c1-12
```

It also **logs in as the role** before reporting success. That check is doing
real work only because it connects through the `postgresql` container's network
name rather than loopback: kubsdb's `pg_hba.conf` trusts `127.0.0.1/32`
unconditionally, so a loopback check would succeed against any password at all.
Storing a credential and having a working one are different claims — the homelab
paid for that lesson once already.

**If the store has no value** — a genuinely fresh cluster, or a lost store —
generate one, run `cold-start.sh` with it, and write it back with `bin/secret set
kubsdb-korg-db-password` in the same sitting. A store that silently disagrees
with the cluster is worse than an empty one: it looks like a working backstop
and is not.

### What cold start does not do

It does not restore data. On a rebuilt host that lost `/datastore`, run it to put
the role and database in place, restore the newest dump from
`/gratch/backups/korg/` per [Restore](#restore), and *then* start the container
— korg's migrations will happily establish a schema against an empty database,
which is not what you want immediately before overwriting it.

The cluster-level half of this also belongs to k-homelab, because kubsdb's DR
runbook does: see that repo's `dr/kubsdb.md` step 8, and k-homelab #1005.

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
both transports, does an idempotent write, and diffs per-kind row counts **and
schema state** against a baseline captured before the deploy:

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

The **schema section** (WI #584) covers what no row count can see. korg applies
migrations automatically at container start, so a deploy can move schema state
while every REST total stays identical — sprint 020 shipped a migration whose
entire contract was to leave the node id sequence alone, and verifying that meant
psql over SSH by hand. The check now records the migration count and max version,
`node` min/max/count, and the `node_id_seq` state, reporting every change and
**failing** on the two that are never benign: a migration count that dropped (the
running image is older than the schema it is talking to) or a rewound sequence
(the next insert collides with an id already in use).

It reads that over SSH, so it needs the DB host — `KORG_DB_SSH`, default
`kubsdb`. Set it empty to skip the section; it also skips itself, with a note and
no failure, when the host is unreachable, so the script still runs against a
local instance. A baseline captured without schema access compares counts only
and says so.

Beyond that, verify what the *sprint* changed — that is per-sprint work no fixed
script can do for you (`sprint-ship` Phase 7).

## Destructive operations

`korg-migrate --reset` runs `TRUNCATE node, project, area … CASCADE`. Read that
literally: it destroys **every** node kind — work items, cards, links, sprint
proposals and reports — not merely the entities the
legacy import creates. The import it belongs to is one-shot and long finished, so
a `--reset` against the live database is almost certainly a mistake.

It refuses to run without `KORG_RESET_CONFIRM=yes` and prints the per-kind
inventory it is about to destroy first. See [migration.md](migration.md).

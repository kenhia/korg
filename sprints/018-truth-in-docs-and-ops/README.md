# Sprint 018 — truth in docs and ops

Proposal `korg:557` ("Cleanup B5"). Covers WI #543, #544, #545, #546 and #572.

The 2026-07 deep review's fifth bundle, and the one with no code in it worth
speaking of. Every finding here is the same finding wearing a different hat:
**korg tells the truth about itself only by accident.** A README that says korg
is a milestone-1 prototype. A tool count nobody re-counted. An endpoint table
missing four routes. A backup that runs nightly and is invisible from this repo.
A deploy that ships whatever happens to be in the working tree and records
nothing about what it shipped.

None of it breaks anything. All of it makes the next person — usually an agent,
sometimes Ken — reason from a false picture. The review's own Phase 1 is the
proof: it read this repository carefully and concluded **korg had no database
backup**. Backups had been running nightly for two weeks.

## The organising idea

Prose about *why* ages gracefully. Inventories of *what* do not — they are wrong
the moment the code changes, and nothing complains.

So the fix is not "write better docs". It is:

1. **Exactly one normative home per fact.** The tool list lives in
   `docs/api.md`, once. The REST table lives in `docs/usage.md`, once. Env vars
   live in `docs/setup.md`, once. Ops lives in a new `docs/operations.md`.
2. **Every inventory is drift-tested.** If the docs disagree with the code, a
   test fails and names the difference.
3. **Delete the copies.** Three tool-category lists became one plus two
   pointers. A copy that cannot drift is a copy that does not exist.

Point 3 is the one worth insisting on. The alternative — testing that all three
copies agree — keeps the maintenance burden and merely automates the nagging.

## WI #543 — CI, and a test suite that passes on a clean checkout

There was no CI. Every gate was voluntary. Worse, `cargo test --workspace`
**failed on a fresh clone**: `korg-migrate`'s three suites restore
`snapshots/*.dump`, frozen dumps of the legacy kwi/kcard databases that are
gitignored and machine-local. The suite passed here only because this machine
happens to have them.

`KORG_SNAPSHOT_TESTS` now decides, with a default chosen so nobody has to think:

| Value | Behaviour |
|---|---|
| unset | run iff both dumps are present, else skip with a message |
| `1` | required — missing dumps fail loudly (what `just verify-import` sets) |
| `0` | skipped even when present (what CI sets) |

The default is the useful one. A clean checkout is green; this machine keeps its
coverage; neither needs a flag. Requiring `KORG_SNAPSHOT_TESTS=1` for the normal
case would have been simpler to describe and would have quietly ended the
suites' useful life the first time someone forgot it.

Rust has no native skip, so a skip prints a line saying so. A silent skip and a
pass look identical, which is how a suite dies without anyone noticing.

All three modes were verified for real, including moving `snapshots/` aside to
simulate a clean checkout:

- unset + dumps present → all three run
- unset + dumps absent → all three skip, with the message naming `just snapshot`
- `=1` + dumps absent → **fails**, pointing at `just snapshot`
- `=0` → skips regardless

`.github/workflows/ci.yml` runs fmt, the generated-file freshness check, clippy
`-D warnings` and the full test suite in one job, `pnpm check` + `pnpm lint` in
another. `just check` gained `web-check` so the local recipe and the workflow
enforce the same set — a gate in only one of them is a gate that will drift.

Playwright stays out of CI deliberately, and the workflow says why in a comment:
it needs a built bundle, a running API and a database, i.e. most of a
deployment. That is a real job to add later, not an oversight to rediscover.

## WI #545 — the docs sweep, and the tests that hold it

All eleven F-12 instances closed. The interesting ones:

**README's status block** claimed korg was "Milestone 1" and that kwi/kcard
"remain the live tools". Per decision D-8, korg *is* the system of record, and
has been for months — five skills and a production deploy depend on it. Fixed.

**Three tool-category lists** (README, usage.md, the MCP server `instructions`)
each omitted something different — proposals and reports were invisible in two
of them. There is now one catalogue in `docs/api.md`; README carries the count
and a link; usage.md carries a link. The server instructions still enumerate,
because they are the only thing an MCP client sees before calling anything — so
a test asserts they name every category.

**The REST table** was missing four implemented endpoints and had a wrong path
parameter (`:date` where the router says `:plan_date`). It is now generated
shape — `| Method(s) | Path | Description |` — because a machine has to parse it.

**The reports feature** — three tools, two endpoints — had no prose anywhere.
It now has a usage.md section covering the upsert-on-`(source, report_date)`
semantics, the finding edges, and why writes are MCP-only.

**`KORG_CORS_ORIGINS`** was undocumented. `KORG_WEB_DIR`'s documented default
was wrong (it has one: `/app/web/build`, the in-container path).

**The MCP protocol version** disagreed between README (`2025-06-18`) and
`scripts/mcp-roundtrip-check.sh` (`2025-03-26`). Both now say `2025-06-18`,
verified against the live server.

### The drift tests

`crates/korg-mcp/tests/docs_drift.rs`, seven tests:

- the api.md tool catalogue ≡ `tools()`, both directions
- catalogue categories are the known set, and all of them are used
- the MCP server instructions name every category
- README's stated tool count == `tools().len()`
- the usage.md REST table ≡ the routes registered in `korg-api`
- setup.md's env table ⊇ what `korg-api`/`korg-core` read
- migration.md's env tables ⊇ what `korg-migrate` reads

The route scanner parses `korg-api/src/lib.rs` for balanced `.route(…)` calls
rather than introspecting a built `Router` (axum does not expose one) — and
rather than a shared inventory constant, which would be one more thing to keep
in sync, i.e. the exact failure being fixed.

Each test was verified to actually **fail** on injected drift, with a message
naming the specific difference: a deleted endpoint row, a removed catalogue
entry, a wrong count, an undocumented env var. A drift test that has never been
seen red is a guess.

The MCP instructions moved out of `tools.rs` into `korg_mcp::server_instructions()`
so a test can read them. They also grew: the envelope contract, the archived
default, and name-or-id selectors are now stated up front, since an agent that
learns those from an error message has already made the mistake.

## WI #544 — the backup exists; now the repo knows

Everything here was **verified live**, not transcribed from the review:

| | |
|---|---|
| What | `pg_dump --clean --if-exists`, gzipped |
| Where | `/gratch/backups/korg/korg-<stamp>.sql.gz` on kubsdb — NAS-backed, survives kubsdb |
| When | `korg-backup.timer`, 03:17 + up to 10m jitter, `Persistent=true` |
| Retention | 14 days |
| Script | `/usr/local/bin/korg-backup.sh`, from k-homelab's `korg-backup` recipe |

15 dumps present, the newest from 03:23 this morning, monotonically growing.

One correction to the review, which matters at 3 a.m.: the dumps are **gzipped
plain SQL**, not custom-format archives. Restoring is `gunzip -c … | psql`.
Reaching for `pg_restore` — which the review's phrasing invites — fails with a
confusing error at the worst possible moment. `docs/operations.md` says so
explicitly.

`docs/operations.md` also covers what was undiscoverable from this repo: the
deployment's shape, logs, rollback (and its limit — rollback does **not** undo a
schema migration), the read-only query path into production, and the
rehearse-against-a-restored-dump recipe promoted from sprint 014's notes.

The read-only path is documented with the two facts that cost real time to
find: the Postgres container is `postgresql`, not `korg-pg`; and kubsdb's login
shell is fish, so anything non-trivial goes through `ssh kubsdb bash -s`. Both
were re-verified while writing this.

## WI #546 — deploys record what they shipped

Per D-9, `deploy-kubsdb` now **refuses a dirty working tree**. The old skill
advertised the opposite ("uncommitted changes ship — you do NOT need to commit
first"), which meant nothing anywhere recorded what was actually running.

Builds stamp `org.opencontainers.image.revision` with the commit SHA and add a
`korg:<short-sha>` tag, so `docker images korg` reads as a deploy history rather
than a list of `<none>`, and a running container can be asked which commit it
is.

Clean-tree-only is affordable because the thing dirty deploys were *for* has a
better answer, now documented in the skill: the vite dev loop on kai, hot
reloading against a local or production API. Refusing the shortcut without
providing the alternative would just get the refusal edited out.

The preflight also gained the backup-freshness check (WI #544's other half) and
a baseline capture. Its env row gained the required `KORG_TIMEZONE` — omitting
it crash-loops a *first* deploy on a new host, since a redeploy inherits env
from the previous container and hides the problem.

## WI #572 — a post-deploy check worth running

`scripts/post-deploy-check.sh`. `/api/health` proves a process is listening; a
container running last week's image passes it happily. This proves the shipped
code answers:

- **reads** — the enveloped collections, a focused read with inlined comments,
  and the error contract (a missing work item is a 404 carrying
  `code: "not_found"`)
- **MCP** — delegates to `mcp-roundtrip-check.sh` rather than duplicating it
- **write** — one idempotent write
- **counts** — every per-kind row count, diffed against a baseline captured
  before the deploy

Two decisions, both from the design notes on the WI:

**The write is idempotent** — a project's status re-PATCHed to the value it
already holds. A create/delete pair would prove more but adds rows to production
and needs cleanup that can itself fail. A *project* rather than a work item
because projects are not nodes: nothing but that row's own `updated` moves, so
no triage view reorders because someone verified a deploy.

**The count diff is reported, not asserted** — korg is live, and rows legitimately
appear while an image builds. A count going **down** exits non-zero, because rows
do not disappear on their own. This step has already paid for itself once: during
the sprint 015 deploy it explained a +1 that would otherwise have looked like the
new archived filter dropping data (the real cause was a work item created in the
UI mid-build).

Verified against **live production**: all reads, MCP roundtrip and the idempotent
write pass, and the counts it reports (378 work items, 27 cards, 4 links, 23
reports, 57 proposals) match a direct `psql` count exactly. The decrease path was
verified by feeding it a doctored baseline: it prints `LOST work_items` and exits
1.

## Verification

`just check` green end to end — fmt, generated-file freshness, `pnpm check`,
`pnpm lint`, clippy `-D warnings`, and the full test suite (34 suites).

The docs-drift and snapshot-gating behaviour was verified by deliberately
breaking things and confirming the failures, as described above, rather than by
observing green.

## Not done here

- The `.github/workflows/ci.yml` job has never run on GitHub — it runs first on
  push. Expect one round of adjustment.
- Playwright in CI (deliberate, see above).
- A restore drill. The last verified restore is 2026-07-08; `docs/operations.md`
  suggests a roughly quarterly cadence, but running one was out of scope.

## Deployed 2026-07-23

Shipped to kubsdb from merged `main` (`c78468fa`), the first deploy under the
clean-tree + SHA-stamp rules this sprint introduced.

| | |
|---|---|
| Image | `sha256:61bdfd51…`, tags `korg:latest` + `korg:c78468fa1912` |
| Revision label | `org.opencontainers.image.revision=c78468fa19120946da68b271af8984fe98cd38b1` |
| Rollback target | `sha256:6237bd19…` (no revision label — it predates the convention) |

The new preflight gates all fired and passed: clean tree, kubsdb reachable,
backups current (`korg-20260723-032356.sql.gz`, 283 KB, larger than each of its
predecessors), rollback target noted, baseline captured.

`scripts/post-deploy-check.sh --compare` passed with **zero delta** on every row
count (379 work items, 27 cards, 4 links, 0 topics, 57 proposals, 23 reports, 29
projects), plus health, the enveloped reads, the focused read, the 404 +
`code: not_found` contract, the MCP roundtrip, and the idempotent write. Its
first real use, and it did the job it was written for.

Verified live, specific to what this sprint changed:

- `docker inspect korg` now answers with the commit it was built from. The
  previous container returned an empty label, which is exactly the "predates the
  convention" answer WI #546 predicted.
- The rewritten MCP `instructions` are serving, and name sprint proposals,
  reports, comments and projects — all invisible to clients before this sprint,
  along with the envelope contract and the name-or-id selector rule.

CI note: `.github/workflows/ci.yml` ran green on the PR **and** on `main` after
the squash merge (both jobs). Its first run failed twice on the way there, which
is what a first run is for — `pnpm/action-setup` resolves `packageManager` from
the repo root while korg's lives in `web/`, and `on.push.branches: ["**"]` plus
`on.pull_request` double-ran every job. Both fixed in `e82787c` before the merge.

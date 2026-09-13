# 080 — the database password stops being korg's to keep

Proposal **korg:2557**, covering **WI 2547**. One leg of program **korg:2440**
("Simplify homelab secrets: name tags stop being locks, passwords get one copy
per host"), rank 19.6 — the last kubsdb consumer still holding its own copy of a
fleet password. Run as an overseen karc leg (`korg-5296c9`) on kai.

## Goal

`/datastore/korg/korg.env` held `DATABASE_URL` with korg's database password
embedded in it, while `/etc/khomelab/secrets.env` on the same host already
carried the same value as `KORG_DB_PASSWORD`. Two copies of one password on one
host is exactly what the program exists to end. After this, korg reads the
per-host file and keeps nothing.

## Premise check

Both of WI 2547's falsifiable claims verified live on kubsdb before any work,
key names only, no values printed:

- `/datastore/korg/korg.env` — holds `DATABASE_URL` (userinfo carries a
  password) and `KORG_TIMEZONE`. **Holds.**
- `/etc/khomelab/secrets.env` — `root:khomelab 0640`, carries
  `KORG_DB_PASSWORD` among six keys. **Holds.**

No cross-project plan applies. korg routes to `korg+/` in
`cross-project-planning`, but every decision there is about the agent-surface
read contract — lean rows, `omitted` envelopes, vocabulary growth across
deploys. Nothing in it names deployment or configuration, and this slice changes
no contract surface it covers.

## The decision this slice was left to make

The proposal offered two shapes and left the choice here: **(i)** keep compose
interpolation and have the deploy pass
`docker compose --env-file /etc/khomelab/secrets.env`, or **(ii)** have korg
build its connection from `KORG_DB_PASSWORD` itself.

**Taken: (ii).** Two reasons were already on the table — it is cleaner, and it
removes the coupling where *every* compose command in `/datastore/korg` (`down`,
`logs` and `ps` included, as k-homelab 062 found the hard way for unpoller) has
to remember a flag or compose aborts.

A third reason turned up in korg's own tree and is the one worth recording.
`deploy/cold-start.sh` **refused any password containing `@` or `/`**, because
it spliced the value into `postgres://korg:<here>@host/db` where either
character silently corrupts the authority section and fails as though the
password were wrong. That is a string format dictating what values the fleet's
rotation tooling is allowed to generate — and **korg:2439's proof pass rotates
exactly this password**. Shape (ii) does not build a URL at all: it sets the
password on parsed `PgConnectOptions`, so no character is special and the
refusal is deleted rather than carried forward.

## What shipped

**`crates/korg-core/src/db.rs`** (new) — the resolution rules and the one
connect+migrate path in the workspace. `korg_core::connect` is re-exported from
here so every existing caller is unchanged, and `korg-api` now goes through
`connect_options_from_env()` → `connect_with()`, which lands in the same place.

The rules:

| `DATABASE_URL` | `KORG_DB_PASSWORD` | result |
|---|---|---|
| carries a password | unset | used as it stands — local dev, the importer, every test |
| no password | set | the deployed shape: applied to the parsed options |
| carries a password | set | **startup error** |
| any | set but empty | **startup error** |

Two sources for one credential is refused rather than resolved by precedence.
The question program korg:2440 exists to make answerable is *"is this consumer
off its own copy?"*, and a fallback makes it unanswerable from config: a
deployment that kept a stale password in `DATABASE_URL` would go on working,
silently, on whichever copy won. This is the program's own ruling 1 for the
klams clients (identity-plus-token is an error, not a fallback) applied to a
password.

No error message can echo a credential — including the "not a valid URL" one,
which is precisely the moment an inline password would otherwise reach a log.
There is a test for that.

**`deploy/docker-compose.yml`** — takes `/etc/khomelab/secrets.env` whole (the
ruling on WI 2501, as every other compose consumer on this fleet does) alongside
`korg.env`, and carries the credential-free
`DATABASE_URL: postgres://korg@postgresql:5432/korg` under `environment:`. It
belongs in git now that it holds no secret, which is what this file has said
about `KORG_FRAME_ANCESTORS` since #1468. The comment says, at length, why it
must never become `${KORG_DB_PASSWORD}` interpolation.

**`deploy/korg.env.example`** — `KORG_TIMEZONE` and nothing else.

**`deploy/cold-start.sh`** — sets the role's password on the cluster as before
(the value still arrives on stdin and reaches no file), but writes only
`KORG_TIMEZONE`, and the `@`/`/` refusal is gone. Rendering the per-host secrets
file is explicitly *not* its job: two producers of one value is the thing being
removed.

**Docs** — `docs/setup.md`'s environment table (the `docs_drift` suite fails
without it), `docs/operations.md` deployment table and cold-start runbook, and
the `deploy-kubsdb` skill.

## Verification

`just check` green.

**Eight unit tests** on the pure rules (`db.rs`), including that neither refusal
quotes a credential back.

**Three integration tests** (`tests/sprint080.rs`) against a real Postgres, each
with a control that has to fail:

| asserted | control |
|---|---|
| a separated password authenticates | a wrong one is refused — and if it were *accepted*, the suite fails rather than becoming a tautology |
| the URL-embedded shape still connects | — |
| a password containing `@ / : # ?` authenticates | a near-miss password is refused |

The third is the old refusal, inverted into a test: the role is created with
that password for real and korg's own path authenticates as it.

**Planted defect.** Making the resolver silently ignore the password was caught
by both password-path tests, with `password authentication failed for user
"awkward"`. The gate is not a tautology.

**Live, on kubsdb, probed from kubsdb** — the host that does the work:

| measured | result |
|---|---|
| compose `env_file:` delivers `KORG_DB_PASSWORD` | fingerprint `59ba378d2eab`, length 32 |
| it authenticates as the **live `korg` role** over `kubsdb-net` | PASS |
| control: a wrong value | refused — so no `trust` rule is in play |
| `environment:` beats `env_file:` for `DATABASE_URL` | PASS |

Done on throwaway containers; the live korg container was not touched. Not
against the production database either — `korg-api` sweeps pending attachments
on startup, so a second instance pointed at it would have written.

### A probe of mine that was wrong

The first attempt at the credential probe used
`docker run --env-file /etc/khomelab/secrets.env` and reported **`password
authentication failed for user "korg"`** — which reads as "the rendered password
disagrees with the live role", the one finding that would have stopped this
slice and sent a defect back to k-homelab.

It was false. **`docker run --env-file` and compose's `env_file:` are different
parsers.** k-homelab renders `KEY='value'`; compose applies dotenv semantics and
strips the quotes, while the docker CLI's `--env-file` passes them through
literally. The two readings fingerprint differently — `34c9cde366ae` with the
quotes, `59ba378d2eab` without — and the container was authenticating with a
32-character password wrapped in two apostrophes.

The negative control did **not** catch this: a mangled password and a wrong
password both fail authentication identically. The control was necessary and not
sufficient, and what settled it was fingerprinting the same value through both
readers instead of believing the first failure. Worth carrying to korg:2439:
`--env-file` is not a stand-in for `env_file:` when probing this fleet's
per-host file.

## The cutover, for the ship turn

Not done here — a karc leg does not ship, and korg's deploy is `/sprint-ship`
Phase 7. Recreating the container drops the MCP channel every leg and both
overseer sessions use, so it happens once, alone, and the wrap-up is written
after korg is back.

Order, and it matters:

1. Deploy merged `main`: new image, new compose file, `up -d --force-recreate`.
   Compose's `environment:` overrides the stale `DATABASE_URL` still sitting in
   `korg.env`, which is why the precedence probe above was worth running — if
   `env_file:` had won, korg's new two-sources check would have refused to
   start.
2. Verify live: a real korg read **and** a write, plus a deliberately wrong
   `KORG_DB_PASSWORD` refused. "korg still answers" proves nothing — a running
   container keeps its create-time environment.
3. Rewrite `/datastore/korg/korg.env` to `KORG_TIMEZONE` only.
4. `up -d --force-recreate` once more, so korg is proven to start from the final
   on-disk state and not from a file that has since changed. Verify again.
5. `bin/secret verify kubsdb-korg-db-password` still `ok` with its control.

WI 2547 stays `open` until step 5. Its acceptance is explicitly the live proof,
and that is a ship-turn fact, not a time-gated one — no soak work item here.

## Repaired in passing

**`deploy/cold-start.sh`'s verification had no control.** It authenticates as
the `korg` role over the docker network specifically to avoid the `trust` rule
that makes a loopback check accept any password — and a long comment explains
that trap. The explanation was the only thing guarding it: if pg_hba ever
changed, the check would report success for the wrong reason and the script
would say "verified". It now also tries a deliberately wrong password and exits
non-zero if that is accepted, saying plainly that nothing was proven. Mechanical,
inside this repo, and the same discipline the rest of this sprint is held to.

Not executed against a live cold start — the script only runs on a rebuilt host.
Its logic is the same two-probe shape measured live above, and a mistake in it
fails loudly during DR rather than silently.

## Deployed

**2026-09-13, kubsdb**, revision `eb0d91b3c5fc` (squash `eb0d91b`, PR #85),
pulled from the homelab registry. Rollback target `37b03d6e2eb4`, confirmed
present in the registry before building.

The cutover ran in the order the sprint record set out and the overseer's
clearance accepted, from a fresh login on kubsdb each time (compose reads
`/etc/khomelab/secrets.env` as the invoking user; the login carried the
`khomelab` group).

1. New image + new compose file, `up -d --force-recreate`. Revision assertion
   passed.
2. Verified.
3. `/datastore/korg/korg.env` rewritten to `KORG_TIMEZONE` only — 34 bytes,
   mode 0600, written through kaed's typed dotenv edit so the old value was
   never read into a transcript.
4. `up -d --force-recreate` again, so korg is proven to start from the **final**
   on-disk state and not from a file that has since changed. Revision assertion
   passed.
5. Verified again.

### What was verified live, and the control for each

Every probe run from kubsdb, the host that does the work.

| check | result | control |
|---|---|---|
| container's `DATABASE_URL` | carries no credential | — |
| container's `KORG_DB_PASSWORD` | `59ba378d2eab`, identical to the per-host file's | — |
| reads + writes, both transports | `post-deploy-check.sh --compare` **OK** | — |
| row counts vs the pre-deploy baseline | cards 30, links 20, projects 59, proposals 456, reports 74, work items 1544 — **all unchanged**; migrations 34, node_count 2483 | — |
| korg genuinely authenticates with that variable | — | **the same image with a deliberately wrong `KORG_DB_PASSWORD` refuses to start**: `password authentication failed for user "korg"` |
| `bin/secret verify kubsdb-korg-db-password` | `ok` | its own negative control refused |

The wrong-password control is the load-bearing one. "korg still answers" proves
nothing — a running container keeps its create-time environment, so only a
process that fails without the variable shows the variable is doing the work.

### The consequence of the whole-file ruling, now measurable

The container's environment carries all six of kubsdb's keys —
`GRAFANA_ADMIN_PASSWORD`, `POSTGRES_PASSWORD`, `REDISCLI_AUTH` and both
`UNIFI_CONTROLLER_*` beside `KORG_DB_PASSWORD`. That follows the WI 2501 ruling
and the overseer upheld it for korg (review korg:2585, ruling 3), recording the
narrowing option for Ken: korg's deploy passing
`--env-file /etc/khomelab/secrets.env` with a single interpolated
`KORG_DB_PASSWORD`, at the cost of every compose command in `/datastore/korg`
needing the flag.

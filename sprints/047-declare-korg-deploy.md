# 047 — Declare korg's deploy: committed compose + a cold-start path

**Proposal:** korg:1006 · **Work items:** #842 (task, S)
**Branch:** `047-declare-korg-deploy`
**Program:** korg:1008 "korg is redeployable from a cold host" — slice 1 of 2.
Slice 2 is k-homelab #1005 (proposal k-homelab:1007), which fixes the
cluster-level half in `dr/kubsdb.md` and links back to the Cold start section
this sprint writes. That ordering is why korg went first.

## The premise correction that reshaped this

#842 came out of k-homelab sprint 016 with the headline *"korg's production DB
credential exists only in the running container — `docker rm korg` would destroy
it"*. Checked against the live host before starting: **it was wrong.**
`/datastore/korg/korg.env` (mode 0600) and a `/datastore/korg/docker-compose.yml`
had both existed since 2026-07-18, and the container env, that file and the age
store all carried the same value (password fingerprint `59ba378d2eab`).

The reasoning that produced the wrong headline is the part worth keeping:
`com.docker.compose.project*` labels were empty, which was read as "no compose
file". Empty labels prove the container was not *started* by compose. They say
nothing about what is on disk. The audit was one `ls` away from the truth.

So this stopped being a credential emergency and became what it actually was: an
undeclared deploy. Downgraded, not dismissed.

## This was known once, and lost

The compose file was not news to this repo. `sprints/004-sprint-proposal/DEPLOY.md`
records finding it the hard way: a deploy that reconstructed `docker run` from
`docker inspect` produced a container with an **empty** environment which
crash-looped on `Error: DATABASE_URL is required`, and recovery was
`find / -iname "*korg*"` on kubsdb turning up `/datastore/korg/docker-compose.yml`
and `korg.env` — described there, exactly, as *"the actual source of truth,
checked into neither git nor memory but sitting right there on the host"*. That
record ends with an instruction:

> check for a compose file on the target host *before* trying to hand-reconstruct
> a `docker run` command.

Sprints 004, 005 and 006 duly deployed with `cd /datastore/korg && docker compose
up -d`.

Then sprint **009** created the `deploy-kubsdb` skill, and its very first commit
(`d60d674`) encodes `docker inspect` + `docker run` — the pre-004 approach. The
lesson had been written down in a sprint record, which is not a place the next
tool looks. Every deploy since has used `docker run`; that is why the container
carries no compose labels; that is what sprint 016's audit saw and misread as
"no compose file exists".

So the same fact was learned in 004, discarded in 009, and mis-rediscovered in
016. That chain is the argument for this sprint's shape. Committing the compose
file and pointing the skill at it is not merely tidier than prose — it is the
only version of the fix a future skill author cannot silently undo, because the
run config becomes a file the deploy *reads* instead of a fact someone has to
remember to honour.

## Goal

Make "how does korg run, and how do I bring it up on a host that has never run
it" answerable from this repo, in a diff, instead of from a prose table in a
skill file plus an operator's memory.

## Shipped

- **`deploy/docker-compose.yml`** — the single declaration of the run config:
  network, both port publications, restart policy, `env_file: korg.env`
  (resolved relative to the file, so it is `/datastore/korg/korg.env` once
  deployed). `image: ${KORG_IMAGE:-korg:latest}` keeps rollback to one command.
  `KORG_WEB_DIR` / `KORG_LISTEN_ADDR` are deliberately **absent** — the
  Dockerfile's `ENV` owns them, and restating them here would create the second
  copy this sprint exists to remove.
- **`deploy/cold-start.sh`** — executable, idempotent. Creates the `korg` role,
  creates the database, writes `korg.env` at 0600, and verifies by logging in.
  Takes the password **on stdin only**: not argv (`ps`), not the environment
  (`/proc`, `docker inspect`), not a temp file, not shell history. Prints two
  truncated sha256 fingerprints and never the value. Every knob
  (`KORG_ROLE`, `KORG_DB`, `KORG_ENV_PATH`, `KORG_PG_CONTAINER`) is overridable,
  which is what made it testable against scratch objects.
- **`deploy/korg.env.example`** + a `.gitignore` entry for `deploy/korg.env`, so
  the real file cannot be committed by accident.
- **`deploy-kubsdb` skill** — the prose run-config table is gone, replaced by a
  pointer to the compose file. Step 3 is now `docker compose up -d` with a
  **self-limiting** one-time cutover: it removes the pre-#842 container only
  while that container lacks compose labels, so the branch never runs again
  after the first compose deploy. Rollback became `KORG_IMAGE=korg:<sha> docker
  compose up -d`, deliberately non-sticky.
- **`docs/operations.md`** — two new rows in the deployment table (run config,
  credential; the document had never mentioned the credential at all) and a
  **Cold start** section.
- **On kubsdb:** removed `docker-compose.yml.bak-20260717`. It published
  `0.0.0.0:5674` — precisely the binding that collides with tailscaled's
  listener and leaves the container created with no network attached,
  crash-looping on name resolution for `postgresql`. A wrong copy sitting beside
  the right one in the directory an operator reaches for under pressure. Its
  only difference from the good file was that binding, which the committed
  compose file now documents in a comment.

## Two things the verification found

**1. The obvious credential check verifies nothing.** `cold-start.sh` first
confirmed the role by connecting to `127.0.0.1` from inside the postgres
container. kubsdb's `pg_hba.conf` opens with:

```
local  all all                trust
host   all all 127.0.0.1/32   trust
host   all all all            scram-sha-256
```

so that check succeeds against **any** password — confirmed empirically with
`PGPASSWORD=totally-wrong`, which was accepted over loopback and rejected via the
container's network name. A verification that always passes is worse than none,
because it reports success. The script now connects through `$PG_CONTAINER`,
which is a non-loopback source address and therefore the same `scram-sha-256`
rule korg's own container authenticates under.

**2. `docker exec -i` eats a `bash -s` heredoc.** A test script piped to
`ssh kubsdb bash -s <<'EOF'` silently stopped executing after its first
`docker exec -i` — that command inherits stdin, which *is* the script. It looked
like a passing test with missing output. Inside `cold-start.sh` this is harmless
(the SQL heredoc redirects stdin for that command specifically), but it is a
good way to lose half a test and not notice.

## Verified

Against scratch objects on kubsdb's live cluster (`korg_cs_test` role +
database, `/tmp/cs-test/korg.env`), all dropped afterwards — `pg_roles` confirmed
back to just `korg`:

| | |
|---|---|
| fresh run | role created, database created, env written 0600, login verified |
| idempotent | second run with the same password — identical file sha256 |
| rotation | third run with a different password — `ALTER ROLE` applied, login verified |
| input guard | password containing `@` rejected with exit 2 (it would corrupt the URL's authority section) |
| negative | wrong password rejected over the scram path |
| compose | `docker compose config --quiet` passes on kubsdb (compose 5.1.3); resolved project `korg`, `env_file: korg.env` resolves relative to the deployed directory as intended |

**Not rehearsed:** a true cold start against an empty cluster. That needs a
scratch Postgres instance, not kubsdb's live one. The role/database/env/login
path is exercised above; the *sequence* on a genuinely fresh host is reasoned,
not tested. Said plainly here rather than implied away.

`just check` was not run — this sprint touches no Rust, no TypeScript and no
generated artefact. `bash -n` passes on `cold-start.sh`; shellcheck is not
installed on kai.

## Decisions

- **Compose, not a `docker run` script.** Three things fall out: the run config
  becomes reviewable in a diff; the compose labels come back, so the next
  auditor's inference is sound; and `up -d` is idempotent where `rm -f` + `run`
  has a window with no container.
- **The age store stays a backstop, and is not on the deploy path.** It is read
  by exactly one procedure — cold start. Rendering the secret at deploy time was
  considered and rejected: the k-homelab checkout exists only on kubs0 while
  deploys run from kai, so it would put a second host and an ssh key on the
  critical path of *every* deploy to solve a problem that occurs once per
  rebuild.
- **`korg.env` becomes the source of truth for the container's environment**,
  replacing the old `docker inspect` carry-forward. Same security property (the
  value never leaves kubsdb), and it drops the hidden precondition that a
  previous container must exist — which was the actual defect behind "there is
  no cold start".
- **Refuse, don't encode, a password containing `@` or `/`.** Percent-encoding
  in the script would make the stored value and the deployed value differ, which
  is the drift this whole exercise is about.

## Follow-on, filed not implied

`kubsdb-korg-db-password` now has three deployed locations (container env,
`korg.env`, age store) and `last_rotated: unknown` in `secrets/index.yml`. It
belongs in **krot**, whose whole job is knowing every copy of a rotatable secret.
Not done here.

## Deployed

Deployed to `kubsdb` 2026-08-06 (post-merge, from `main` @ `1eacecd`) — the
**first compose-driven deploy of korg**, and the one that closes the loop this
sprint is about.

| | |
|---|---|
| Image | `korg:1eacecd10af7` / `korg:latest`, revision label `1eacecd10af76e99ded9329f2bd427025783ed02` |
| Rollback target | `korg:2d3c401ce6b2` (sprint 046), retained on kubsdb |
| Migration | none — deploy/docs only; `migrations: 23` before and after |

The one-time cutover guard **fired exactly once**, as designed: it saw a
container with no `com.docker.compose.project` label, printed `removing
pre-compose container (one-time)`, removed it, and `docker compose up -d`
created the replacement. On any subsequent deploy the label is present and the
branch is skipped.

Verified live:

- `com.docker.compose.project=korg`, `service=korg` — **the labels are back.**
  Their absence is what sprint 016 misread, and what made this sprint's audit
  trail wrong in the first place.
- Revision label on the running container matches the commit built, exactly.
- Deployed `/datastore/korg/docker-compose.yml` is **byte-identical** to the
  committed `deploy/docker-compose.yml` (sha256 `366c25f2…` both sides) — the
  proposal's stated success criterion.
- `korg.env` untouched; the container's environment is unchanged from before the
  cutover, including `KORG_WEB_DIR` and `KORG_LISTEN_ADDR` arriving from the
  image rather than the env file.
- `post-deploy-check.sh --compare`: `== OK`. Every count flat (935 nodes, 708
  work items, 149 proposals, 40 projects, 30 cards, 27 reports, 4 links, 1
  topic), schema and sequence state identical.
- `/`, `/plan`, `/work-items` all 200 over the tailnet HTTPS URL; container
  `healthy`; MCP exercised by a real `get_work_item` call against the new
  container.

One thing worth not misreading: a bare `curl -X POST /mcp` returns *"Not
Acceptable: Client must accept both application/json and text/event-stream"*.
That is the MCP spec being enforced, not a broken endpoint — the transport was
confirmed with an actual MCP client call instead.

### A third instance of the same stdin trap

The deploy hit the `docker exec -i` lesson again in a new costume: a nested
`ssh -n`-less `ssh` inside an outer `ssh kai bash -s <<'EOF'` swallowed the rest
of the preflight script, silently skipping the baseline capture. `ssh` reads
stdin exactly like `docker exec -i` does. The general rule, now seen three times
in one sprint: **any command that inherits stdin will eat the heredoc that is
feeding the shell it runs in** — pass `-n` to nested `ssh`, redirect
`</dev/null`, or give the inner command its own explicit input.

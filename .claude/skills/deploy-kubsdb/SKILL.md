---
name: deploy-kubsdb
description: Build the korg image from a clean working tree and deploy it to the kubsdb production host (web UI + REST + MCP on :5674). Use when asked to deploy/redeploy/ship korg to kubsdb. For iterating on UI changes, use the kai dev loop instead — this deploys committed code only.
---

# Deploy korg to kubsdb

Builds `korg:latest` locally from **committed code**, stamps it with the commit
it was built from, and deploys it to `kubsdb` over SSH. There is no image
registry.

## Deploys are clean-tree only

**Refuse to deploy from a dirty working tree.** The Dockerfile compiles the
SvelteKit bundle and the Rust release binary from source, so whatever is in the
tree ships — and if that includes uncommitted work, nothing on earth records
what is actually running. `docker inspect` would say `korg:latest`, git would
say something else, and the difference is invisible until it bites.

So (decision D-9): production only ever receives builds of committed code, and
every image carries its commit SHA as a label.

If you want to see a UI change in a browser without committing it, that is what
the **dev loop on kai** is for — see the last section. Do not reach for a dirty
deploy; it is the thing this skill exists to prevent.

## Target

`kubsdb` (192.168.1.60) runs one `korg` Docker container serving web + REST +
MCP on `:5674`. User-facing access is over Tailscale at
`https://kubsdb.encke-wahoo.ts.net:5674/` (HTTPS — a secure context, so the
browser clipboard API works there); deploy verification below uses the tailnet
HTTPS `https://kubsdb.encke-wahoo.ts.net:5674` from the build host.

**The run config is not in this file.** It lives in
[`deploy/docker-compose.yml`](../../../deploy/docker-compose.yml) — network,
both port publications, restart policy, the env file — and that is the only
place to change it (WI #842). This skill copies that file to
`/datastore/korg/docker-compose.yml` and brings the container up from it. It
deliberately does not restate what the file already says: a second copy of a run
config is a second thing to drift, and the prose table that used to live here
was exactly that.

Two things the compose file cannot carry:

- **`/datastore/korg/korg.env`** (mode 0600) holds `DATABASE_URL` and
  `KORG_TIMEZONE`. It is not in this repo and never will be, and a redeploy does
  not touch it — which is how the password stays on kubsdb without anyone
  hand-copying it. If it is *missing* (a rebuilt host), you are in the **cold
  start** case, which this skill does not cover: see
  [docs/operations.md](../../../docs/operations.md#cold-start) and
  `deploy/cold-start.sh`.
- **`KORG_TIMEZONE` is required** — `korg-core`'s config rejects a missing or
  invalid value at startup rather than guessing a zone, so a first deploy onto a
  host with no `korg.env` crash-loops immediately.

**Do not publish on `0.0.0.0:5674`.** `tailscale serve` terminates TLS on
kubsdb's tailnet address (`100.90.99.84:5674` + the IPv6 tailnet address) and
proxies to `http://localhost:5674`. A `0.0.0.0` bind overlaps that listener, so
`docker run` fails with *failed to bind host port 0.0.0.0:5674/tcp: address
already in use* whenever tailscaled got there first — and the failed run leaves
the container created with **no network attached**, which then crash-loops on
`Temporary failure in name resolution` for `postgresql`. Publishing loopback
(what `tailscale serve` proxies to) plus the LAN address serves both consumers
with no ordering dependency. If you hit the wedged state, `docker rm -f korg`
and re-run step 3 — `docker start` cannot repair the missing network.

`kubsdb`'s login shell is **fish** — pipe multi-line remote scripts through
`ssh kubsdb bash -s` (a bare `ssh kubsdb '…'` runs under fish and mis-parses
`$()` / `{{…}}`).

## Preflight

1. **Clean tree — stop here if it is not.**
   ```bash
   git status --porcelain
   ```
   Any output at all means **do not deploy**. Report what is dirty and ask
   whether to commit it or to use the kai dev loop instead. Never stash, never
   `--no-verify`, never "just this once": an unrecorded production build is the
   exact failure this check exists for.

2. `ssh -o ConnectTimeout=5 kubsdb hostname` — confirm reachability.

3. **Backups current?** A deploy applies pending schema migrations
   automatically, and last night's dump is the only thing that makes a bad one
   survivable.
   ```bash
   ssh kubsdb 'ls -la /gratch/backups/korg/ | tail -3; systemctl list-timers korg-backup.timer'
   ```
   The newest dump should be from last night and larger than the one before it.
   If it is stale or shrinking, say so and ask before continuing — see
   [docs/operations.md](../../../docs/operations.md#backups).

4. From the repo root, confirm `just check` is green, or that the caller has
   already verified. A broken build wastes a multi-minute image build.

5. Note the currently-deployed image so a rollback target exists:
   ```bash
   ssh kubsdb 'docker inspect korg --format "{{.Image}} {{index .Config.Labels \"org.opencontainers.image.revision\"}}"'
   ```

6. **Capture the baseline** the post-deploy check will diff against:
   ```bash
   bash scripts/post-deploy-check.sh --baseline /tmp/korg-baseline.json
   ```

## Procedure

1. **Build** (from repo root; the Rust release stage takes a few minutes — run
   it in the background and poll the log). The label is what makes a running
   container answerable about which commit it is:
   ```bash
   REV=$(git rev-parse HEAD)
   docker build -t korg:latest -t "korg:${REV:0:12}" \
     --label "org.opencontainers.image.revision=$REV" .
   ```
   The second tag means the previous image keeps a meaningful name after
   `korg:latest` moves, so `docker images korg` reads as a deploy history
   instead of a list of `<none>`.
2. **Ship** over SSH (no registry — save the image and load it remotely). Send
   **both** refs: `docker save` accepts several and the archive is
   content-addressed, so the SHA tag costs nothing extra to transfer.
   ```bash
   docker save korg:latest "korg:${REV:0:12}" | ssh kubsdb 'docker load'
   ```
   Shipping only `korg:latest` (what this step used to do) left the SHA tag on
   the build host alone, so the previous image on kubsdb became `<none>` the
   moment `latest` moved — quietly removing the named rollback target the
   Rollback section below depends on (WI #584).
3. **Deploy** — ship the run config, then bring the container up from it:
   ```bash
   ssh kubsdb 'mkdir -p /datastore/korg'
   scp deploy/docker-compose.yml kubsdb:/datastore/korg/docker-compose.yml
   ssh kubsdb bash -s <<'EOF'
     cd /datastore/korg
     # One-time cutover: a container created before WI #842 came from `docker
     # run` and carries no com.docker.compose.* labels, so compose will not
     # adopt it and fails with "container name /korg is already in use". Remove
     # it once. The guard is self-limiting — after the first compose deploy the
     # label is present and this branch never runs again.
     if [ -n "$(docker ps -aq -f name='^korg$')" ] && \
        [ -z "$(docker inspect korg --format '{{index .Config.Labels "com.docker.compose.project"}}')" ]; then
       echo "removing pre-compose container (one-time)"
       docker rm -f korg
     fi
     docker compose up -d
   EOF
   ```
   `korg.env` is untouched by all of this — compose reads it in place. `docker
   compose up -d` recreates the container because `docker load` moved
   `korg:latest` to a new image id; if it reports `up-to-date`, step 2 did not
   land and you are about to verify last week's build.
4. **Verify.** Run the fixed check first — reads on both transports, the error
   contract, an idempotent write, and a diff of every row count against the
   baseline from preflight:
   ```bash
   bash scripts/post-deploy-check.sh --compare /tmp/korg-baseline.json
   ```
   It exits non-zero if any count went **down**. Do not wave that away: rows do
   not disappear on their own. A count going *up* is normal — humans and agents
   add rows while an image builds — but it should be explainable.

   Confirm the running container is the commit you built:
   ```bash
   ssh kubsdb 'docker inspect korg --format "{{index .Config.Labels \"org.opencontainers.image.revision\"}}"'
   ```

   Then smoke-test **whatever this deploy was actually for** — that part no
   fixed script can do for you. A deep link returns 200
   (`curl -s -o /dev/null -w '%{http_code}' https://kubsdb.encke-wahoo.ts.net:5674/plan`),
   the new endpoint answers, the changed UI renders. `/api/health` alone proves
   only that a process is listening; a container running last week's image
   passes it happily.

## Rollback

Old images stay in kubsdb's local store — `docker images korg` lists them, and
since step 2 now ships the `korg:<short-sha>` tag alongside `latest`, that list
is readable *on the host that needs it*. To revert, override the image the
compose file defaults to (from preflight step 5):

```bash
ssh kubsdb bash -s <<'EOF'
  cd /datastore/korg
  KORG_IMAGE=korg:<short-sha> docker compose up -d
EOF
```

That override is not sticky — the next deploy's plain `docker compose up -d`
goes back to `korg:latest`, which is what you want, since a rollback is meant to
be undone by shipping a fix rather than by remembering to unset something.

Before WI #584, only `latest` was shipped, so each previous image went `<none>`
the moment a new deploy moved the tag — and several sprint notes recorded a
`korg:<sha>` rollback target that did not exist on kubsdb. The images themselves
survived as dangling layers, so the names were recoverable from the label each
one carries; sprint 027 restored them with:

```bash
ssh kubsdb bash -s <<'EOF'
for id in $(docker images -aq --filter "dangling=true") $(docker images -q korg); do
  rev=$(docker inspect "$id" --format '{{index .Config.Labels "org.opencontainers.image.revision"}}' 2>/dev/null)
  case "$rev" in ""|"<no value>") continue ;; esac
  docker image inspect "korg:${rev:0:12}" >/dev/null 2>&1 || docker tag "$id" "korg:${rev:0:12}"
done
EOF
```

Worth knowing if an old target is ever missing again — the revision label makes
an untagged image self-identifying.

Rollback is image-only. It does **not** undo a schema migration — korg applies
those automatically at startup. Going back across a migration boundary needs a
restore from a dump, not a re-tag; see
[docs/operations.md](../../../docs/operations.md#restore).

## Iterating on the UI without deploying

Do not deploy to see a change. Run the dev loop on kai:

```bash
# Terminal 1 — API against a local database
DATABASE_URL=... KORG_TIMEZONE=Etc/UTC cargo run -p korg-api

# Terminal 2 — vite dev server, hot reload
cd web && KORG_API=http://localhost:8090 pnpm dev    # http://localhost:5173
```

`KORG_API` can point at the production API instead of a local one when you need
real data behind a UI change — set `KORG_CORS_ORIGINS` on the target so the
cross-origin request is allowed. Remember that a dev server pointed at
production issues **real writes**.

This is the whole reason clean-tree-only is affordable: the thing dirty deploys
were used for has a better answer.

## After a successful deploy

Record it: append a short "Deployed <date>" note to the sprint's
`sprints/<branch>/README.md` — the image SHA and what was verified live —
mirroring the sprint-001 `DEPLOY.md` result section. This keeps the deploy
history with the work that shipped.

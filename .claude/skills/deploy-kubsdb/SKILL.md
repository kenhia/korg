---
name: deploy-kubsdb
description: Build the korg image from a clean working tree and deploy it to the kubsdb production host (web UI + REST + MCP on :5674). Use when asked to deploy/redeploy/ship korg to kubsdb. For iterating on UI changes, use the kai dev loop instead — this deploys committed code only.
---

# Deploy korg to kubsdb

Builds the korg image locally from **committed code**, stamps it with the commit
it was built from, pushes it to the homelab docker registry, and brings kubsdb
up on it.

## Deploys are clean-tree only

**Refuse to deploy from a dirty working tree.** The Dockerfile compiles the
SvelteKit bundle and the Rust release binary from source, so whatever is in the
tree ships — and if that includes uncommitted work, nothing on earth records
what is actually running. `docker inspect` would name a `latest` tag, git would
say something else, and the difference is invisible until it bites — and now
that images are published, a dirty build does not just mislead this host, it
puts an unattributable artefact in the registry's permanent history.

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

## The registry

Images travel through the homelab docker registry —
`kubsdb.encke-wahoo.ts.net:5000`, a `registry:2` container whose blobs live in
the backed-up `/datastore/packages` tree (k-homelab sprint 020, korg #1011).
kai pushes, kubsdb pulls. This replaced `docker save | ssh kubsdb docker load`,
which worked but left no history anywhere except kubsdb's local image store.

- **TLS comes from `tailscale serve`**, so there is nothing to configure: no
  `insecure-registries` entry, no certificate to distribute, no login. If a
  push ever fails on TLS, the answer is tailscaled on kubsdb, not a daemon
  flag.
- **Use the tailnet name on both ends.** kubsdb pulls from
  `kubsdb.encke-wahoo.ts.net:5000` — itself, over the tailnet — rather than
  `localhost:5000`, because the image reference is baked into the compose file
  and has to name the same thing on the host that pushes and the host that
  pulls. The cost is that a deploy needs tailscaled up on kubsdb even though
  the registry is running locally on it.
- **Always push both tags** — `korg:<short-sha>` *and* `korg:latest`. `latest`
  alone would recreate the no-history status quo the registry exists to fix;
  the SHA tag is what [Rollback](#rollback) reverts to, and unlike the old
  local-image-store history it survives kubsdb pruning its images and is
  mirrored to the NAS nightly with the rest of `/datastore/packages`.

The tag is the 12-character git short SHA, matching the
`org.opencontainers.image.revision` label. korg's crate versions are a flat
`0.1.0` and are not maintained per release, so a semver tag would collide on
every build and name nothing — the commit is korg's real version.

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

   Confirm the registry answers too, since a deploy now goes through it and a
   multi-minute build is a bad time to find out it does not:
   ```bash
   curl -s -o /dev/null -w '%{http_code}\n' --max-time 10 \
     https://kubsdb.encke-wahoo.ts.net:5000/v2/
   ```
   `200` is the healthy answer. Anything else is the registry container or
   tailscaled on kubsdb — check `ssh kubsdb 'docker ps --filter name=registry'`
   before building.

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
   Confirm that revision is actually in the registry — that is what Rollback
   pulls, and a target you never pushed is not a target:
   ```bash
   curl -s --max-time 10 https://kubsdb.encke-wahoo.ts.net:5000/v2/korg/tags/list
   ```

6. **Capture the baseline** the post-deploy check will diff against:
   ```bash
   bash scripts/post-deploy-check.sh --baseline /tmp/korg-baseline.json
   ```

## Procedure

1. **Build** (from repo root; the Rust release stage takes a few minutes — run
   it in the background and poll the log). Tag with the registry-qualified name
   directly, so there is no separate `docker tag` step to forget. The label is
   what makes a running container answerable about which commit it is:
   ```bash
   REG=kubsdb.encke-wahoo.ts.net:5000
   REV=$(git rev-parse HEAD)
   docker build -t "$REG/korg:latest" -t "$REG/korg:${REV:0:12}" \
     --label "org.opencontainers.image.revision=$REV" .
   ```
   The SHA tag is the one that lasts: after `latest` moves, it is the only name
   the previous build still answers to.
2. **Push** both refs. They share every layer, so the SHA tag costs one small
   manifest on top of `latest`:
   ```bash
   docker push "$REG/korg:${REV:0:12}"
   docker push "$REG/korg:latest"
   ```
   Push the SHA **first**. If the second push fails, the registry holds a
   complete, named build and `latest` still points at the last good one — the
   safe half-state. The reverse order leaves `latest` pointing at a build with
   no durable name.

   This replaced `docker save … | ssh kubsdb 'docker load'`. That path had to
   ship both refs by hand for the same reason (WI #584: shipping only `latest`
   left the previous image on kubsdb as `<none>`, quietly deleting the rollback
   target). The registry makes the history durable rather than a property of
   one host's image store.
3. **Deploy** — ship the run config, pull, then bring the container up:
   ```bash
   ssh kubsdb 'mkdir -p /datastore/korg'
   scp deploy/docker-compose.yml kubsdb:/datastore/korg/docker-compose.yml
   ssh kubsdb bash -s "$REV" <<'EOF'
     set -euo pipefail
     expected="$1"
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
     docker compose pull
     docker compose up -d

     # The gate that makes the pull trustworthy — see below.
     actual=$(docker inspect korg \
       --format '{{index .Config.Labels "org.opencontainers.image.revision"}}')
     if [ "$actual" != "$expected" ]; then
       echo "REVISION MISMATCH: running $actual, built $expected" >&2
       exit 1
     fi
     echo "running revision $actual"
   EOF
   ```
   `korg.env` is untouched by all of this — compose reads it in place.

   **`docker compose pull` is required, and it does not report whether it did
   anything.** Compose will not re-fetch an image it already has cached under
   that name, so omitting the pull would leave `up -d` re-running whatever
   `latest` kubsdb fetched last time — and unlike the old `docker save`
   transfer, there is no visible shipping step whose absence you would notice.
   But `compose pull` prints `Pulling` / `Pulled` identically whether it
   downloaded a new image or matched a cached one, so **its output is not
   evidence**: do not read success into it.

   That is why the revision assertion above is part of the deploy rather than
   an afterthought in Verify. It compares the running container's commit label
   against the commit this run built, and fails the deploy if they differ —
   which is the one check that catches a pull that silently did nothing, a
   `latest` that never moved, and a compose `up` that adopted an existing
   container, without depending on any command's phrasing. `set -euo pipefail`
   is doing real work here too: a failed `compose pull` must not fall through
   to `up -d`.
4. **Verify.** Run the fixed check first — reads on both transports, the error
   contract, an idempotent write, and a diff of every row count against the
   baseline from preflight:
   ```bash
   bash scripts/post-deploy-check.sh --compare /tmp/korg-baseline.json
   ```
   It exits non-zero if any count went **down**. Do not wave that away: rows do
   not disappear on their own. A count going *up* is normal — humans and agents
   add rows while an image builds — but it should be explainable.

   Step 3 already asserted the running container is the commit you built, so
   this is where that fact gets recorded rather than re-checked. If you want it
   by hand anyway — reviewing a deploy someone else ran, say:
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

**The registry is the rollback history.** Every build korg has deployed since
the cutover is a named tag there, whether or not kubsdb still has it cached:

```bash
curl -s https://kubsdb.encke-wahoo.ts.net:5000/v2/korg/tags/list
```

To revert, override the image the compose file defaults to, using a tag from
that list (or the revision noted in preflight step 5):

```bash
ssh kubsdb bash -s <<'EOF'
  cd /datastore/korg
  export KORG_IMAGE=kubsdb.encke-wahoo.ts.net:5000/korg:<short-sha>
  docker compose pull
  docker compose up -d
EOF
```

`compose pull` matters here too: the point of rolling back to a registry tag is
that it works even when the old image is *not* in kubsdb's local store, which
is exactly the case pruning or a host rebuild leaves you in.

That override is not sticky — the next deploy's plain `docker compose up -d`
goes back to `latest`, which is what you want, since a rollback is meant to be
undone by shipping a fix rather than by remembering to unset something.

This used to depend on kubsdb's local image store, which made it fragile in a
way worth remembering: before WI #584 only `latest` was shipped, so each
previous image went `<none>` the moment a deploy moved the tag, and several
sprint notes recorded a `korg:<sha>` rollback target that did not exist on the
host. Sprint 027 recovered those names from each image's revision label. The
registry removes the failure mode rather than mitigating it — the tags live off
the host, and `/datastore/packages` is mirrored to the NAS nightly.

Pre-cutover images remain on kubsdb under their unqualified `korg:<short-sha>`
names and still work as `KORG_IMAGE` values. They are not in the registry —
only builds pushed since the cutover are — so for anything older than sprint
048 check `ssh kubsdb 'docker images korg'` rather than the tags list.

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

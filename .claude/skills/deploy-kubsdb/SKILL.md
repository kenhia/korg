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
HTTPS `https://kubsdb.encke-wahoo.ts.net:5674` from the build host. Preserve this run config on
redeploy:

| Setting | Value |
|---------|-------|
| network | `kubsdb-net` |
| ports   | `127.0.0.1:5674 -> 5674` **and** `192.168.1.60:5674 -> 5674` (see the warning below — do **not** use `0.0.0.0:5674`) |
| restart | `unless-stopped` |
| env     | `DATABASE_URL=postgres://korg:…@postgresql:5432/korg`, `KORG_TIMEZONE=America/Los_Angeles`, `KORG_WEB_DIR=/app/web/build`, `KORG_LISTEN_ADDR=0.0.0.0:5674` |
| mounts  | none |

`KORG_TIMEZONE` is **required** — `korg-core`'s config rejects a missing or
invalid value at startup, so a *first* deploy onto a host without it crash-loops
immediately. Step 3 below carries the existing env forward, so a redeploy
inherits it; it is listed here for the case where there is no previous container
to inherit from.

**Never hand-copy the DB password.** Step 3 reuses the env from the running
container via `docker inspect`, so the secret never leaves kubsdb.

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
2. **Ship** over SSH (no registry — save the image and load it remotely):
   ```bash
   docker save korg:latest | ssh kubsdb 'docker load'
   ```
3. **Recreate** the container, reusing the existing env (piped through bash):
   ```bash
   ssh kubsdb bash -s <<'EOF'
     ENV=$(docker inspect korg --format '{{range .Config.Env}}-e {{.}} {{end}}')
     docker rm -f korg
     docker run -d --name korg --network kubsdb-net \
       -p 127.0.0.1:5674:5674 -p 192.168.1.60:5674:5674 \
       --restart unless-stopped $ENV korg:latest
   EOF
   ```
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
since the build tags each one `korg:<short-sha>`, that list is readable. To
revert, recreate the container from the previous image (from preflight step 5)
using the same step-3 command with that tag in place of `korg:latest`.

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

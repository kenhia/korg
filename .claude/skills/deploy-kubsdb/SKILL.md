---
name: deploy-kubsdb
description: Build the korg image from the current working tree and deploy it to the kubsdb production host (web UI + REST + MCP on :5674). Use when asked to deploy/redeploy/ship korg to kubsdb, or to push local changes live for UI verification before landing a sprint.
---

# Deploy korg to kubsdb

Builds `korg:latest` locally from the **current working tree** (the Dockerfile
compiles the SvelteKit bundle and the Rust release binary from source, so
uncommitted changes ship — you do NOT need to commit first) and deploys it to
`kubsdb` over SSH. There is no image registry.

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
| ports   | `0.0.0.0:5674 -> 5674` |
| restart | `unless-stopped` |
| env     | `DATABASE_URL=postgres://korg:…@postgresql:5432/korg`, `KORG_WEB_DIR=/app/web/build`, `KORG_LISTEN_ADDR=0.0.0.0:5674` |
| mounts  | none |

**Never hand-copy the DB password.** Step 3 reuses the env from the running
container via `docker inspect`, so the secret never leaves kubsdb.

`kubsdb`'s login shell is **fish** — pipe multi-line remote scripts through
`ssh kubsdb bash -s` (a bare `ssh kubsdb '…'` runs under fish and mis-parses
`$()` / `{{…}}`).

## Preflight

1. `ssh -o ConnectTimeout=5 kubsdb hostname` — confirm reachability.
2. From the repo root, confirm `pnpm --dir web check` and the Rust tests are
   green, or that the caller has already verified. A broken build wastes a
   multi-minute image build.
3. Note the currently-deployed image so a rollback target exists:
   `ssh kubsdb 'docker inspect korg --format "{{.Image}}"'`.

## Procedure

1. **Build** (from repo root; the Rust release stage takes a few minutes — run
   it in the background and poll the log):
   ```bash
   docker build -t korg:latest .
   ```
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
       -p 5674:5674 --restart unless-stopped $ENV korg:latest
   EOF
   ```
4. **Verify:**
   ```bash
   curl -fsS https://kubsdb.encke-wahoo.ts.net:5674/api/health
   ```
   Then smoke-test whatever the deploy was for — e.g. a deep link returns 200
   (`curl -s -o /dev/null -w '%{http_code}' https://kubsdb.encke-wahoo.ts.net:5674/plan`), a new
   endpoint responds, and if MCP tooling changed:
   `bash scripts/mcp-roundtrip-check.sh`.

## Rollback

Old images stay in kubsdb's local store. To revert, recreate the container from
the previous image id (from Preflight step 3) using the same step-3 command with
that id in place of `korg:latest`.

## After a successful deploy

Record it: append a short "Deployed <date>" note to the sprint's
`sprints/<branch>/README.md` (image sha, what was verified live), mirroring the
sprint-001 `DEPLOY.md` result section. This keeps the deploy history with the
work that shipped.

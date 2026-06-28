# Deploy — sprint 001 to `kubsdb`

## Target

`kubsdb` (192.168.1.60) runs a single `korg` Docker container from the
`korg:latest` image, serving web + REST + MCP on `:5674`. There is no image
registry; deploys ship the image over SSH.

Existing run config (preserve on redeploy):

| Setting   | Value                                                             |
|-----------|------------------------------------------------------------------|
| network   | `kubsdb-net`                                                      |
| ports     | `0.0.0.0:5674 -> 5674`                                            |
| restart   | `unless-stopped`                                                  |
| env       | `DATABASE_URL=postgres://korg:…@postgresql:5432/korg`            |
|           | `KORG_WEB_DIR=/app/web/build`, `KORG_LISTEN_ADDR=0.0.0.0:5674`    |
| mounts    | none                                                              |

## Procedure

1. Build the image locally (dev host is linux/amd64, same as kubsdb):
   ```bash
   docker build -t korg:latest .
   ```
2. Ship it over SSH (no registry):
   ```bash
   docker save korg:latest | ssh kubsdb 'docker load'
   ```
3. Recreate the container on kubsdb, reusing the existing env from the running
   container so the DB password isn't copied by hand:
   ```bash
   ssh kubsdb '
     ENV=$(docker inspect korg --format "{{range .Config.Env}}-e {{.}} {{end}}")
     docker rm -f korg
     docker run -d --name korg --network kubsdb-net \
       -p 5674:5674 --restart unless-stopped $ENV korg:latest
   '
   ```
4. Verify:
   ```bash
   curl -fsS http://kubsdb:5674/api/health
   bash scripts/mcp-roundtrip-check.sh   # tools/list should include update_work_item
   ```

## Result

Deployed 2026-06-28.

- Built `korg:latest` locally, shipped via `docker save | ssh kubsdb docker load`.
- Recreated the `korg` container (env reused from the running container; note
  kubsdb's login shell is fish, so the script was piped through `bash -s`).
- New image `sha256:6093a1fc…`; container healthy on first healthcheck.
- `tools/list` over the deployed `/mcp` reports 17 tools including
  `update_work_item`.
- Live round-trip: `update_work_item {wi_number:92, wi_status:"resolved"}` ->
  `{"ok":true}`; `get_work_item 92` reads back `wi_status: resolved`.

WI #92 is fixed and resolved in production.

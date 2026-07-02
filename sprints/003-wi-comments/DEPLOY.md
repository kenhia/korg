# Deploy — sprint 003 to `kubsdb`

Same procedure as sprints 001/002 (image over SSH, recreate container reusing
env, `bash -s` because kubsdb's login shell is fish). Migration 0007 auto-applies
on startup via `connect()` → `migrator().run()`.

```bash
docker build -t korg:latest .
docker save korg:latest | ssh kubsdb 'docker load'
ssh kubsdb 'bash -s' <<'SH'
set -e
ENV=$(docker inspect korg --format "{{range .Config.Env}}-e {{.}} {{end}}")
docker rm -f korg >/dev/null
docker run -d --name korg --network kubsdb-net \
  -p 5674:5674 --restart unless-stopped $ENV korg:latest
SH
```

## Result

Deployed 2026-07-02.

- New image `sha256:d9470952…`; container healthy after ~6s (health implies
  migration 0007 applied cleanly — `connect()` migrates before serving).
- **Payoff verified against existing data:** `GET /api/nodes/163/comments`
  (WI #104, `kvllm`) returns its **2** pre-existing comments, now tagged
  `node_id: 163` — the same comments an agent added via `add_comment` that were
  previously invisible in the work-item UI. They render in the work-item detail
  view (read-only, no edit mode needed).
- Old `GET /api/cards/163/comments` route correctly returns **404** (replaced by
  the node-scoped route).

## Note

The MCP `add_comment`/`list_comments` param was renamed `card_node_id` → `node_id`.
Clients holding the sprint-002 tool schema must reconnect (Developer Reload
Window) to pick up the new parameter.

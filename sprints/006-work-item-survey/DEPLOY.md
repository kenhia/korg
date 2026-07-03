# Deploy — sprint 006 to `kubsdb`

Same procedure as sprint 005 (compose file is authoritative):

```bash
docker build -t korg:latest .
docker save korg:latest | ssh kubsdb 'docker load'
ssh kubsdb 'docker rm -f korg; cd /datastore/korg && docker compose up -d'
```

## Result

Deployed 2026-07-03.

- Container healthy within a few seconds of `compose up`.
- **Payoff verified against real data**: `GET /api/work-items/survey?limit=2`
  against the live instance (118 total work items at deploy time) returned
  exactly 2 items and `total: 118` — pagination and the full-count window
  function both correct against production-scale data, not just the test
  fixtures. Slim projection confirmed: response keys are `node_id, project,
  title, wi_number, wi_status, wi_tshirt, wi_type` only — no `content` or
  `details`.
- No schema migration in this sprint, so no migration-apply log line to
  check on startup.

## Note

Same MCP tool-cache caveat as every prior sprint that added tools: clients
holding the pre-006 schema (29 vs 28 tools) need to reconnect to see
`survey_work_items`.

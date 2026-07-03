# Deploy — sprint 004 to `kubsdb`

Same image-over-SSH procedure as sprints 001-003, with one correction: don't
reconstruct the `docker run` invocation from `docker inspect` on a container
you're about to remove — read `/datastore/korg/docker-compose.yml` +
`korg.env` instead. They're authoritative and were already there.

```bash
docker build -t korg:latest .
docker save korg:latest | ssh kubsdb 'docker load'
ssh kubsdb 'docker rm -f korg; cd /datastore/korg && docker compose up -d'
```

## What went wrong first (kept for the next person's sake)

The first attempt tried to snapshot the running container's env with
`docker inspect korg --format '{{range .Config.Env}}-e {{.}} {{end}}'` and
re-run it by hand, piped through two nested SSH hops (cleo → kai → kubsdb)
inside a `fish -l -c "..."` wrapper. Fish's heredoc handling and three layers
of shell-quoting mangled the `$ENV` capture; the container came back up with
an **empty** environment and crash-looped on `Error: DATABASE_URL is
required`. Recovery: `find / -iname "*korg*"` on kubsdb turned up
`/datastore/korg/docker-compose.yml` + `korg.env` — the actual source of
truth, checked into neither git nor memory but sitting right there on the
host. `docker rm -f korg && docker compose up -d` from that directory fixed
it in one shot. Lesson for future deploys: check for a compose file on the
target host *before* trying to hand-reconstruct a `docker run` command.

## Result

Deployed 2026-07-03.

- Container healthy ~3s after `compose up`; log line confirms migration 0008
  applied cleanly (`relation "_sqlx_migrations" already exists, skipping`
  from prior migrations, then `korg-api listening`).
- **Payoff verified against real data**: created the 5 proposals from the
  Agent-Plan bootstrap as real `sprint_proposal` nodes via
  `POST /api/proposals` (curl, since this session's MCP tool cache predates
  the deploy — same "reconnect to pick up new tools" caveat as sprint 003's
  `add_comment` rename). `GET /api/proposals` returns them in rank order
  (1-5); `tools/list` over MCP reports **28** tools including
  `propose_sprint`, `list_proposals`, `update_proposal`.
- The 5 original `Agent-Plan` bootstrap work items (`#108`-`#112`) archived
  with comments cross-referencing their replacement `sprint_proposal` node
  ids (174-178) — history preserved, not deleted.

## Note

Clients holding the pre-sprint-004 tool schema (28 vs 25 tools) must
reconnect to pick up `propose_sprint` / `list_proposals` / `update_proposal`
— same caveat as every prior sprint that added MCP tools.

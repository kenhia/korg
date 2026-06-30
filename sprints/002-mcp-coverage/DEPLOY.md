# Deploy — sprint 002 to `kubsdb`

Same procedure as sprint 001 (see `sprints/001-wi-update/DEPLOY.md` for the
full rationale): no registry; ship the image over SSH and recreate the
container, reusing its env. kubsdb's login shell is fish, so the recreate
script is piped through `bash -s`.

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

Deployed 2026-06-28.

- New image `sha256:92ad1624…`; container healthy on first healthcheck.
- Deployed `/mcp` `tools/list` reports **25** tools; all 8 new tools present
  (`create_project`, `create_area`, `list_areas`, `update_card`, `unrelate`,
  `add_comment`, `list_comments`, `delete_comment`).
- Live verification (idempotent / read-only, no prod data added):
  - `create_project {name:"korg"}` -> `{"id":11}` (returned existing id).
  - `list_areas {project:"korg"}` -> `[calendar, engine, mcp, ui]`.

MCP surface now matches the REST surface for the capabilities agents need to
manage work (deferred low-value items: `set_node_tags`,
`set_link_disposition`, `recent_project`).

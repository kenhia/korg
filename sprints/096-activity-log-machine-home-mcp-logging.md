# 096 — activity log, per-machine home table, agent-stamped /mcp logging

Proposal korg:4314. It covers korg's whole open backlog except the parked WI
1349: WI 2936 (activity log and UI), WI 2919 (`machine` table) and WI 4242
(`/mcp` caller logging). The sprint ran as a karc leg on kai (`korg-9ae4b3`),
under oversight. The overseer's readiness comment is 4282, and Ken's rulings
are 4278 (2919), 4279 (2936) and 4280 (4242). The work went in the proposal's
order: 4242, then 2919, then 2936.

## Premise check

- **4242 holds.** `rg -i homelab-agent crates/korg-api` found nothing.
  korg-gate stamps the header (`crates/korg-gate/src/config.rs`
  `AGENT_HEADER`), and korg-api did not log it.
- **2919 holds.** The `POSIX_HOME = "/home/ken"` constant was in
  `connectors.rs`. Live `projects.machines` names four hosts: kai (40
  projects), kubs0 (7), cleo (7) and thrall (1). Homes were probed from kai
  with `ssh -o BatchMode=yes`. kai, kubs0, thrall and kubsdb are `/home/ken`.
  kimac is `/Users/ken`, which confirms the item's guess and is the one host
  the constant got wrong. komarchy timed out on port 22, so it is not seeded.
  No project names it.
- **2936 holds.** 0026's `transition` was the only change log. It has two
  readers: the board ticker (`list_transitions`) and the flow series
  (`flow.rs`).

## Decisions

- **4242: one INFO line per `/mcp` request**, written by middleware on the
  `/mcp` route only (`crates/korg-api/src/mcp_log.rs`). It logs `agent` (the
  `X-Homelab-Agent` value, or `-`), `http`, and `rpc`. `rpc` is the JSON-RPC
  method, plus the tool name for `tools/call`, comma-joined for a batch. The
  log target is `korg_api::mcp`. The middleware buffers the body to read it,
  which costs nothing new: rmcp parses the whole message before dispatching it
  anyway. There is no body cap beyond what rmcp already applies.
- **2919: two columns, REST only, and the rows are data written after
  deploy.**
  - Migration 0037 adds `machine (name, home)`, with CHECKs on both. A NULL
    `home` means the host does not expand `~`.
  - `project.machines` stays a `TEXT[]`, with no foreign key.
  - The write surface is `GET /api/machines` and `PUT /api/machines/:name`
    with a body of `{"home": …}`. `home` is required and may be `null`.
  - There is no MCP tool, so korg-gate's allowlist needs no ruling.
  - The fleet's rows are not seeded in the migration (0035's rule, restated in
    0036). They are seeded after deploy; see "Deploy-time step".
  - The connector now reports `~/` paths it cannot expand in an additive
    `unlocated` array, as `{name, host, src_path, reason}`, instead of
    guessing. That is additive within connector v1, so `version` stays 1.
- **2936: a sibling table, not a generalised `transition`.** Migration 0038
  adds `activity` beside `transition`, and `GET /api/activity` reads the two
  with `UNION ALL`. Generalising `transition` would have meant changing both
  of its readers in the same deploy and re-arguing its
  `transition_actually_changed` invariant. The sibling table touches neither,
  which is the migration-risk call the 2936 thread asked for.
  - **Hooked in korg-core, never a trigger** (0023/0026). The hooks cover:
    - creation of every node kind except attachments;
    - edits in all seven `update_*` paths, plus `set_link_disposition`,
      `mark_link_read`, `set_node_tags`, and a same-day `upsert_report`
      replace;
    - comment add, edit and delete;
    - `relate` and `unrelate`;
    - `create_attachment` with an owner, and `delete_attachment`;
    - `create_handoff`'s owner edges.
  - **The no-op guard for edits is a fingerprint.** It is an md5 of the node
    row plus its satellite row, taken before and after the write inside the
    same transaction. It leaves out:
    - `updated`;
    - the awaiting marker, which `settle_awaiting` may clear as a side effect;
    - the status column of the three kinds `transition` covers.

    So a status-only write is exactly one `status_changed` row. One generic
    helper was chosen over per-field comparisons in each path, so a field
    added to a kind later is covered without anyone remembering to.
  - The other no-op guards:
    - `relate` logs only when it inserted (`RETURNING (xmax = 0)`);
    - `update_comment` logs only when the body or origin moved.
  - **What is not logged, on purpose:**
    - a node's own deletion (its history cascades, as in 0026);
    - create-time edges other than `has_handoff` (`covers`, `includes`,
      `finding` and `materializes` are part of their node's `created`);
    - the awaiting marker;
    - a report's `reviewed` flag (its own code says marking read is not a
      change);
    - schedule materialisation on the schedule itself (the work item it
      creates is logged);
    - projects and areas, which are not nodes.

    All of these are listed in `docs/api.md`.
  - **`has_attachment` reads as `attached`/`detached`**, whichever path wrote
    the edge.
  - **Context is resolved at read time.** The proposal is the newest `covers`
    edge into the target. The program is the newest `includes` edge into that
    proposal, or into the target when the target is a proposal. Nothing is
    copied onto the row.
  - **REST only, no MCP tool.** The one consumer is korg's own page, so
    korg-gate's `READ_TOOLS` and `WRITE_TOOLS` need no ruling. This was the
    proposal's "Watch for" item.
  - **Ordering:** `at` descending. Within one transaction's `now()`, the
    status change sorts above the edit, then the newer id goes first.
  - No backfill and no actor column, as in 0026.
- **The page follows Ken's v1 line format.** It reads
  `11/1 20:37: Comment Added - WI:1234 title  proposal:5678 program:9012`,
  in browser-local time. The filters are action, kind, project and a time
  window (any, 1h, 24h, 7d or 30d). All of them live in the URL, as on
  Search. It has a "Load more" button and a refresh link. "Activity" sits in
  the nav before "Plan".

## What shipped

- `crates/korg-api/src/mcp_log.rs`, layered on `/mcp` (4242).
- `crates/korg-core/migrations/0037_machine.sql`,
  `crates/korg-core/src/repo/machines.rs`, the machine routes, and the
  connector's lookup and `unlocated` array (2919).
- `crates/korg-core/migrations/0038_activity_log.sql` and
  `crates/korg-core/src/repo/activity.rs`, with the hooks across the repo
  modules (2936).
- `ACTIVITY_ACTIONS` in `vocab.rs`, exported to the web app.
- The `/api/activity` route, `api.activity`, and
  `web/src/routes/activity/+page.svelte`.
- Docs:
  - `docs/api.md`: "The activity log (2936)", and the connector section
    rewritten for the machine table and `unlocated`;
  - `docs/usage.md`: REST rows for `/api/activity` and `/api/machines`, and
    the activity `action` vocabulary bullet;
  - `docs/gate.md`: korg-api now logs the header.

## Verification

- `just check` is green: fmt, gen-check, web check and lint, clippy with
  `--all-targets`, and the full workspace tests.
- New tests:
  - `korg-core/tests/sprint096.rs` (10 tests): the vocabulary is pinned to
    the CHECK; the no-op guard covers edits, comments and edges; a status
    change is counted once for work items, proposals and programs; a deleted
    comment keeps its event; a handoff lands in its owner's feed; context is
    resolved at read time; the filters work, with `total`; and bad input is
    `invalid_input`.
  - `korg-api/tests/sprint086.rs` gained 3 tests: a kimac home, `unlocated`
    reasons, and the REST machine surface with its validation. The existing
    connector tests now seed hosts through `PUT /api/machines`.
  - `mcp_log` has 4 unit tests.
- **Live, on kai, against a throwaway Postgres:**
  - MCP calls stamped `X-Homelab-Agent: claude-duet` logged
    `agent=claude-duet http="POST" rpc=tools/call:relate`. An unstamped
    `tools/list` logged `agent=-`.
  - The connector placed korg at `/home/ken/src/tools/korg` and listed a
    komarchy project as `unlocated` with `no_machine_row`.
  - A screenshot of the Activity page is attached to the proposal (see the
    handoff). A same-value status re-PATCH did not appear in it.
- **Migration rehearsal against today's production backup**
  (`kubsdb:/gratch/backups/korg/korg-20261102-031934.sql.gz`, restored into a
  throwaway `postgres:16`):
  - 0037 and 0038 applied with `success = t`.
  - `/api/activity` served the existing transition history straight away.
  - Before seeding, the connector located 7 projects (cleo, drive-root paths)
    and listed 48 as `no_machine_row`. After the six `PUT`s below it located
    all 55, with `unlocated` empty.

## Deploy-time step (for the ship turn)

Between the deploy and these six writes, the connector lists every `~/`
project as `unlocated` (48 of 55 on today's corpus), and kctrldeck's Code tab
drops them. Run the writes **immediately** after `deploy-kubsdb` verifies:

```sh
K=https://kubsdb.encke-wahoo.ts.net:5674
for m in kai:/home/ken kubs0:/home/ken kubsdb:/home/ken thrall:/home/ken kimac:/Users/ken; do
  curl -s -XPUT "$K/api/machines/${m%%:*}" -H 'content-type: application/json' -d "{\"home\":\"${m#*:}\"}"; echo
done
curl -s -XPUT "$K/api/machines/cleo" -H 'content-type: application/json' -d '{"home":null}'; echo
curl -s "$K/api/connectors/projects" | jq '.unlocated'     # expect []
```

## Cross-repo changes made

None.

## Repaired in passing

None.

## Follow-ups

None filed.

## Deployed

Deployed on 2026-11-02 at about 10:56 PST, through `deploy-kubsdb`, by the
same karc leg on its ship turn. The overseer cleared it in korg:4314 comment
4284.

- **What shipped:** squash merge `7a25ffd` (PR #104). The image is
  `kubsdb.encke-wahoo.ts.net:5000/korg:7a25ffd3659b` (plus `latest`). The
  running container's revision label matched the built commit. The rollback
  target is `korg:b73c14155380`, confirmed in the registry. Rolling back past
  0037/0038 is a re-tag; the tables are inert to the old image.
- **Migrations 0037 and 0038** applied at startup. `post-deploy-check
  --compare` was OK, and every count was unchanged from the baseline (node
  count 4217).
- **Machine seed:** the six `PUT /api/machines` calls ran straight after the
  health check. kai, kubs0, kubsdb and thrall are `/home/ken`, kimac is
  `/Users/ken`, and cleo is `null`. `GET /api/connectors/projects` then
  located 55 projects, with `unlocated: []`.
- **Activity log:** `GET /api/activity` serves the existing transition
  history (4826 events at verification). The newest entry, WI 2936's status
  change, resolves its proposal, korg:4314. `/activity` and `/plan` return
  200.
- **/mcp logging:** `docker logs korg` shows the new lines, for example
  `korg_api::mcp: mcp request agent=- http="POST" rpc=tools/call:list_projects`.
  No `agent=claude-duet` line yet: Duet had not called korg since the deploy.
  It is the first Duet call that will show one.

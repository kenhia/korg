# 094 — korg-gate: a second door for klams (read-only), and `server/discover`

Proposal korg:4297, slice 2 of program korg:4299 ("Claude Duet front door,
phase 2"). This is the **code half**: WI 4293 (klams upstream at `/klams/mcp`
behind the same OAuth server) and WI 4243 (admit `server/discover`). WI 4294,
the ops half, stays open. It covers the tailnet grant, the deploy to krgate,
the second connector and the phone test, and runs interactively with Ken after
this ships. Nothing was deployed to krgate, and the tailnet policy was not
touched. This sprint ran as a karc leg on kai (`korg-131184`), under oversight.

## Premise check

- **WI 4293 holds.** The gate had one upstream (`KORG_GATE_UPSTREAM`), one
  hardcoded allowlist, one resource, and issuer = origin, as the item says.
  klams's live catalogue (klams-mcp 0.1.55, probed from kai) has exactly the
  eleven tools the item lists. Two facts the item did not have:
  - `claude-duet` **already exists on klams** (slice 1 landed), and klams
    already narrows that identity's `tools/list` to the four reads. So the
    gate's own forcing test needs the catalogue fetched as an identity that sees
    everything (`claude`), not as `claude-duet`.
  - klams is **stateful** (`Mcp-Session-Id`) and answers in **SSE**, opening
    every stream with an empty `data:` priming event. The gate's SSE filter
    failed closed on that event, so a klams `tools/list` would have come back as
    an error. Fixed here, because the door does not work without it.
- **WI 4243: the premise drifted, in the direction that helps.** The brief
  expected korg-api's rmcp 3.1.2 to answer `server/discover` with an error.
  It does not. rmcp 3.1.2 implements MCP 2026-07-28. Here is what each probe
  actually got, measured 2026-11-01 against korg-api on kubsdb and klams on
  kubs0, both directly and through the new gate:
  - No `_meta`: HTTP 400,
    `-32602 Invalid params: request _meta is missing or has malformed required fields: io.modelcontextprotocol/protocolVersion, io.modelcontextprotocol/clientCapabilities`.
  - With `_meta` but no `Mcp-Method` header: HTTP 400,
    `-32020 missing required Mcp-Method header`.
  - Well-formed: HTTP 200, `resultType: complete`,
    `supportedVersions: [2024-11-05, 2025-03-26, 2025-06-18, 2025-11-25, 2026-07-28]`,
    capabilities and instructions (klams adds `serverInfo` in `_meta`).
  - A mismatched header gets HTTP 400 `-32020`, e.g. "Mcp-Method header
    `tools/list` does not match body method `server/discover`". `tools/call`
    also requires `Mcp-Name`, and the upstream checks it against the tool name.

  The old gate answered every `server/discover` with its own `-32601 method
  ... is not available through korg-gate`.
- Cross-project plan: korg routes to `korg+/`. GP-17 records the gate as one
  scoped exception and says "a second door … is a new decision". Ken made it
  2026-10-31, and it is recorded below as a Branch A cross-repo change.

## Decisions

- **The multi-upstream design on WI 4293: confirmed.** One process, one
  issuer (the origin), per-path RFC 9728 protected-resource metadata, and RFC
  8707 resource binding on every code, access token and refresh token. The
  reason is the one the item gives, and it held up. RFC 8414 metadata lives at
  the origin root, so two gate processes on one Funnel hostname would both
  claim the same issuer. Path-based issuers would have meant a second
  passphrase prompt surface and a second client table for no gain. The
  fallback (path-based issuers) was not needed. The tests drive the connector
  shape claude.ai uses (`resource` on both the authorize and the token
  request), and binding by `resource` handles it.
- **A door is a code variant, not configuration.** `Door { Korg, Klams }` in
  the new `crates/korg-gate/src/door.rs` owns the path, scope and allowlist.
  The environment only says where an upstream lives. A config file of
  arbitrary upstreams would make publishing another server's tools to the
  internet a config edit, and GP-17 makes it a decision. One new variable,
  `KORG_GATE_UPSTREAM_KLAMS`, is optional. Unset or empty, the klams door does
  not exist: its paths are 404s and a `resource` naming it gets
  `invalid_target`. The running unit's env file therefore needs no change to
  keep working, and the unit file itself does not change.
- **A request that names no `resource` is korg's.** That is what every request
  was before klams existed. The one exception is a request whose `scope` asks
  for `klams:read` and not `korg:read`. That is the scope the klams door's
  metadata and 401 advertise, so a client that skips RFC 8707 still lands at
  the right door instead of being handed a korg token for the klams connector.
- **The authorization-server metadata still lists only `korg:read`.** That
  document is shared by both doors, and RFC 8414 lets a server leave scopes
  unadvertised. Listing `klams:read` would change what the live korg connector
  sees, for no client that needs it.
- **The 401 for klams adds `scope="klams:read"`** (RFC 6750 §3) and points at
  the path-inserted metadata. korg's 401 is the old string, byte for byte.
- **A token at the wrong door is a plain 401, not a 403.** It is
  indistinguishable from no token, and it points at the right door's metadata,
  so a confused client re-authorizes for the door it is actually calling. A
  refresh request naming the other door's `resource` gets `invalid_target`
  and **does not spend the token**. It proves nothing about the holder.
- **The klams forcing test is total.** klams has no `get_`/`list_` convention,
  so every tool in the committed catalogue (`crates/korg-gate/tests/klams_tools.json`)
  must be in `KLAMS_READ_TOOLS` or `KLAMS_REFUSED_TOOLS`. The test also pins
  `KLAMS_READ_TOOLS` to the four names, so widening it fails a test that says
  why.
- **`server/discover`: option (a), as ruled.** It is in `METHODS` and is
  forwarded; the upstream's answer stands. Pinned in `tests/flow.rs`
  (`server_discover_is_forwarded_and_the_upstream_answer_stands`), with stubs
  answering exactly as rmcp 3.1.2 was measured to.
- **2026-07-28's routing headers are the gate's own, never the caller's.**
  *Flagged for the overseer's ruling (proposal comment).* Admitting
  `server/discover` while stripping `Mcp-Method` would hand a 2026-era client
  `-32020 missing required Mcp-Method header`, a modern-era error it may not
  fall back from, instead of the clean refusal it falls back from today.
  Every later 2026-era call would then fail the same way, because `Mcp-Name`
  is stripped too. Forwarding the caller's headers raw would let header and
  checked body disagree, the same hazard the gate's re-serialisation rule
  exists for. So, if the caller sends `Mcp-Method`, it must equal the checked
  method, and the gate forwards its own copy. If the caller sends `Mcp-Name` on
  `tools/call`, it must equal the checked tool name. A mismatch is refused at
  the gate (HTTP 400, `-32020`, the upstream's wording). A caller that sends
  neither, which is every 2025-era client including today's connector, is
  forwarded exactly as before. This is WI 4243's "check whether the transport
  headers need passing through", answered by measurement.

## What shipped

- `crates/korg-gate/src/door.rs`: the `Door` type.
- `config.rs`: `klams_upstream` (`KORG_GATE_UPSTREAM_KLAMS`), plus
  `resource_for`, `upstream_for`, `doors` and `door_for_resource`.
- `store.rs`: codes, access grants and refresh grants carry a `Door`.
  `#[serde(default)]` makes a pre-094 grant korg's. `WrongResource` is a new
  `GrantError` that maps to `invalid_target`.
- `oauth.rs`: per-door protected-resource metadata, door resolution on
  authorize and token, a hidden `resource` field on the sign-in form for
  non-korg doors only (so korg's page is unchanged), and the page names the
  door.
- `proxy.rs`: per-door auth, allowlist, upstream and messages; the SSE
  priming-event fix; the routing-header rule.
- `allowlist.rs`: `KLAMS_READ_TOOLS` and `KLAMS_REFUSED_TOOLS`, and
  `server/discover` in `METHODS`.
- `lib.rs`: routes mounted per configured door.
- Tests. The harness now runs a klams stub that answers like klams 0.1.55 (SSE
  with a priming event, a session id), and **opens both doors by default**, so
  every pre-existing korg-door test now also proves the klams door changes
  nothing at `/mcp`. New tests:
  - the klams flow end to end;
  - door binding, both directions and on all three HTTP methods;
  - refresh with the wrong `resource`, which is refused without spending the token;
  - the scope fallback;
  - the door being absent when unconfigured;
  - a refresh token in the pre-094 state format;
  - `server/discover`, and the routing headers;
  - the klams forcing test, klams write refusals, the klams list being exactly
    four, a klams read refused at the korg door;
  - store tests for legacy and klams grants;
  - a proxy test on klams's literal SSE shape.
- `docs/gate.md`: the doors table and section, the `server/discover` and
  routing-header rules, the klams allowlist, the env variable, the tailnet
  prerequisite, the second connector, operator checks, an "adding a door"
  runbook, and a rollback note.
- `deploy/korg-gate/env.example`: `KORG_GATE_UPSTREAM_KLAMS=` (empty, door
  shut). The unit file does not change. README bullet.

## Verification

- `just check` green (see the ship).
- **Mutation check.** Accepting a token at any door fails three flow tests,
  restored afterwards.
- **The korg door is byte-for-byte unchanged, including the existing refresh
  token.** Proved against the previous binary, not by inspection. On kai, I
  built `main` at `d9e1667` and this branch. Both ran on the same loopback port
  and state directory, against live korg-api.
  1. The old binary served a scripted survey of every korg-door surface. That
     is 16 responses: both metadata forms, AS metadata, the 401 on
     POST/GET/DELETE `/mcp`, a 405, healthz, an evil registration, the sign-in
     page without a resource, with korg's and with a bad one, three token
     errors, and a bogus bearer. Status, headers (minus `date` and
     `content-length`) and body were recorded.
  2. A sign-in on the old binary produced a refresh token, plus its 26-tool
     `tools/list` and its write refusal.
  3. The new binary ran on the same state directory with the klams door
     **open**. The same survey was **identical** (`diff` empty), and identical
     again with the klams door unset. The old binary's refresh token refreshed
     (200, `korg:read`, `expires_in 3600`). `tools/list` returned the same 26
     tools, the write refusal was identical, `get_work_item` read live korg,
     and that korg token got 401 at `/klams/mcp`.

  The one korg-door difference is intended. `server/discover` is now forwarded
  rather than refused, which is WI 4243.
- **Loopback smoke against live klams** (`https://kubs0.encke-wahoo.ts.net:7777/mcp`,
  identity `claude-duet`, which exists):
  - The 401 points at `…/oauth-protected-resource/klams/mcp` with
    `scope="klams:read"`, and the metadata names the klams resource.
  - Sign-in gives a token with scope `klams:read`. `initialize` reaches
    klams-mcp 0.1.55 with a session.
  - `tools/list` returns exactly `event_search, memory_get, memory_related, memory_search`.
  - `memory_search` returns real hits (the korg-gate klams record).
  - `memory_add` is refused at the gate, never reaching klams.
  - The klams token gets 401 at `/mcp`.
- **2026-07-28 era through the gate, both doors, live.**
  - `server/discover` with `Mcp-Method` returns the real `supportedVersions`.
  - Modern `tools/list` is filtered: 26 on korg, 4 on klams.
  - Modern `tools/call` with `Mcp-Name` works (`get_project`, `memory_get`).
  - A mismatched `Mcp-Name` is refused at the gate with `-32020`.

## Cross-repo changes made

- cross-project-planning `72466fa`, on `main` and pushed. One dated paragraph
  appended to GP-17's exception in `korg+/PLAN.md`, recording the klams door
  and its limits, following 7e83eed's procedure (commit straight to `main`,
  push). Checked: `## main...origin/main` after push, and no open PRs.

## Repaired in passing

- The SSE `tools/list` filter failed closed on an empty `data:` event (the
  resumability priming event that rmcp's stateful servers send first). It now
  passes through. Proved by a unit test on klams's literal response shape and
  by the live klams `tools/list`. korg-api answers in JSON, so the korg door
  never hit this.

## Follow-ups

- WI 4294 (ops half), open. The tailnet grant `tag:korg-gate` → kubs0:7777,
  `KORG_GATE_UPSTREAM_KLAMS` in krgate's `/etc/korg-gate/env`, deploy, the
  second connector at `<origin>/klams/mcp`, and the phone test. The program's
  claude-duet repo steps (connectors, rulings, test prompts) belong there too.

## Deployed

Nothing deployed from this ship, deliberately, as the overseer's clearance
said (korg:4297 comment 4261). The declared step is `deploy-kubsdb`, which
rebuilds the korg-api image (`cargo build --release -p korg-api`, plus the web
bundle) from `main`. The squash `194d7f2` (PR #100) changed only
`crates/korg-gate/`, `docs/gate.md`, `deploy/korg-gate/env.example`, the README
and this record. `git diff d9e1667..194d7f2` is empty for `Cargo.lock`, the root
`Cargo.toml`, `crates/korg-api`, `korg-core`, `korg-mcp`, `web/`, `contract/`
and the `Dockerfile`. A rebuild would have published an identical korg-api under
a new tag, so kubsdb stays on the release recorded in sprints 092 and 093. The
gate itself goes to krgate by hand in the ops half (WI 4294), per
`docs/gate.md`. Until then, krgate runs the sprint-093 gate with one door.

# 092 — korg-gate: the read-only Duet front door (code half)

Proposal korg:4148, covering **WI 4147** (code half). WI 4146 (the decision)
was already done. WI 4234 (stand-up on the Pi) is covered too, but it is
tomorrow's interactive ops work and is not in this PR. This sprint ran as a
karc leg on kai (`korg-a25007`), under oversight.

## Goal

Claude Duet reaches tools only through claude.ai custom connectors, which must
be public HTTPS. korg is tailnet-only and has no auth. Ken allowed one scoped
exception (WI 4146): an OAuth-gated, **read-only** front door on an isolated
Pi behind Tailscale Funnel. This sprint builds the gate itself. It opens no
ingress.

## Premise check

- **WI 4147 holds.** korg-api's `/mcp` is rmcp streamable-HTTP, configured
  stateless with JSON responses (`crates/korg-api/src/lib.rs`, `mcp_service`).
  The catalogue the allowlist test reads, `crates/korg-mcp/tests/tools_schema.json`,
  exists with 57 tools. kai has `gcc-aarch64-linux-gnu`. axum, tokio and tower
  are already workspace dependencies.
- Cross-project plan: korg routes to `korg+/`. GP-17's rule is "embedding is
  not authentication". The posture statement inside it, "the tailnet is the
  perimeter", needed the gate recorded as its one scoped exception (WI 4146).
  On the overseer's ruling, that went in this ship as a Branch A cross-repo
  change: one dated paragraph appended to GP-17, cross-project-planning
  `7e83eed`, on `main` and pushed.

## Decisions

- **The issuer is the public URL's origin, and the resource is the full URL.**
  `KORG_GATE_PUBLIC_URL` (`https://…/mcp`) is the `resource`, byte for byte.
  The issuer is its origin, so RFC 8414 metadata sits at the root well-known
  path every client probes, not at a path-inserted variant only some clients
  try. Protected-resource metadata is served at both the root and the `/mcp`
  path-inserted forms.
- **An explicit tool list, not a prefix rule.** `READ_TOOLS` names 26 tools.
  A test fails when korg adds a read-shaped tool the list has not ruled on, and
  when the list names a tool korg dropped. Exposing a tool to the internet is
  then a decision someone makes, not a naming accident.
- **Forward the re-serialisation, not the caller's bytes.** With duplicate
  JSON keys, the gate's parser and korg's could disagree about which tool is
  being called. Forwarding what was checked closes that gap. A test smuggles
  `delete_area` behind `get_item` and checks what reaches the stub.
- **`tools/list` fails closed.** An answer the gate cannot parse, whether JSON
  or SSE, becomes a JSON-RPC error, never the unfiltered list. Batches are
  refused whole.
- **DCR only, no CIMD.** CIMD would make the gate fetch a URL the client
  names. DCR keeps the gate making no outbound call except to korg. Only
  `https://claude.ai/api/mcp/auth_callback` may be registered. Claude Code's
  loopback redirect is out on purpose.
- **The client table is bounded**, because registration is unauthenticated.
  The cap is 64; when it is full, the oldest client holding no live grant is
  evicted, and a client with a live grant is never evicted.
- **Tokens are stored as digests.** `state.json` holds SHA-256 digests of
  refresh tokens. Access tokens and codes live in memory only. A restart costs
  a refresh, not a sign-in.
- **The lockout is global.** After 5 free failures, attempts are refused for
  30 s, doubling per further failure, capped at 1 h. Per-address limits do
  nothing against an attacker with many addresses. The cost is that the owner's
  *sign-in* can be delayed. A connected phone only refreshes and is never
  affected.
- **Passphrase floor of 20 characters.** The gate refuses to start below it,
  because a guessable passphrase is risk 1 in korg:4146#comment-4097.
- **The env drift test checks both directions.** The new
  `the_gate_env_table_covers_every_gate_variable` also fails on a documented
  variable the code never reads. That caught `KORG_GATE_LOG` being read through
  `EnvFilter`, where the scanner could not see it; it is now a plain `env::var`.

## What shipped

- `crates/korg-gate`: `config`, `allowlist`, `store` (persisted clients and
  refresh grants, lockout), `oauth` (metadata, DCR, sign-in, token), and
  `proxy` (the `/mcp` POST/GET/DELETE proxy, `Mcp-Session-Id` passed through
  both ways, `X-Homelab-Agent: claude-duet` stamped on). Tests:
  - 12 unit tests;
  - `tests/flow.rs` (8): the whole OAuth flow against an in-process stub
    upstream, the exact 401 shape, registration refusals, PKCE, the lockout,
    and persistence;
  - `tests/allowlist.rs` (5): every write tool in the real catalogue is refused
    on `tools/call` and absent from `tools/list`; non-lifecycle methods,
    notifications and batches are refused; the duplicate-key smuggle fails.
- `deploy/korg-gate/korg-gate.service`: `DynamicUser`, `StateDirectory`,
  `ProtectSystem=strict`, no capabilities, `@system-service` syscall filter.
  Also `deploy/korg-gate/env.example`.
- `docs/gate.md`: what the gate does, the env table, the build, the Pi
  install, Funnel, claude.ai connector steps, operator checks, and passphrase
  rotation.
- `just gate-pi`: an aarch64 cross-build. `.cargo/config.toml` gains the
  aarch64 linker.
- README crate list; a docs_drift test for the gate's env table.

## Verification

- `just check`: green (exit 0). The Dockerfile `rust` stage (`docker build --target rust`) still builds with the new workspace member, so the `.sprint-deploy` image is unaffected.
- `just gate-pi`: `ELF 64-bit LSB pie executable, ARM aarch64`. Its highest
  glibc symbol is `GLIBC_2.34`, which Trixie (2.41) satisfies.
- **Live smoke against production korg**, from kai, loopback only (no
  ingress). The release gate on `127.0.0.1:18475` proxied to
  `https://kubsdb.encke-wahoo.ts.net:5674/mcp`, and a stdlib Python client
  drove 401 → DCR → sign-in (303) → token → `initialize` → `tools/list` (26
  tools, every one a read) → `get_work_item 4147` (real data) → `add_comment`
  (refused with -32602, so nothing reached korg). The gate logged the refusal.

## Follow-ups

- WI 4234 (ops half, interactive with Ken): Pi, UDM network, tailnet policy,
  Funnel, connector, phone test. `docs/gate.md` is its runbook.
- On the first real Funnel sign-in, confirm that the logged `ip` is the phone's
  public address. This is an operator check in `docs/gate.md`, not a work item:
  it needs the Pi, which 4234 already holds.
- korg+ GP-17 now records the gate as one scoped exception to "the tailnet
  is the perimeter" (cross-project-planning `7e83eed`). Nothing is pending.

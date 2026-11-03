# korg-gate — the public front door

`korg-gate` lets **Claude Duet** (and claude.ai on the web, Desktop and mobile)
read korg and klams, and make a ruled set of writes to each. Those clients reach tools only through *custom
connectors*, which must be public HTTPS. korg and klams are tailnet-only, so
something has to stand between the two. That is this crate (`crates/korg-gate`,
one binary).

- **Decision:** WI 4146, a scoped exception to the 2026-07-11 "nothing to the
  public internet" decision. **Build:** WI 4147. **Stand-up:** WI 4234.
- **The klams door:** Ken's decision of 2026-10-31, recorded in korg+ GP-17.
  **Build:** WI 4293 (sprint 094). **Stand-up:** WI 4294.
- **korg writes:** Ken's ruling of 2026-10-31 (WI 4244, comment 4264), recorded
  in korg+ GP-17. **Build:** sprint 095 (proposal korg:4298, code half). The ops
  half is below, under [Turning the writes on](#turning-the-writes-on).
- **klams writes:** Ken's ruling of 2026-11-02 (WI 4324, answering WI 4307),
  recorded in korg+ GP-17. **Build:** sprint 097 (proposal korg:4326). The ops
  half is korg:4308, below under
  [Turning the klams writes on](#turning-the-klams-writes-on).
- **korg-api and klams do not change.** The gate is a client of each one's
  `/mcp`, the same as any fleet agent.
- **It does not ship with the kubsdb image.** It runs on its own Pi, on an
  isolated network, behind Tailscale Funnel. `just deploy`/`.sprint-deploy` never
  touch it.

## What it does

| Path | Purpose |
|---|---|
| `/.well-known/oauth-protected-resource` (and `…/mcp`) | RFC 9728 metadata for the korg door: `resource` is the connector URL, `authorization_servers` is the gate itself |
| `/.well-known/oauth-protected-resource/klams/mcp` | The same for the klams door: `resource` is `<origin>/klams/mcp`, scope `klams:read` |
| `/.well-known/oauth-authorization-server` | RFC 8414 metadata: DCR, S256 PKCE, public clients, `korg:read` + `offline_access` |
| `POST /register` | RFC 7591 dynamic client registration. Only `https://claude.ai/api/mcp/auth_callback` may be registered |
| `GET`/`POST /authorize` | The sign-in page: one owner passphrase, no accounts |
| `POST /token` | Code + PKCE exchange, and refresh with rotation. A dead refresh token gets `invalid_grant` |
| `POST`/`GET`/`DELETE /mcp` | The korg door. Without a live korg token: `401` + `WWW-Authenticate: Bearer resource_metadata="…"` |
| `POST`/`GET`/`DELETE /klams/mcp` | The klams door, only when `KORG_GATE_UPSTREAM_KLAMS` is set (otherwise `404`, as is its metadata). Without a live klams token: `401` pointing at its own metadata, with `scope="klams:read"` |
| `GET /healthz` | `ok` |

## Doors

One process, one authorization server, two doors (sprint 094). The issuer is
the origin for both, so there is one passphrase, one client table and one
lockout. What a door owns is its path, its upstream, its OAuth `resource` and
scope, and its tool allowlist:

| Door | Path | Upstream | `resource` | Scope | Tools |
|---|---|---|---|---|---|
| korg | `/mcp` | `KORG_GATE_UPSTREAM` | `KORG_GATE_PUBLIC_URL`, byte for byte | `korg:read` | `READ_TOOLS` (26) + `WRITE_TOOLS` (8) |
| klams | `/klams/mcp` | `KORG_GATE_UPSTREAM_KLAMS` | `<origin>/klams/mcp` | `klams:read` | `KLAMS_READ_TOOLS` (4) + `KLAMS_WRITE_TOOLS` (4) |

**Every token is bound to its door** (RFC 8707). A sign-in names its door by
`resource`, which claude.ai sends. The code, the access token and the refresh
token all carry that door. A token presented at the other door gets the same
401 as no token. A token request naming the other door's `resource` gets
`invalid_target`, and does not spend the refresh token. A sign-in that names no
`resource` is korg's, as every sign-in was before klams had a door. The one
exception is a request for `klams:read` alone, which is klams's: that is the
scope its metadata advertises, so a client that skips RFC 8707 still lands at
the right door.

**The korg door's behaviour is unchanged apart from its tools.** Its URL, 401,
scopes and token responses are what they were before klams existed. Sprint 095
gave its tool list the write set, and corrected three human-facing strings that
had become false: the metadata's `resource_name` is now `korg` (was
`korg (read-only)`), and the sign-in page now asks for "access to korg's reads
and the ruled write set" with an **Allow access** button (was "**read-only**
access" and **Allow read access**). Sprint 097 did the same for klams when its
door gained writes: `resource_name` is `klams` (was `klams (read-only)`), the
page asks for "access to klams's reads and the ruled write set", and the button
is **Allow access**. The metadata still lists only `korg:read`. That document is
shared by both doors, RFC 8414 lets a server leave scopes unadvertised, and a
client takes each door's scope from that door's own metadata. Refresh tokens
issued before sprint 094 carry no door in `state.json` and read as korg's.
Sprints 094 and 095 each proved this against the previous binary on one state
directory.

**Adding a door** is a code change on purpose, because each door publishes
another server's tools to the internet (korg+ GP-17). Add a `Door` variant in
`crates/korg-gate/src/door.rs` (path, scope, tools), give it an explicit
allowlist and a forcing test against a committed catalogue in `allowlist.rs`,
and add its upstream variable to `config.rs` and the table below. Then record
the decision in GP-17. The deploy is an ops item: a tailnet grant from
`tag:korg-gate` to the new upstream, the variable in `/etc/korg-gate/env`, a
restart, and a second connector.

**Rolling back** to a one-door binary: it ignores the `door` field, so a
klams refresh token would mint korg tokens. Both are behind the same
passphrase, but to keep the doors apart, stop the unit, delete
`/var/lib/korg-gate/state.json`, and reconnect.

**Deny by default.** Only the MCP lifecycle methods (`initialize`,
`notifications/initialized`, `ping`, `server/discover`, `tools/list`,
`tools/call`) pass, on either door. A `tools/call` passes only for a tool on
that door's list (`crates/korg-gate/src/allowlist.rs`), and `tools/list` is
filtered to the same list. A refused request is a JSON-RPC error; it never
reaches the upstream. A `tools/list` answer the gate cannot parse is refused,
never passed through unfiltered. SSE answers are filtered too, including
klams's empty priming event. Batches are refused.

**`server/discover`** is MCP 2026-07-28's mandatory server method. A dual-era
client (claude.ai's connector) probes with it before falling back to
`initialize`. The gate forwards it and the upstream's own answer stands
(WI 4243), and leaves an INFO `forwarded … method="server/discover"` line in the
journal, as a `tools/call` does. Both upstreams run rmcp 3.1.2, which implements 2026-07-28, so a
well-formed probe gets the real version list back. 2026-07-28 also routes by
the `Mcp-Method` and `Mcp-Name` request headers. The gate never forwards the
caller's copies. If the caller sends one, it must agree with the body the gate
checked: the method, and for `tools/call` the tool name. A mismatch is refused
at the gate with `-32020` on an HTTP 400, and an agreeing header is forwarded as
the gate's own value. A caller that sends neither (every 2025-era client) is
forwarded exactly as before.

The allowlist is explicit names, and the korg ruling is total (sprint 095):
every tool in korg's real catalogue (`crates/korg-mcp/tests/tools_schema.json`)
must be in `READ_TOOLS`, `WRITE_TOOLS` or `REFUSED_TOOLS`. A tool korg adds fails
the build until somebody says which, and so does a ruling naming a tool korg no
longer serves. A read-shaped name (`get_*`, `list_*`) must be a read. **Adding a
tool to `READ_TOOLS` or `WRITE_TOOLS` publishes it to the internet.** Do that on
purpose.

### korg writes (sprint 095)

`WRITE_TOOLS`, exactly as Ken ruled (WI 4244, comment 4264), and pinned by a
test that says widening it is a decision:

| Tool | Why Duet has it |
|---|---|
| `add_comment` | Notes on the thread. `origin` is overwritten (below) |
| `set_awaiting` | Raise and clear the Awaiting Ken lane |
| `create_work_item`, `update_work_item` | The whole tool, status included |
| `create_link` | The reading list, from the phone |
| `relate` | Wire `depends_on`/`covers` while scheduling. `origin` is overwritten |
| `propose_sprint`, `create_program` | Duet's charter: scheduling the work its scans find |

**Refused by name** (`REFUSED_TOOLS`): `update_proposal` and `update_program`
(status transitions start and finish sprints, and stay with Ken and the sprint
skills); `create_handoff`, `update_handoff`; `update_comment`, `delete_comment`;
`update_link`, `delete_link`, `mark_link_read`; `unrelate`; cards
(`create_card`, `update_card`); areas and projects (`create_area`,
`update_area`, `delete_area`, `create_project`, `update_project`); reports
(`create_report`, `review_report`, `set_report_source`); and schedules
(`create_schedule`, `update_schedule`, `materialize_schedule`). A refused call is
a JSON-RPC error and never reaches korg. Widening the set is one allowlist line,
one test, a GP-17 amendment, and a line in claude-duet's `docs/rulings.md`.

**`origin` is the gate's, not the caller's.** korg records `origin` unverified
(#1879), so the gate is the only place it can be made true. On the two korg
tools that carry it, `add_comment` and `relate`, the gate sets
`arguments.origin` to `claude-duet` before forwarding, whatever the caller sent
(a missing or `null` `arguments` gets an object; arguments that are not an
object are refused). It touches no other field and no other tool. For the
writes without an `origin` field, attribution is the gate's journal, which logs
every `tools/call` with its tool name, and the `X-Homelab-Agent: claude-duet`
header on every forwarded request. korg-api logs that header on one INFO line
per `/mcp` request (WI 4242), with the JSON-RPC method and, for `tools/call`,
the tool name: `docker logs korg | grep 'agent=claude-duet'` on kubsdb.

**The second control is claude.ai's per-tool approval.** The ruling's
condition is that every write tool stays on **Needs approval** in the
connector's settings, so each write is Ken's tap on the phone; reads may be
**Always allow**. That is a client setting Ken owns, not something the gate can
see or enforce, so it is re-checked in the ops half and after any reconnect.

**OAuth scope: still `korg:read` (option A).** The korg door's one scope keeps
its name, and the allowlist plus per-tool approval are the controls on writes.
The scope's name is now inaccurate, and is kept on purpose: the alternative, a
`korg:write` scope, would leave every existing token without it, and claude.ai's
connector flow has no scope picker, so the phone would have to be disconnected
and signed in again to gain anything a scope check could add. With one owner,
one passphrase and no second client to tell apart, a scope would separate
nobody. The existing connector gains the write tools on its next `tools/list`,
with no re-sign-in.

### klams

klams has no naming rule to lean on, so its test is total. Every tool in
klams's catalogue, committed at `crates/korg-gate/tests/klams_tools.json`, must
be in `KLAMS_READ_TOOLS` (`memory_search`, `memory_get`, `memory_related`,
`event_search`), `KLAMS_WRITE_TOOLS` (below) or `KLAMS_REFUSED_TOOLS`. A tool
klams adds fails the build until somebody rules on it, and so does a ruling
naming a tool klams no longer serves.

**The fixture is a full-scope view.** klams narrows `tools/list` to the
caller's scopes, so a fixture fetched as a write identity hides the six
`memory_admin_*` tools, and the forcing test would never see them. No live
identity holds `admin`, so sprint 097 generated the fixture from klams's source:
in a throwaway clone, an example calling
`klams_mcp::tools::all_tool_descriptors()` and printing it as pretty JSON. It
first checked that those descriptors, for the eleven tools a `claude` identity
sees, equal the live `tools/list` byte for byte. Refresh it the same way, and
keep the check. A test fails if the fixture carries no admin tools.

#### klams writes (sprint 097)

`KLAMS_WRITE_TOOLS`, exactly as Ken ruled (WI 4324), and pinned by a test that
says widening it is a decision:

| Tool | What the gate does to it |
|---|---|
| `memory_add` | Forwarded only when `arguments.kind` is exactly `"knowledge"`. Anything else (`"fact"`, a missing or `null` kind, `"Knowledge"`) is refused with `-32602` and a message saying the door writes knowledge only. `author_id` is stripped |
| `memory_supersede`, `memory_update` | `author_id` stripped (neither declares one today; the strip is defence in depth) |
| `memory_delete` | `author_id` stripped |

**Refused by name** (`KLAMS_REFUSED_TOOLS`): `memory_append_event` (events are
irreversible), `dissent_propose`, `register_author`, and every admin-scope tool
(`memory_admin_restore`, `memory_admin_hard_delete`,
`memory_admin_list_deleted`, `memory_admin_list_authors`,
`memory_admin_remove_author`, `memory_admin_merge_authors`).

**`author_id` is stripped, not stamped.** klams fills an omitted `author_id`
with the author bound to the forwarded identity, which is `claude-duet`
(`X-Homelab-Agent`, below). So the gate removes the key from every klams write
it forwards, whatever the caller sent, and klams attributes the write to Duet.
The gate does not know Duet's author UUID and does not need to. Missing or
`null` arguments become an object; arguments that are not an object are
refused. Reads are forwarded untouched: `event_search`'s `author_id` is a
filter, not an attribution. klams refusing a foreign `author_id` for every
caller (klams WI 4323) is the second, independent control.

**What `manage` means here.** The ops half gives `claude-duet` the scopes
`read,write,manage` (Ken's ruling), so Duet can supersede, update and delete
other agents' records, not only its own. With `manage`, `memory_delete` can
soft-delete any record. klams deletes are soft and restorable through
`memory_admin_restore`, which needs `admin` and is refused at this door.

**Approval and scope are as on the korg door.** Every klams write tool stays on
**Needs approval** in the klams connector's settings, subject to the finding
in claude-duet WI 4309. The scope keeps its name, `klams:read`, for the same
reason as option A above: claude.ai has no scope picker, and a new scope would
force a re-sign-in to separate nobody. The existing klams connector gains the
four tools on its next `tools/list`.

**Forwarded requests** carry `X-Homelab-Agent: claude-duet`, so korg's logs can
tell Duet traffic apart. For klams that header is the whole credential (klams
sprint 049), so the klams door needs no secret. The gate's own bearer token is
never forwarded.
`Mcp-Session-Id` passes in both directions. The gate forwards its own
re-serialisation of the message it checked, not the caller's bytes, so a
duplicate JSON key cannot make the check and korg see different tools.

**Tokens.** Access tokens live an hour, in memory only. Refresh tokens rotate
on every use, live 30 days, and persist (as SHA-256 digests, never raw) with
the registered clients in `$KORG_GATE_STATE_DIR/state.json`, written
atomically. A restart costs the phone nothing: its next call gets a 401 and it
refreshes. The client table is capped at 64. When it is full, the oldest client
holding no live grant is evicted, because registration is unauthenticated.

**Sign-in.** The passphrase is compared in constant time and must be at least 20
characters, or the gate refuses to start. After 5 failures, every attempt
(right or wrong) is refused for 30 s, doubling per further failure, capped at an
hour. The lockout is global, not per address: an attacker has many addresses,
the owner has one passphrase. The cost is that an attacker can delay the
owner's sign-in. It cannot touch an already-connected phone, which only
refreshes. The lockout resets on restart. Every attempt is logged with the
caller's address.

Claude Code's loopback redirect (`http://localhost:<port>/callback`) is
deliberately **not** accepted. The gate is for the hosted Claude apps, and fleet
agents reach korg over the tailnet directly.

## Environment variables

Set in `/etc/korg-gate/env` (template: `deploy/korg-gate/env.example`).

| Variable | Required | Meaning |
|---|---|---|
| `KORG_GATE_LISTEN` | no (`127.0.0.1:8475`) | Bind address. Keep it on loopback; Funnel terminates TLS |
| `KORG_GATE_PUBLIC_URL` | yes | The connector URL exactly as entered in claude.ai, ending in `/mcp`. It is the OAuth `resource`; its origin is the issuer |
| `KORG_GATE_UPSTREAM` | yes | korg-api's MCP URL, e.g. `https://kubsdb.encke-wahoo.ts.net:5674/mcp`. It is the korg door's upstream |
| `KORG_GATE_UPSTREAM_KLAMS` | no (unset) | klams's MCP URL, `https://kubs0.encke-wahoo.ts.net:7777/mcp`. Setting it opens the klams door at `/klams/mcp`. Unset or empty, the door does not exist |
| `KORG_GATE_PASSPHRASE` | yes | The owner passphrase, 20+ characters. Registered in krot; on the Pi it is rendered into `/etc/khomelab/secrets.env` by k-homelab and reaches the unit through the `korg-gate.service.d/khomelab-secrets.conf` drop-in, so it is **not** in `/etc/korg-gate/env` (see below) |
| `KORG_GATE_STATE_DIR` | yes | Where clients and refresh tokens persist: `/var/lib/korg-gate` under the unit |
| `KORG_GATE_CLIENT_IP_HEADER` | no (`x-forwarded-for`) | Header carrying the caller's address. The *last* entry is used, because that is the one tailscaled appended |
| `KORG_GATE_LOG` | no (`info`) | `tracing` filter |

## Build

```sh
just gate-pi    # aarch64-unknown-linux-gnu release binary, via gcc-aarch64-linux-gnu on kai
```

The binary is `target/aarch64-unknown-linux-gnu/release/korg-gate`. It needs
glibc 2.34 or newer, which Raspberry Pi OS Trixie (2.41) meets. The gate does not
depend on `korg-core`, so the build carries no sqlx or Postgres and no OpenSSL
(rustls throughout).

## Install on the Pi

Prerequisites belong to the ops item (WI 4234) and its k-homelab onboarding
item, not to this page. They are: Raspberry Pi OS Lite 64-bit; fleet onboarding;
the Pi on its **own isolated UniFi network** with internet only; Tailscale up as
`tag:korg-gate`; and the tailnet policy narrowed so `tag:korg-gate` reaches
`kubsdb:5674` and nothing else (korg:4146#comment-4098), plus `kubs0:7777` once
the klams door is open (WI 4294). **Do not enable Funnel until all of that
holds.**

```sh
# from kai
scp target/aarch64-unknown-linux-gnu/release/korg-gate <pi>:/tmp/
scp deploy/korg-gate/korg-gate.service deploy/korg-gate/env.example <pi>:/tmp/

# on the Pi
sudo install -m 0755 /tmp/korg-gate /usr/local/bin/korg-gate
sudo install -d -m 0700 /etc/korg-gate
sudo install -m 0600 /tmp/env.example /etc/korg-gate/env
sudoedit /etc/korg-gate/env       # set KORG_GATE_PUBLIC_URL; leave KORG_GATE_PASSPHRASE out (next step)
sudo install -m 0644 /tmp/korg-gate.service /etc/systemd/system/korg-gate.service
sudo install -d /etc/systemd/system/korg-gate.service.d
sudo install -m 0600 /tmp/khomelab-secrets.conf /etc/systemd/system/korg-gate.service.d/
sudo systemctl daemon-reload
sudo systemctl enable korg-gate     # not started yet: it refuses to start without the passphrase

# on kubs0 (the k-homelab control node): render the passphrase, then start
bin/apply krgate khomelab-secrets --create
ssh krgate sudo systemctl start korg-gate
ssh krgate curl -fsS http://127.0.0.1:8475/healthz       # ok
```

**Where the passphrase lives.** The Pi is a k-homelab host, and krot's rule for
the fleet is one copy of a secret per host, at `/etc/khomelab/secrets.env`,
rendered from k-homelab's age store (`korg-gate-passphrase`). A value in
`/etc/korg-gate/env` would be a second copy outside that file, which the
registration rule treats as a defect. So `/etc/korg-gate/env` carries only the
non-secret variables, and `deploy/korg-gate/korg-gate.service.d/khomelab-secrets.conf`
adds `EnvironmentFile=/etc/khomelab/secrets.env` to the unit. systemd reads
both files as root before dropping to the dynamic user. Decided in sprint 093
(k-homelab sprint 121 carries the manifest half).

The unit runs as a `DynamicUser` with `ProtectSystem=strict`, no capabilities,
and a system-call filter. Its only writable path is `/var/lib/korg-gate`.

## Funnel

```sh
sudo tailscale funnel --bg 8475
tailscale funnel status
```

This publishes `https://<pi-node>.encke-wahoo.ts.net/` → `http://127.0.0.1:8475`.
The connector URL is that origin plus `/mcp`, and it must equal
`KORG_GATE_PUBLIC_URL` exactly. The off switch is
`sudo tailscale funnel --https=443 off`, and it takes effect immediately.

## Connect claude.ai

1. claude.ai → Settings → Connectors → **Add custom connector**.
2. URL: the connector URL (`https://<pi-node>.encke-wahoo.ts.net/mcp`). Leave
   the OAuth client ID and secret empty, so Claude registers itself through DCR.
3. **Connect**. The gate's sign-in page opens and names `claude.ai` as the
   destination. Enter the passphrase.
4. In a chat, ask for something only korg knows ("what's on my korg board?").
   On the phone, do the same in the Claude app, where the connector is already
   present.

The klams door is a **second connector** with URL `<origin>/klams/mcp`. Add it
the same way. The same passphrase signs it in, and its tokens open only
`/klams/mcp`.

## Operator checks

Run these on the Pi after install, and again after any change:

```sh
U=https://<pi-node>.encke-wahoo.ts.net
curl -si $U/mcp -X POST | sed -n '1p;/^www-authenticate/Ip'    # 401 + the resource_metadata pointer
curl -s  $U/.well-known/oauth-protected-resource               # resource == the connector URL, exactly
curl -s  $U/.well-known/oauth-authorization-server             # S256, registration_endpoint, issuer == $U
curl -s  $U/register -H 'content-type: application/json' \
     -d '{"redirect_uris":["https://evil.example/cb"]}'        # invalid_redirect_uri
# with the klams door open:
curl -si $U/klams/mcp -X POST | sed -n '1p;/^www-authenticate/Ip'  # 401 + …/oauth-protected-resource/klams/mcp, scope="klams:read"
curl -s  $U/.well-known/oauth-protected-resource/klams/mcp     # resource == $U/klams/mcp
journalctl -u korg-gate -n 50                                  # sign-ins carry the caller's public address
```

On the first real sign-in, check that the logged `ip` is the phone's public
address and not `127.0.0.1`. If it is loopback, tailscaled is not sending
`X-Forwarded-For` for Funnel traffic. Set `KORG_GATE_CLIENT_IP_HEADER` to
whatever header it does send.

From the Pi, `curl -s --max-time 3 http://<any LAN host>` should **fail**. That
is the UDM network's isolation, and it is what makes a compromised gate a korg
problem rather than a homelab problem.

## Turning the writes on

The ops half of sprint 095, interactive with Ken (proposal korg:4298):

1. `just gate-pi` on kai; `scp` the binary to krgate, `sudo install -m 0755`
   it over `/usr/local/bin/korg-gate`, `sudo systemctl restart korg-gate`.
   The journal's first line names both upstreams
   (`upstreams="korg=… klams=…"`).
2. Operator checks above: korg door, klams door and AS metadata unchanged.
3. In claude.ai → Settings → Connectors → the korg connector, confirm every
   write tool is **Needs approval**. The existing connector picks the tools up
   on its next `tools/list`; no reconnect.
4. From the phone: one real `add_comment` on a work item and one
   `propose_sprint`. Read both back from the tailnet; the comment's `origin` is
   `claude-duet`. `journalctl -u korg-gate` shows a `tools/call` line for each.

## Turning the klams writes on

The ops half of sprint 097, interactive with Ken (korg:4308, claude-duet). In
this order, because each step is safe only once the one before it holds:

1. **klams first: the bound-author rule.** klams WI 4323 (refuse a foreign
   `author_id` on every create tool, fleet-wide) is deployed on kubs0. Without
   it the gate's strip is the only control on attribution.
2. **The identity's scopes.** On kubs0, widen `claude-duet` to
   `read,write,manage`: `sudo klams-token identity scopes claude-duet --add
   write`, then `--add manage` (klams `docs/auth.md`), and reload klams. Until the new gate is installed, the old one still refuses
   every klams write, so this step publishes nothing.
3. **The binary.** As in [Turning the writes on](#turning-the-writes-on), step
   1: `just gate-pi` on kai, install over `/usr/local/bin/korg-gate`, restart.
   Operator checks: klams metadata `resource_name: "klams"`, korg's unchanged,
   both doors 401.
4. **The connector.** In claude.ai → Settings → Connectors → the klams
   connector, let it refresh its tool list (a new chat is enough; no
   reconnect), then confirm the four write tools are **Needs approval**.
5. **From the phone:** one `memory_add` with `kind: "knowledge"`, one
   `memory_add` with `kind: "fact"` (refused, "knowledge only"), and a
   `memory_supersede` of the first. Read them back over the tailnet: the
   author is `claude-duet`. `journalctl -u korg-gate` shows a `tools/call …
   door="klams"` line for each, and a `tools/call arguments refused` warning
   for the fact. Clean up with `memory_delete`.

## Rotating the passphrase

It is a k-homelab secret: `bin/secret set korg-gate-passphrase` on kubs0, then
`bin/apply krgate khomelab-secrets` and `sudo systemctl restart korg-gate` on
the Pi (the `krot-rotate` skill walks it; krot's entry is
`korg-gate-passphrase`). Connected clients keep working, because a
refresh never asks for the passphrase. To disconnect every client as well, stop
the unit, delete `/var/lib/korg-gate/state.json`, and start it again.

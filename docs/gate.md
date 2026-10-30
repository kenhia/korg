# korg-gate — the read-only public front door

`korg-gate` lets **Claude Duet** (and claude.ai on the web, Desktop and mobile)
read korg. Those clients reach tools only through *custom connectors*, which
must be public HTTPS. korg is tailnet-only and unauthenticated by design, so
something has to stand between the two. That is this crate (`crates/korg-gate`,
one binary).

- **Decision:** WI 4146, a scoped exception to the 2026-07-11 "nothing to the
  public internet" decision. **Build:** WI 4147. **Stand-up:** WI 4234.
- **korg-api does not change.** The gate is a client of korg-api's `/mcp`, the
  same as any fleet agent.
- **It does not ship with the kubsdb image.** It runs on its own Pi, on an
  isolated network, behind Tailscale Funnel. `just deploy`/`.sprint-deploy` never
  touch it.

## What it does

| Path | Purpose |
|---|---|
| `/.well-known/oauth-protected-resource` (and `…/mcp`) | RFC 9728 metadata: `resource` is the connector URL, `authorization_servers` is the gate itself |
| `/.well-known/oauth-authorization-server` | RFC 8414 metadata: DCR, S256 PKCE, public clients, `korg:read` + `offline_access` |
| `POST /register` | RFC 7591 dynamic client registration. Only `https://claude.ai/api/mcp/auth_callback` may be registered |
| `GET`/`POST /authorize` | The sign-in page: one owner passphrase, no accounts |
| `POST /token` | Code + PKCE exchange, and refresh with rotation. A dead refresh token gets `invalid_grant` |
| `POST`/`GET`/`DELETE /mcp` | The proxied MCP endpoint. Without a live access token: `401` + `WWW-Authenticate: Bearer resource_metadata="…"` |
| `GET /healthz` | `ok` |

**Read-only, deny by default.** Only the MCP lifecycle methods (`initialize`,
`notifications/initialized`, `ping`, `tools/list`, `tools/call`) pass. A
`tools/call` passes only for a tool named in `READ_TOOLS`
(`crates/korg-gate/src/allowlist.rs`), and `tools/list` is filtered to the same
list. A refused request is a JSON-RPC error; it never reaches korg. A
`tools/list` answer the gate cannot parse is refused, never passed through
unfiltered. Batches are refused.

The allowlist is explicit names, not a prefix rule. The `allowlist` test suite
reads korg's real catalogue (`crates/korg-mcp/tests/tools_schema.json`). It fails
when korg adds a read-shaped tool (`get_*`, `list_*`) the list has not ruled on,
and when the list names a tool korg no longer serves. **Adding a tool to
`READ_TOOLS` publishes it to the internet.** Do that on purpose. Write access is
a separate, later decision.

**Forwarded requests** carry `X-Homelab-Agent: claude-duet`, so korg's logs can
tell Duet traffic apart. The gate's own bearer token is never forwarded.
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
| `KORG_GATE_UPSTREAM` | yes | korg-api's MCP URL, e.g. `https://kubsdb.encke-wahoo.ts.net:5674/mcp` |
| `KORG_GATE_PASSPHRASE` | yes | The owner passphrase, 20+ characters. Held in krot, never in the repo |
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
`kubsdb:5674` and nothing else (korg:4146#comment-4098). **Do not enable Funnel
until all of that holds.**

```sh
# from kai
scp target/aarch64-unknown-linux-gnu/release/korg-gate <pi>:/tmp/
scp deploy/korg-gate/korg-gate.service deploy/korg-gate/env.example <pi>:/tmp/

# on the Pi
sudo install -m 0755 /tmp/korg-gate /usr/local/bin/korg-gate
sudo install -d -m 0700 /etc/korg-gate
sudo install -m 0600 /tmp/env.example /etc/korg-gate/env
sudoedit /etc/korg-gate/env       # set KORG_GATE_PUBLIC_URL and the passphrase (from krot)
sudo install -m 0644 /tmp/korg-gate.service /etc/systemd/system/korg-gate.service
sudo systemctl daemon-reload
sudo systemctl enable --now korg-gate
curl -fsS http://127.0.0.1:8475/healthz       # ok
```

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

## Operator checks

Run these on the Pi after install, and again after any change:

```sh
U=https://<pi-node>.encke-wahoo.ts.net
curl -si $U/mcp -X POST | sed -n '1p;/^www-authenticate/Ip'    # 401 + the resource_metadata pointer
curl -s  $U/.well-known/oauth-protected-resource               # resource == the connector URL, exactly
curl -s  $U/.well-known/oauth-authorization-server             # S256, registration_endpoint, issuer == $U
curl -s  $U/register -H 'content-type: application/json' \
     -d '{"redirect_uris":["https://evil.example/cb"]}'        # invalid_redirect_uri
journalctl -u korg-gate -n 50                                  # sign-ins carry the caller's public address
```

On the first real sign-in, check that the logged `ip` is the phone's public
address and not `127.0.0.1`. If it is loopback, tailscaled is not sending
`X-Forwarded-For` for Funnel traffic. Set `KORG_GATE_CLIENT_IP_HEADER` to
whatever header it does send.

From the Pi, `curl -s --max-time 3 http://<any LAN host>` should **fail**. That
is the UDM network's isolation, and it is what makes a compromised gate a korg
problem rather than a homelab problem.

## Rotating the passphrase

Change it in krot, update `/etc/korg-gate/env`, then run
`sudo systemctl restart korg-gate`. Connected clients keep working, because a
refresh never asks for the passphrase. To disconnect every client as well, stop
the unit, delete `/var/lib/korg-gate/state.json`, and start it again.

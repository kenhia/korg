# 093 — korg-gate on the Pi: the Duet front door, ops half

Proposal korg:4148, covering **WI 4234** (stand the gate up on the Pi and
connect claude.ai). Its dependency, k-homelab **WI 4235** (onboard the Pi as a
fleet host), ships through k-homelab's own sprint flow (sprint 121) and is
cross-referenced here. Interactive session on cleo with Ken, 2026-10-30; every
step needs his hands or an admin console. Runbook: `docs/gate.md`.

## Goal

Sprint 092 built the gate and opened no ingress. This sprint puts it on a
dedicated Pi on an isolated UniFi network, joins that Pi to the tailnet as
`tag:korg-gate` under a narrowed policy, enables Funnel on that one node, adds
the connector in claude.ai, and verifies every Done line on WI 4234.

The order is a safety order, not a preference: policy first, isolated network
second, only then the tailnet join, and Funnel last.

## Premise check

- **WI 4234 holds.** `crates/korg-gate`, `deploy/korg-gate/`, `docs/gate.md`
  and `just gate-pi` are on main (b73c141, PR #98). The aarch64 binary from the
  092 build is still in kai's target dir (7.2 MB, 2026-10-29 22:11); it is
  rebuilt from main before install anyway.
- **Tailnet state (from kai, `tailscale status --json`):** 14 nodes. The only
  tag in use is `tag:server` (kai, kubsdb, kubs0, gratch, ksandbox, rpi53,
  rpidash2, rpidash3). No `tag:korg-gate`, no Funnel anywhere. Neither spare Pi
  is on the tailnet.
- **korg:4146#comment-4098 drifted, in a way that matters.** It says to change
  the catch-all source from `*` to `autogroup:member` "so every user-owned
  device keeps working". That leaves out the tagged hosts: every always-on
  server carries `tag:server`, and server-to-server traffic (kai → kubsdb for
  korg and klams, kaed gateway peers, kmon's ssh fan-out from kai, kubs0's
  audits over tailnet names) is tagged-to-tagged. The narrowed rule's source is
  therefore `autogroup:member` **and** `tag:server`.
- Cross-project plan: korg routes to `korg+/`. GP-17 already records the gate
  as its one scoped exception (cross-project-planning 7e83eed, sprint 092).
  Nothing in this sprint touches a GP.

## Log

### 2026-10-30 09:30 PDT — the Pi is imaged; nothing joined yet

Ken chose the OctoPrint board and imaged it himself before the session
reached step 3. Preflight over ssh from cleo (`ssh ken@krgate`):

- Raspberry Pi 4 Model B Rev 1.4, 8 GB (7.6 GiB visible), 59 GB SD card,
  2 GB zram swap. `throttled=0x0` on a CanaKit 5.1 V/3.5 A supply, so the
  2026-10-20 under-voltage is gone with the old supply. A Waveshare PoE HAT
  (C) may replace it later; nothing in the install depends on which.
- Raspberry Pi OS Lite, Debian 13 trixie, kernel `6.18.50+rpt-rpi-v8`
  (the Pi 4 flavour), glibc 2.41, python3 and curl present, 633 packages, 0
  upgradable. `unattended-upgrades` not installed (the Pi images never ship
  it). No tailscale.
- Hostname `krgate`, user `ken`, `/bin/bash`, `sudo -n` works, timezone
  America/Los_Angeles, timesyncd running.
- `~/.ssh/authorized_keys` is kai's fleet set copied over: kwork, kenhi@cleo,
  cleo-wsl, ken@kubs0, ken@kai, ken@komarchy, ken@kimac, ken@kbook2,
  ken@thrall. So the control node already reaches it (Q1 of k-homelab's
  four questions).
- **On the main LAN**: eth0 192.168.1.220/24 by DHCP from the UDM, DNS
  192.168.1.1. wlan0 down, NetworkManager, wpa_supplicant, bluetooth and
  avahi-daemon running. Only listener: sshd on :22.

So the Pi is on the home LAN with no Tailscale, which is the allowed state
before steps 1 and 2. The sequencing holds: policy, then the isolated UniFi
network, then `tailscale up --advertise-tags=tag:korg-gate`.

One reach fact for the k-homelab side: once the Pi sits on the isolated VLAN
the control node can only reach it over the tailnet. kubs0 has MagicDNS on
(`search encke-wahoo.ts.net localdomain`, `getent hosts rpidash3` answers the
100.x address), so `address: krgate` in `inventory.yml` resolves to the
tailnet node, not the unreachable VLAN address.

### Step 1 — tailnet policy narrowed (Ken, admin console, ~09:50 PDT)

The policy was the Tailscale default (`grants` form, `src *` → `dst *`,
`tagOwners` holding only `tag:server`). Ken pasted it here, took back the
edited file, and saved it. Now:

- `hosts.kubsdb = 100.90.99.84`, so one grant can name one host.
- `tagOwners`: `tag:server` and `tag:korg-gate`, both `autogroup:admin`.
- grant 1: `src [autogroup:member, tag:server]` → `dst *`, `ip *`. The
  `tag:server` half is the premise-check correction above.
- grant 2: `src [tag:korg-gate]` → `dst [kubsdb]`, `ip [tcp:5674]`.
- `nodeAttrs`: `funnel` on `tag:korg-gate` only.
- `ssh` block unchanged; two `tests` run on every save (the gate reaches
  kubsdb:5674 and nothing else; a server still reaches kubsdb:5674, kubsdb:22
  and kai:22).

Verified from the fleet afterwards: kai → korg 200, kai → kubs0/kubsdb/gratch
ssh, kai → rpi53:6379 open; kubs0 → kai ssh and korg 200; cleo → korg 200;
kaed's fleet probe reports both peers `ok`. Nothing lost reach.

### Step 2 — the isolated UniFi network (Ken, UDM-Pro)

Port chosen: **port 20 on the first USW 24 PoE (192.168.1.249)**, so the
Waveshare PoE HAT works there this weekend. unpoller's metrics show link on
ports 1, 7, 11, 13–16, 21, 23, 24 and SFP 2 across the two PoE switches (they
share a name, so the series collide); 20 is free on both. Suggested network:
`korg-gate`, VLAN 40, isolation on, internet on, mDNS/IGMP off.

**What Ken built (his write-up: `D:\ClaudeWorks\korg-gate-vlan-setup.md` on
cleo).** Network `korg-gate`, VLAN 40, 192.168.2.0/24 (gateway .1, DHCP
.6–.254), Isolate Network on, internet on, IPv6 none; mDNS and IGMP are
global in UniFi 10.6 and were left alone. **Port 2, not 20**: only ports
1–16 on these switches carry PoE, so the Pi sits on port 2 of the .249
switch, a PoE+ edge port named `Port 2 - korg-gate Pi`, native `korg-gate
(40)`, tagged VLANs blocked. unpoller confirms link at 1 Gbps on that port
under that name.

While Ken studied the UniFi app, with his say-so, the `tailscale` package
(1.104.4) went onto krgate from the official trixie repo, the way rpidash3
has it. tailscaled is enabled and active and **logged out** (`NeedsLogin`,
no TailscaleIPs): no node exists. The join itself waits for the move.

### Step 4 (k-homelab, WI 4235) ran early, while the Pi was still reachable

Nothing in the onboarding depends on the VLAN, so it landed during Ken's
UniFi work: k-homelab proposal korg:4241, branch `121-onboard-krgate`,
manifest with `authorized-keys`, `unattended-upgrades` and `disabled-units`,
the fleet, ring, DR, changelog, docs, port-registry and backup-coverage
entries, `just check` green, and both recipes applied from kubs0 (audit all
`ok`). Wi-Fi off and sshd key-only by hand, recorded in its `dr/krgate.md`.
korg's project row now has `deploy_to: [kubsdb, krgate]`. Details are in
k-homelab `sprints/121-onboard-krgate.md`.

### Step 3 — the join, from a keyboard (~10:40 PDT)

Once the Pi sat on the VLAN (lease 192.168.2.208, seen in the UniFi client
list), neither cleo nor kubs0 could open a connection to it: UniFi's Isolate
Network blocks both directions. So Ken ran
`sudo tailscale up --advertise-tags=tag:korg-gate --ssh=false` at the Pi and
approved the URL. Node `krgate.encke-wahoo.ts.net`, 100.89.98.71; `tailscale
whois` from kai shows `Tags: tag:korg-gate`; kubs0 resolves `krgate` to the
tailnet address and `ssh krgate` works.

**The fence, probed from the Pi** (6 s timeouts): reachable, kubsdb:5674
(`/plan` 200) and the internet. Blocked on the tailnet: kubsdb:22/5432/9090,
kai:22/4870, kubs0:22/4870, rpi53:6379, gratch:22, cleo:22, ping kai. Blocked
on 192.168.1.0/24: kubsdb .60:5674, kai .111:22, rpi53 .79:6379, cleo .77:22,
the .249 switch, ping kubs0 .235. **Reachable, and a finding:** the UDM's own
192.168.1.1 answers ping and has 22, 53, 443, 8080 and 8443 open to the Pi,
and its admin UI answers on 192.168.2.1 as well. Isolate Network covers other
networks, not the gateway's own interfaces. Raised with Ken: a firewall policy
blocking `korg-gate` → the gateway (keeping DNS and DHCP on 192.168.2.1)
closes it, and it should be in place before Funnel.

### Step 5 — passphrase and install (~10:50 PDT)

**Where the passphrase lives changed from the 092 runbook.** `docs/gate.md`
said to put it in `/etc/korg-gate/env`. krot's registration rule is one copy
of a secret per host, at `/etc/khomelab/secrets.env`, rendered by k-homelab;
a value anywhere else is a defect to file. So: `manifests/krgate.yml` declares
`khomelab-secrets` with `KORG_GATE_PASSPHRASE: korg-gate-passphrase`, the
unit reads that file through a new drop-in
`deploy/korg-gate/korg-gate.service.d/khomelab-secrets.conf`
(`EnvironmentFile=/etc/khomelab/secrets.env`; systemd reads it as root before
the DynamicUser drop), and `/etc/korg-gate/env` carries only the non-secret
variables. `docs/gate.md` now says so (install, env table, rotation).

- Minted on kubs0 as `prefixed(korg-gate-, base64url(43))`, 53 characters,
  piped straight into `bin/secret set korg-gate-passphrase`; the transcript
  holds only the fingerprint, `3ab00e772e3f`. `secrets/index.yml` documents
  it with a probe (`bin/secret verify`: ok, control refused; the first draft
  raced `ssh` for stdin and reported STALE against a correct render, fixed
  with `V=$(cat)` first and `ssh -n`).
- krot: `registry/korg-gate.toml` (service, procedure, secret, `hosts.krgate`,
  authority `env` probe). `krot scan korg-gate-passphrase` clean on the
  authority and the copy. `check --manifests` and the registry test both
  refused the row until `krgate` was added to `POSIX_HOSTS` in
  `crates/krot-core/src/model.rs` — repaired in passing in krot, as the
  error message asked.
- Install from kai over the tailnet (host key fingerprint verified first):
  binary sha256 `0e414140f0df…` (rebuilt from main), `/etc/korg-gate/env`
  0600 root, the unit, the drop-in; `systemctl enable`. Then
  `bin/apply krgate khomelab-secrets --create` from kubs0 (group, helper,
  file `root:khomelab 0640`; on-host fingerprint of the rendered value
  `3ab00e772e3f`), `systemctl start korg-gate`: active, `healthz` ok,
  listening `127.0.0.1:8475`, log line `korg-gate starting …
  resource=https://krgate.encke-wahoo.ts.net/mcp
  upstream=https://kubsdb.encke-wahoo.ts.net:5674/mcp`.
- Operator checks on loopback, all as documented: 401 with the
  `resource_metadata` pointer; protected-resource `resource` is the connector
  URL exactly; issuer is the origin, S256, `registration_endpoint`; an evil
  redirect gets `invalid_redirect_uri` and a WARN in the journal.
- k-homelab `bin/audit krgate`: four rows `ok`. `just check` green in both
  repos.

### The gateway gap, closed (Ken, UDM, ~11:20 PDT)

The UDM's legacy firewall (the console offered the zone-based upgrade; declined
mid-change) took three **LAN Local** rules, source network `korg-gate`, Before
Predefined: allow DNS (TCP/UDP 53), allow DHCP (UDP 67–68), drop the rest
(logged). Re-probed from the Pi: 192.168.1.1 and 192.168.2.1 answer only on
53; DNS direct and via MagicDNS, korg on kubsdb:5674 and the internet still
work. k-homelab `dr/krgate.md` carries the rules. With that, every
precondition in `docs/gate.md` held.

### Step 6 — Funnel on (~11:40 PDT, on Ken's go)

`sudo tailscale funnel --bg 8475` on the Pi. `tailscale funnel status` and
`tailscale serve status` both show exactly one mapping,
`https://krgate.encke-wahoo.ts.net` → `http://127.0.0.1:8475`. Let's Encrypt
certificate for the name, expiring 2027-01-28. From kai: `/healthz` ok, the
evil-redirect registration refused. The homelab's first public endpoint, as
decided on WI 4146. Off switch: `tailscale funnel --https=443 off`.

Checked through the public relay from cleo (`--resolve` to 208.111.34.11,
bypassing the tailnet): `/healthz` ok, the 401 shape, `/authorize` 400
without parameters, the evil redirect refused.

### Step 6 — connector and first use (Ken, ~11:55–12:05 PDT)

- claude.ai Settings → Connectors → Add custom connector, URL
  `https://krgate.encke-wahoo.ts.net/mcp`; claude.ai detected "Sign in now"
  and "Register automatically (DCR)". Connect, passphrase from `bin/secret
  get` in Ken's own terminal. Journal: `client registered … name=Some("Claude")`,
  `sign-in succeeded ip=100.94.70.96`. That address is cleo's tailnet IP: the
  browser on cleo resolves krgate via MagicDNS and signed in over the tailnet.
- **The Funnel address check, done directly** rather than waiting for a
  phone sign-in: a throwaway DCR client registered through the relay and one
  deliberately wrong passphrase. Journal: `sign-in failed ip=73.42.247.157`,
  which is cleo's public IP. tailscaled sets `X-Forwarded-For` on Funnel
  traffic; `KORG_GATE_CLIENT_IP_HEADER` stays at its default. One failure is
  on the lockout counter.
- claude.ai lists 26 tools for the connector, all reads: `tools/list` served
  through the gate. Its default per-tool setting is "Needs approval"; Ken set
  the group to Always allow, since the gate is read-only server-side and Duet
  cannot answer approval prompts.
- claude.ai sent `server/discover` three times; the gate refused it (not in
  `METHODS`). Filed as korg:4243 rather than added.
- First read from the phone: journal `tools/call … tool="get_project"`, and
  Claude answered. korg-api does not log `X-Homelab-Agent` (korg:4242), so the
  stamp was proven on the wire: a tcpdump of kubsdb's loopback hop (tailscale
  serve → 127.0.0.1:5674) caught exactly four requests with
  `X-Homelab-Agent: claude-duet`: `initialize`, `notifications/initialized`,
  `tools/list`, `tools/call`. Capture deleted afterwards.
- The write test, from the connector side (12:08 PDT): asked to add a
  comment on WI 4234, Duet answered that the connector gives it no write
  tools at all, so there was nothing to call; no `tools/call` reached the
  gate. The filtered `tools/list` is the first layer. The second, a write
  forced through `tools/call`, is refused with -32602 before korg, as 092's
  live smoke and `tests/allowlist.rs` already proved on this binary.

## What shipped (this branch)

- `deploy/korg-gate/korg-gate.service.d/khomelab-secrets.conf`: the drop-in
  that reads the passphrase from the fleet's per-host secrets file.
- `docs/gate.md`: the install sequence (enable, render, start), the env
  table's `KORG_GATE_PASSPHRASE` row, a "Where the passphrase lives"
  paragraph, and the rotation section, all moved to the per-host file.
- This record.

Everything else this sprint produced lives elsewhere: the host in k-homelab
(sprint 121, proposal korg:4241), the secret in krot (`registry/korg-gate.toml`,
krot `1ce4497` on main) and the k-homelab age store, the tailnet policy in the
admin console, the network and firewall rules on the UDM, and the connector in
claude.ai.

## Verification

- Every Done line on WI 4234, with its evidence in the item's thread
  (comments 4192–4205).
- `just check` on this branch: run at ship. The only code-adjacent change is
  a doc, and the `docs_drift` env-table test reads `docs/gate.md`.

## Repaired in passing

- krot: `krgate` added to `POSIX_HOSTS` (`crates/krot-core/src/model.rs`), as
  krot's own registry gate asked for by name. Landed with the registration.
- k-homelab: the first draft of `secrets/index.yml`'s verify probe raced `ssh`
  for stdin; fixed before it was committed.

## Follow-ups

- korg:4242 — korg-api does not log `X-Homelab-Agent`; the stamp had to be
  proven with a packet capture. One INFO line per `/mcp` request would make
  "is Duet reading korg" answerable from kubsdb's log.
- korg:4243 — claude.ai sends `server/discover`, which the gate refuses.
  Decide whether to forward it.
- Not filed, recorded: the UDM offered its zone-based firewall upgrade and
  Ken declined it mid-change. If it is ever taken, the three LAN Local rules
  in k-homelab `dr/krgate.md` become one policy pair.
- Known limit, unchanged from the proposal: Duet's scheduled tasks reportedly
  cannot use custom connectors, so this is chat-only use.

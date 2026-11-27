# 103 — the phone may tag calendar days; operations.md catches up with the restore drill

Proposal korg:5108, slice 4 of the 2026-11-27 low-hanging-fruit program
korg:5112. It covers WI 4676 and rides along the korg half of k-homelab
WI 4589's mechanical doc lines. Ran as a karc leg on kai (`korg-007e7d`),
under oversight.

## Premise check

- **WI 4676 holds.** `add_tagged_day` and `remove_tagged_day` were in
  `REFUSED_TOOLS`; `list_tagged_days` in `READ_TOOLS`; the fence pinned nine
  writes and 36 listed tools. Ken ruled yes on 2026-11-27 (comment 5209).
- **WI 4589's korg lines hold.** `docs/operations.md` still said the image
  store "has no second copy yet" and gave 2026-07-08 as the last verified
  restore. Live on kubsdb (probed from kai, 2026-11-27): `korg-images-sync.timer`
  fires nightly at 03:47 UTC, last run `Result=success` 03:51 UTC today, and
  `/datastore/korg/images` and `/gratch/backups/korg/images` are both
  49,224,430 bytes. The oldest journal line is 2026-09-27 (journal retention),
  so "since August" is not checkable from the host; the doc does not claim a
  start date. Schedule 1110: last drill WI 4587, next due 2027-02-13.

## What shipped

- `crates/korg-gate/src/allowlist.rs`: the two tagged-day writes move from
  `REFUSED_TOOLS` to `WRITE_TOOLS`, whole — no `LIMITED_TOOLS` entry. Neither
  carries `origin`, so `ORIGIN_TOOLS` is unchanged.
- `crates/korg-gate/tests/allowlist.rs`: `the_write_set_is_exactly_the_ruled_nine`
  becomes `…_ruled_eleven`; the `tools/list` count goes 36 → 38.
- `docs/gate.md`: the write table gains the pair; the refused paragraph says
  when and by whom they moved.
- `docs/operations.md`: "Backups" now describes the image copy (what, where,
  when, script, owner) and the half-hour skew between the dump and the sync;
  "Is the backup current?" checks both timers; "Restore" says to put the
  store back from the copy first; "Restore drills" cites the 2026-11-13 drill.

## Cross-repo changes made

- **cross-project-planning `38f3f3d`** — korg+ GP-17 gains the 2026-11-27
  paragraph. Its README's procedure: commit straight to main, push. Pushed;
  `main...origin/main`.
- **claude-duet `f9bb2b1`** — `docs/rulings.md` gains the 2026-11-27 entry,
  the same way sprint 099's ruling landed (`1d48e2f`). `just check` green,
  pushed; `main...origin/main`.

## Verification

- `cargo test -p korg-gate` green.
- **Loopback smoke against live korg, no sign-in on the live gate.** The x86
  build of this branch ran on kai at `127.0.0.1:18475` with a `mktemp -d`
  state dir and a test passphrase, upstream the live korg-api
  (`.scratch/103/smoke103.py`): `tools/list` 38 with all three tagged-day
  tools; `list_tagged_days` read; `add_tagged_day 1999-01-04 off` → id 15;
  listed; `remove_tagged_day id 15` → removed; listed again, gone. Negative
  controls `update_proposal` and `create_card` refused `-32602`. Torn down:
  no process, port closed, state removed.

## Deployed

Merged as `9702a10` (PR #113; CI green on the PR: rust 10m1s, web 21s). This
was a gate-only deploy to **krgate** at 2026-11-27 12:20 PST, under the
overseer's clearance (korg:5108 comment 5235).

**The declared `deploy-kubsdb` step was skipped on purpose**, by the
overseer's ruling. korg-api does not depend on korg-gate (`cargo tree -p
korg-api` shows no korg-gate edge), and nothing server-side changed. So the
korg service was neither rebuilt nor restarted, and the sibling legs were not
disturbed.

1. **Built from merged main** on kai with `just gate-pi`: aarch64 sha256
   `7c73c277…`.
2. **Verified the exact build before installing it, with no sign-in on the
   live gate.** It ran as a throwaway on krgate on `127.0.0.1:18475`, with a
   `mktemp -d` state dir, a test passphrase, and the live gate's own
   upstream. `smoke103.py` gave the same results as on kai:
   - `tools/list` returned 38 tools, including all three tagged-day tools.
   - `list_tagged_days` read the list.
   - `add_tagged_day 1999-01-04 off` created id 16. It was listed, then
     removed with `remove_tagged_day`, and was gone afterwards.
   - **Negative control:** `update_proposal` and `create_card` were refused
     with `-32602`.

   Teardown: no process, port closed, state and binary deleted.
3. **Installed** with `sudo install -m 0755` and `systemctl restart korg-gate`.
   There was no env or unit change. The service is `active`, and
   `/usr/local/bin/korg-gate` hashes to `7c73c277…`. The previous binary,
   `7f01d365…` (sprint 099), is the rollback target, and a copy is at
   `/tmp/korg-gate-prev` on krgate. The journal's start line names both
   upstreams.
4. **Live checks needing no sign-in**, from kai over the public path:
   - `GET /healthz` returned 200.
   - `POST /mcp` and `POST /klams/mcp` returned 401, and the korg one points
     at its resource metadata.
   - korg metadata still reads `resource_name: korg` and `korg:read`, and
     klams reads `klams` and `klams:read`.

**Still pending: Ken's check from claude.ai.** Confirm the korg connector
lists `add_tagged_day` and `remove_tagged_day`, and that both are set to
**Needs approval**.

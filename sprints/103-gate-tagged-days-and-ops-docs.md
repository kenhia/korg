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

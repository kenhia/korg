# 100 — korg-pulse: publish `kdash:korg:pulse` for kpidash

Proposal korg:4616, slice 3 of program korg:4619 (kpidash row 2). It covers
WI 4611. The program needs the korg half of kpidash's korg + klams card, which
needs a record on central Redis. The contract is kdashdata's
`kdash-korg-pulse` schema (CD-28) at kdashdata main d57e1dc, shipped by the
contracts slice korg:4615. This sprint ran as a karc leg on kai
(`korg-0cc987`), under oversight. Comment 4657 on the proposal carried the
contract facts forward.

## Premise check

- **WI 4611 holds, and its one design point was already settled.** The item
  left the history series open: derive it from korg's tables if they can
  answer it, else keep a series. They can. `work_item_flow` (#1318) returns
  the end-of-day `backlog` per day, reconstructed from the transition log.
  The contract, written after the item, says to copy exactly that column. On
  2026-11-14 the flow returned 30 days (horizon 2026-08-08), and today's
  `backlog` (31) equalled the sum of the board's `depth[].wi_total` (31).
- **kubsdb has no `kdash-pub`** (`/usr/local/bin` there holds knarr, kpkg and
  the backup scripts, checked over ssh from kai). kai has it and authenticates
  to central (`kdash-pub check`: PING answered). That decided the host, below.

## Decisions

- **A korg reader, not a korg feature.** The publisher calls korg's public
  REST reads, `/api/board` and `/api/work-items/flow?days=30`, and writes with
  `kdash-pub set`. korg-api, the image and the compose file do not change, and
  korg gains no Redis dependency (kdashdata CD-26).
- **Python stdlib, not a workspace crate.** k-homelab's `repo-user-timer`
  recipe installs units from a checkout and runs a runner from it. It builds
  nothing. A Rust binary would need either a build step on the host or a
  package-store artifact plus a knarr deploy. A stdlib script runs from the
  checkout as-is, the way kweather and the kfdc curator do.
- **Validated by the contract's own validator.** `pulse/vendor/` holds
  kdashdata's schema and its stdlib `jsonschema_mini.py`, byte for byte at
  d57e1dc. A test pins their digests. The validator refuses keywords it does
  not implement, and a test runs the schema's own examples and counterexamples
  through it, so the suite cannot pass on a validator that accepts everything.
- **Every figure is copied.** `open` is the flow's last `backlog`, `history`
  is the whole column (oldest first, newest 30), and `by_project`, `projects`,
  `awaiting`, `active` and `queued` come from the board. `queued` counts only
  `proposed` queue rows, because parked rows ride at the end of `queue`.
  `other_open` is `open` minus what `by_project` lists. That is the cap's
  overflow plus unassigned items, which `depth` cannot show.
- **A torn read is refused.** Two requests can straddle a write. If the
  board's per-project sum exceeds the flow's backlog, the record would break
  the contract's sum rule. The publisher re-reads once, then exits 1 and
  writes nothing, so the old record goes stale rather than wrong.
- **Five minutes, `expected_refresh_s: 300`.** The WI said a few minutes is
  plenty. The contract's 60 s was a suggestion, made for the board read in
  CD-26. The timer and `DEFAULT_REFRESH_S` carry the same number, each with a
  comment pointing at the other.
- **kai, as a user timer.** The units are in `systemd/` (the recipe's default
  subdir), and the runner is `bin/korg-pulse`. kai already runs kweather and
  the kfdc curator this way, and it holds the korg checkout the recipe stages
  from. kubsdb would first need kdash-pub deployed there, for no gain.

## What shipped

- `pulse/korg_pulse.py`: a pure `build_record(board, flow, ts, refresh_s)`,
  plus the read, the retry and the `kdash-pub` write. `--dry-run` prints the
  record instead.
- `pulse/test_korg_pulse.py`: 11 tests on a synthetic fixture board with 27
  holding projects (exercising the 24 cap and ties), unassigned items and a
  parked proposal. They cover the 30-day history cap, an empty korg, a torn
  read, the retry, and the exact `kdash-pub` argv.
- `pulse/vendor/`: the pinned schema and validator, with a README on
  re-pinning.
- `bin/korg-pulse` and `systemd/korg-pulse.{service,timer}`.
- `just pulse-check` (now part of `just check`), `just pulse-dry`, and a CI
  step running the same suite.
- docs/operations.md § korg-pulse, and a README pointer.

## Verification

- `just check` is green, including `pulse-check` (11 tests).
- **First publish fired by trigger, not by timer.** On 2026-11-13 at
  17:15:04 PST, a transient user unit ran the service's own ExecStart, PATH
  and working directory (`systemd-run --user --wait --collect`). The
  result was `success`, logging
  `open=31 projects=18 awaiting=1 active=2 queued=2 history=30`.
- **The live key matches the board.** `kdash-pub get kdash:korg:pulse`, read
  7 s after the write, passes the vendored validator with no errors. It was
  compared with reads taken in the same second: REST `/api/board` and `/flow`,
  and MCP `get_board` (`generated` 01:15:06Z). open 31 = 31, projects 18 = 18,
  awaiting 1 = 1, active 2 = 2, queued 2 = 2, `by_project` equal entry for
  entry, `history` equal day for day (2026-10-15 130 → 2026-11-13 31), and
  `other_open` 0.

## Follow-ups

- **k-homelab installs the timer.** One `repo_user_timer:` block in
  `manifests/kai.yml`, a consumer page, and `bin/apply kai repo-user-timer`.
  This waits for the merge, because the recipe stages from the kai checkout's
  `systemd/`. The exact text is in the wrap-up handoff on korg:4616.
- The timer runs whatever the kai checkout has checked out. During a korg
  sprint on kai, that is the sprint branch. This is the same arrangement as
  every other `repo-user-timer` consumer, and is noted rather than changed.

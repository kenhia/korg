# 101 — tagged days: storage, MCP tools, computed holidays, `kdash:calendar`

Proposal korg:4667, slice 2 of program korg:4669 (kpidash's time and calendar
card). It covers WI 4664. The contract is kdashdata's `kdash-calendar` schema
(CD-30), shipped by slice 1 at kdashdata main `b1ad6af`. Handoff korg:4671
spells it out. This sprint ran as a karc leg on kai (`korg-4191a8`), under
oversight.

## Premise check

- **WI 4664 holds.** korg had no table, tool or record for day tags or
  holidays. Nothing open in korg covered them, and `kdash:calendar` read
  back `is not set` from central (`kdash-pub get`, kai).
- **The contract moved since the WI, and the launch brief settles each
  change.** There is no `v` field, `expected_refresh_s` is required (3600) and
  `off` is required on every entry. `off` means a day Ken tagged off and
  nothing else, so a holiday is `off: false` unless he also tagged it. Names
  are at most 31 characters and `short` at most 12. All dates are
  America/Los_Angeles. There are at most 64 entries, and the window runs from
  the first of the month to at least 90 days ahead.

## Decisions

- **A table, not a node kind** (migration 0039, `tagged_day`). A tag has no
  comments, no edges and no lifecycle. That is 0025 §3's argument for report
  sources and 0037's for machines. Removing a tag is a DELETE.
- **One row per tag.** A date that is both a day off and a birthday is two
  rows, and the publisher folds them into the feed's one entry. `yearly`
  repeats a row's month and day from its own year on, so a birthday is
  entered once. A 29 February falls back to the 28th in a common year. The
  CHECKs mirror the contract's bounds, so no stored row can produce an entry
  the panel's reader would skip. An `off` row carries no label or short.
- **Three MCP tools, one call each.** `add_tagged_day` returns `conflict` for
  a duplicate, naming the id. `remove_tagged_day` takes an `id`, or a `date`
  (which reaches a yearly row through its month and day), optionally narrowed
  by `kind`. Removing nothing is `not_found`, never a silent success.
  `list_tagged_days` is a bare array. A new vocabulary, `TAGGED_DAY_KINDS`
  (`off`, `special`), feeds the enums. There is a new catalogue category,
  "Tagged days". REST gets only `GET /api/tagged-days`, the publisher's read.
  Writes are MCP-only, because the slice ships no UI.
- **Holidays are computed in the publisher, not stored or served by korg.**
  `calfeed/us_holidays.py` covers the eleven federal holidays by rule. A
  fixed-date one that falls on a weekend gets a `<Name> (observed)` entry on
  the Friday or Monday, including New Year's crossing back into 31 December.
  Juneteenth starts in 2021. Names are the common ones that fit ("Presidents'
  Day", "Juneteenth"). Shorts are at most 12 characters, and observed ones read
  "July 4th obs", "Xmas obs".
- **Folding a date.** A special day's stored `short` wins. Otherwise the
  holiday's short is used, and a date that is only off has none. Two holidays
  or two specials are joined with " / " and cut to 31 characters with "…".
- **The window** is from the first of the LA month to LA today + 92 days. The
  extra two days keep "≥ 90 days after `ts`'s date" true for a reader that
  dates `ts` in UTC, which can be a day ahead in the evening.
- **Hourly and on change, from a single per-minute timer.** korg runs on
  kubsdb and cannot poke kai, and korg gains no Redis dependency (CD-26). So
  `korg-calendar.timer` fires every minute, and the script reads the live
  record back. It writes only when the window or the days differ, or when the
  live record is within 120 s of an hour old. A quiet minute writes nothing
  and logs nothing. A failed read is treated as absent, and the write then
  fails loudly if central is really down.
- **Python stdlib, from the checkout**, as korg-pulse does (sprint 100). The
  schema and validator are vendored byte for byte at `b1ad6af` with pinned
  digests.
- **korg-gate rulings, forced by its allowlist fence.** `list_tagged_days`
  is in `READ_TOOLS`, because the fence requires every `list_*` to be a read.
  `add_tagged_day` and `remove_tagged_day` are in `REFUSED_TOOLS`, because the
  proposal kept the gate out of this slice. Whether Duet may tag days is
  Ken's GP-17 call, filed as WI 4676. No gate build or install happened in
  this sprint.

## What shipped

- Migration 0039 and `repo/tagged_days.rs`. The three tools, the REST read,
  the vocabulary and the server instructions all went in. The docs went in
  too: the api.md catalogue and shape table, the usage.md REST row and
  vocabulary bullet, the gate.md refused list, and the README count, now 60.
- `crates/korg-core/tests/sprint101.rs` has 5 tests: the round trip,
  narrowing by kind, reaching a yearly row by date, no reach before the first
  year, the conflict, the bounds counted in characters, and removal
  ambiguity. The dispatch, read-shape and gate fixtures were extended, and the
  contract was regenerated.
- `calfeed/korg_calendar.py`, `calfeed/us_holidays.py` and `calfeed/vendor/`.
  `calfeed/test_korg_calendar.py` has 18 tests: the vendored pins, a validator
  that bites, the 2026 holiday list, observed days across weekends and years,
  and name and short bounds for 2021–2060. They also cover this November's
  record, LA dates rather than UTC, the UTC-safe window, yearly and leap days,
  the precedence of `short`, joining, the 64 cap and the publish decision.
- `bin/korg-calendar`, `systemd/korg-calendar.{service,timer}`, and
  `just calendar-check` (now part of `just check`) and `just calendar-dry`. A
  CI step was added, and docs/operations.md gained a § korg-calendar.

## Verification

- `just check` is green on kai.
- **Planted defects:** making a holiday imply `off` failed 2 tests, and dating
  in UTC instead of LA failed 1.
- **A local rehearsal** ran korg-api on a throwaway Postgres with 0039
  applied, using made-up tags in the brief's shape (two days off and a yearly
  special day). It drove the three tools over real MCP HTTP, including the
  conflict, a missing label, and removal by date and kind. Then
  `bin/korg-calendar --dry-run` against it printed exactly 11th Veterans Day
  (`off` false), 25th off, 26th Thanksgiving (`off` false), and 27th off plus
  special with the stored short. Christmas, New Year and MLK Day were all
  `off` false. The window was 2026-11-01 → 2027-02-14. Nothing was written to
  central.

## Follow-ups

- **Ship turn:** deploy the image (`deploy-kubsdb`), then install the timer
  through k-homelab's `repo-user-timer`, with a `korg-calendar` block beside
  `korg-pulse` in `manifests/kai.yml`. Then seed Ken's three tags through the
  MCP tools, fire the publisher once, read `kdash:calendar` back, and validate
  it with kdashdata's `jsonschema_mini`. Finally, add and remove a throwaway
  tag to show the on-change write.
- WI 4676 asks Ken's GP-17 decision on whether Duet may tag days.

## Deployed

2026-11-14 (PST), on the ship turn of karc leg `korg-4191a8`, cleared by the
overseer in comment 4698 on korg:4667.

- **korg image, via `deploy-kubsdb`** (the `.sprint-deploy` step). It was
  built from main `c49e2b6` and pushed as `korg:c49e2b6aae2e` and
  `korg:latest`. The revision gate passed ("running revision c49e2b6…").
  `post-deploy-check.sh --compare` reported OK, with no count down. Migration
  0039 applied: `GET /api/tagged-days` answered 200 `[]`, and `tools/list`
  serves 60 tools, the three tagged-day tools among them. The rollback target
  is `korg:792725128826`. Rolling back across 0039 is image-only; the table
  is harmless to an older image.
- **Seeded through the deployed MCP tools** (`tools/call` on `/mcp`): Ken's
  three tags, two days off and one yearly special day. The values are in
  korg's database, not here.
- **First publish fired by trigger.** A transient user unit,
  `systemd-run --user --wait --collect`, ran the service's ExecStart, PATH
  and working directory. It wrote `(absent)`, with 7 days and 2 of them off.
  The live record read back from central showed the 11th (Veterans Day, not
  off), the 25th (off), the 26th (Thanksgiving, not off), and the 27th (off
  plus the special day with its stored short). It validated against kdashdata
  main `0e921a1`'s schema with `jsonschema_mini` and no errors. kdashdata's
  C reader (`kdash_dump`) read it with 0 skipped and today covered.
- **The timer, via k-homelab `5b38d46`** (pushed to main, checkout on kubs0
  synced, k-homelab `just check` green). That commit added the
  `korg-calendar` block to `manifests/kai.yml`, the consumer page with its
  README row, and a changelog-kai entry. `bin/audit kai repo-user-timer`
  reported `WOULD CHANGE` for the new block only. `bin/apply kai
  repo-user-timer`, run from kubs0, reported `changed`. The timer is enabled
  and active.
- **The on-change write, by the timer's own firings.**
  - A throwaway tag added at 01:24:16 was written at 01:25:03 (`changed`, 8
    days).
  - Its removal at 01:25:11 was written at 01:26:03 (`changed`, 7 days).
  - The 01:27:02 firing succeeded and wrote nothing (`ts` unchanged).
  - Only Ken's three tags remain.

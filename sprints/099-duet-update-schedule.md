# 099 — korg-gate: Duet may retire or pause a schedule

Proposal korg:4416, a slice of the November backlog drawdown (program
korg:4368). It covers WI 4322: Ken asked on 2026-11-03 that Claude Duet be able
to retire the schedules it reviews. The overseer held it until claude-duet WI
4309 was settled, then Ken re-tested claude.ai's approval prompt on 2026-11-09
and it worked. This sprint ran as a karc leg on kai (`korg-bb2d66`), under
oversight. It follows the shape of sprints 095 and 097.

## Premise check

- **WI 4322 holds.** `update_schedule` was in `REFUSED_TOOLS`
  (`allowlist.rs`), and nothing else in the korg door's write set touches a
  schedule. korg's own `update_schedule` takes `status` (`active`, `paused`,
  `done`) and ten other fields.
- **The two retirements are already done, and this sprint did not repeat
  them.** Schedules 3950 and 1383 read `done`, with the reason in each one's
  notes. The overseer set them on 2026-11-09 at 02:56 UTC (WI 4322, comment
  4488). `get_schedule` through the new gate shows 3950's `updated` unchanged
  (Verification).

## Decisions

- **Status only, enforced at the gate.** The overseer's brief said to scope
  the allowance to status changes if the gate can express field-level limits.
  It can: the klams door already refuses a `memory_add` whose `kind` is not
  `knowledge`. The item's own ask was wider ("at minimum `status` and
  `archived`, ideally the fields the REST PATCH takes"). The overseer's brief
  is narrower, and it was followed. A new `LIMITED_TOOLS` table in
  `allowlist.rs` names the tool, its exact fields (`node_id`, `status`) and an
  advertised description.
- **Exactly those fields, and both are required.** A call with any other
  field is refused with `-32602`. So is a call with no `status`, or with
  arguments that are not an object. It is refused, not trimmed: a caller that
  asked for `notes` with a retire should hear that the gate would not write
  them, rather than get half a write. `origin` is not special-cased. korg's
  `update_schedule` has no such field, so sending one is refused like any
  other extra.
- **All three statuses are allowed.** `done` retires, `paused` pauses, and
  `active` resumes. The item asks for retire and pause. Resume is the only way
  to undo a mistaken pause, and it is still a status change.
- **`archived` stays out.** The item listed it as a minimum, but the brief
  scopes to status changes, and `done` already retires a schedule. Adding it
  is a one-line change to `LIMITED_TOOLS` if Ken wants it.
- **The advertised schema is narrowed to match.** In `tools/list`, the gate
  cuts `update_schedule`'s `inputSchema.properties` to `node_id` and
  `status`, marks both `required`, and replaces korg's description, which
  names `anchor_at` and the template fields, with one the gate writes. This
  way Duet is never offered an argument the gate would refuse. korg's own
  `status` enum and description pass through unchanged.
- **The audit trail is the one the other non-`origin` writes use.** The gate
  journal logs `tools/call … tool="update_schedule"` on every call and a WARN
  `tools/call arguments refused` on every refusal. `X-Homelab-Agent:
  claude-duet` is on every forwarded request. Duet writes the reason for a
  retire as `add_comment` on the schedule, and that comment's `origin` is
  stamped.

## What shipped

- `crates/korg-gate/src/allowlist.rs`: `update_schedule` moves from
  `REFUSED_TOOLS` (now 22) to `WRITE_TOOLS` (now 9). Adds `LIMITED_TOOLS`,
  `LimitedTool` and `limited()`. The unit test requires every limited tool to
  be a write that names its `node_id`. `create_schedule` and
  `materialize_schedule` are pinned as refused.
- `crates/korg-gate/src/proxy.rs`: `limit_korg_fields` runs on korg-door
  `tools/call` before `stamp_origin`. `narrow_schema` runs on the korg door's
  filtered `tools/list`, in both its JSON and SSE forms.
- Tests:
  - `update_schedule_reaches_korg_with_a_status_and_nothing_else` sends the
    three statuses, which are forwarded byte for byte. It then sends ten
    off-limit argument shapes plus a call with no `arguments` key. All are
    refused, and none reaches the stub.
  - `the_other_schedule_writes_stay_refused` is the negative control.
  - The unit test `update_schedule_is_advertised_with_only_the_fields_the_gate_forwards`
    runs against korg's real catalogue schema.
  - `the_write_set_is_exactly_the_ruled_nine`; the tool list now holds 35.
- `docs/gate.md`: the write table, the refusal list, and a paragraph on the
  status-only rule.

## Verification

- `just check` green: 776 passed, 0 failed.
- **Mutation checks.** Removing the `limit_korg_fields` call fails
  `update_schedule_reaches_korg_with_a_status_and_nothing_else`. Removing
  `narrow_schema` fails the schema unit test. Both were restored.
- **Loopback smoke against live korg**, from kai. The branch's release binary
  (sha256 `5eaf01f0…`) ran on `127.0.0.1:18475` with a fresh state directory
  and upstream korg-api on kubsdb:5674. The script is
  `.scratch/099/smoke099.py`. It signs in through the gate's own OAuth flow.
  - `tools/list` returned 35 tools, `update_schedule` among them. Its schema
    held exactly `node_id` and `status`, both required, with korg's enum.
  - **Forwarded, live, mutating nothing:** `update_schedule {node_id:
    999999999, status: paused}`. korg answered `not_found` ("no schedule
    with node_id 999999999"), so the call reached korg-api.
  - **Refused at the gate:** `update_schedule` on 3950 with `notes`, and with
    `cadence`. Both returned `-32602` "update_schedule through korg-gate takes
    exactly `node_id` and `status`".
  - **Negative control:** `create_schedule`, `materialize_schedule` and
    `update_proposal` were each refused as "not available through korg-gate".
  - `get_schedule 3950` read `done` with `updated` still `02:56:46Z`.
  - Gate journal: an INFO `tools/call` line on every call that passed the
    allowlist, a WARN `tools/call arguments refused` on each off-limit
    `update_schedule`, and a WARN `tool refused` on each negative control.

## Cross-repo changes made

- **cross-project-planning `2c03fcb`** (main, pushed, `main...origin/main`).
  It appends a paragraph on the schedule's status to GP-17's exception list
  in `korg+/PLAN.md`, after the klams-writes paragraph, by the same inline
  procedure as `05b7fff` and `7fc4971`.
- **claude-duet `1d48e2f`** (main, pushed, `main...origin/main`, `just check`
  ok). It adds a `docs/rulings.md` entry, "2026-11-09 — a schedule's status".
  `docs/connectors.md` (the live write list) and the entry's deployed line
  are updated after the gate deploy, so neither claims a live state early.

## Repaired in passing

- None. The `Description=… read-only` nit that sprint 095 recorded in
  `deploy/korg-gate/korg-gate.service` had already been repaired.

## Follow-ups

- None filed.

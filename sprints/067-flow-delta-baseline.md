# 067 — Flow endpoint: give the delta a baseline, then widen to 10 days

Proposal korg:1435 (program korg:1437, "Rate of Fire correctness").
Covers #1432 (S) and #1433 (XS). Branch `067-flow-delta-baseline`.

## Goal

kfdc's Rate of Fire panel printed a header that would not reconcile:

    in 119  out 136  durable out 42  backlog 157  -8/6d

`119 − 136 = −17`, not `−8`. Ken read it, was suspicious, and was right.

The series was not wrong. Every row reconciled exactly against its
predecessor — no archive noise, no reopens. The panel was being asked a
question its input could not answer: it computed the delta as
`bars[last].backlog − bars[0].backlog`, and every row's `backlog` is the
count at that day's *end*, so `bars[0]` already has the first day's own
flow applied. The delta spanned N−1 days while `added`/`closed` spanned N,
and both were labelled `/6d`. The gap was exactly the first day's own net.

That makes it korg's fix, not a kfdc patch: the day before the window is
simply not in the response.

## What shipped

**#1432 — `backlog_before` on the flow envelope.** Nullable open count at
the end of the day immediately before `days[0]`. The window delta becomes
`days.last().backlog − backlog_before`, over the same days the sums cover,
with the invariant `backlog_before + Σ(added − closed) == days.last().backlog`.

Implementation is one token of SQL plus four lines of Rust: the generated
series starts at `GREATEST(start_day - 1, horizon)` — one extra day in
front — and `work_item_flow` pops that row off and returns its `backlog` as
the baseline. At the horizon the `GREATEST` collapses the extra day onto
the first window day, so a clamped series comes back no longer than `days`
and the field is `null`. That length comparison *is* the null test; there
is no second query and no duplicated bounds logic.

`null`, never `0`, at the horizon — a zero is a real backlog level and
would read as "the backlog was empty", which is the silent-wrong-answer
failure mode #1318 built this endpoint to end.

**#1433 — `FLOW_DAYS_DEFAULT` 6 → 10.** The launch value of six was a
coverage decision (the whole window had to sit inside the transition log on
day one). The log now covers eleven days: ten for the window, one for the
baseline.

## Decisions

- **The consumer-side fix was rejected before it was written.** kfdc could
  have fetched `days + 1` and sliced off the first row. That puts horizon
  reasoning back in the caller — which this endpoint's own contract tells
  callers not to do — and degrades silently at the horizon, rendering N−1
  days under an "N day" label, the same class of bug. It also makes every
  future widening a dated question. With `backlog_before` the answer is
  never date-gated again.
- **The two items shipped together, in this order.** Not a code
  dependency, but widening the window while the delta was still off would
  have stretched the error (9 days under a "10d" label) without making it
  any more visible.
- **The extra day is an envelope field, not a series row.** A row would
  make `days.length` disagree with the window the caller asked for, which
  is precisely the number consumers are told to trust.
- **`FLOW_DAYS_DEFAULT == 10` is pinned in a test.** It is a decision with
  a reason (#1433), not a tuning knob, and a silent revert to 6 would undo
  it invisibly.

### Cross-project plan

`cross-project-planning/korg+/PLAN.md` applies (korg is in the routing
table). Nothing here contradicts a `GP-n`; **GP-1** is the reason this
sprint exists in korg at all — the board renders korg, so data the board
needs that korg cannot hold is a korg work item, never a consumer
workaround. No amendment needed.

## Tests

`crates/korg-core/tests/sprint067.rs`, four cases:

- the baseline is exactly the row a one-day-wider window leads with — asked
  two ways, pinned together, on a fixture built so the two spellings of the
  delta *disagree* (a fixture where they happen to match would pass with
  the bug still in);
- `backlog_before + Σ(added − closed) == days.last().backlog`, over three
  window widths;
- a window starting at the horizon has `None`, with a pre-horizon item in
  the corpus so a `0` would be a wrong count rather than a lucky one;
- the default window is ten days and still leaves a day for its baseline.

Transport sweeps assert the field is on both the MCP and REST envelopes.

## Follow-ups

kfdc #1434 (proposal korg:1436) consumes `backlog_before` and handles the
second consequence of the widening: at ten days `added_durable` stops being
structurally zero, so a summed "durable in" becomes *partially* knowable
and must not be printed as if it were comparable to a full-window "durable
out". korg's side of that is documentation — the tool description, the
`FLOW_DURABLE_AFTER_DAYS` doc comment and `docs/api.md` all now say it.

## Deployed

**2026-08-18** (kubsdb local; 03:2x UTC on the 19th), image
`kubsdb.encke-wahoo.ts.net:5000/korg:4c8ec330c6c9` — commit `4c8ec33`, the
squash-merge of PR #72. Rollback target `4c7d0249627f` (sprint 066), confirmed
present in the registry before the build.

Both tags pushed (SHA first, then `latest`). The deploy script's revision
assertion passed, so the running container is provably the commit that was
built. `post-deploy-check.sh --compare` clean: no row count moved in either
direction, `migrations`/`migration_max` both still 29 — this sprint carried no
migration, and the check confirms it.

Verified live against production data over the tailnet FQDN, which is the
point: the bug was only ever visible on real numbers.

| check | result |
|---|---|
| Default window | **10 rows**, 2026-08-09 → 08-18 (#1433) |
| `backlog_before` | **153** — exactly the value #1432 predicted for a 10-day window while the item was being filed |
| Invariant `backlog_before + Σ(added − closed)` | `153 + 5 = 158` = last row's `backlog` ✅ |
| Window delta, corrected | **+5**, and `in 170 − out 165 = +5` — the header now reconciles, which is the whole sprint |
| Window delta, old spelling (`last − days[0]`) | **−4** — still wrong on live data, so the fix is doing real work, not agreeing by luck |
| `days=999` (clamped to horizon) | 11 rows from 2026-08-08, `backlog_before: null` — null, not 0 ✅ |
| `/plan`, `/api/health` | HTTP 200 |

`added_durable` per day came back `[5, 0, 1, 0, 0, 0, 0, 0, 0, 0]` — Aug 9 → 5
and Aug 11 → 1, matching #1433's prediction exactly. That is the widening's
second-order effect now live: a summed "durable in" covers only the oldest
three days of the window. kfdc #1434 owns labelling it, and until that lands
the board must not print it beside a full-window "durable out".

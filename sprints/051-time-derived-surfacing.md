# 051 — Time-derived surfacing: scheduled maintenance and staleness age-out

Proposal korg:1079 (rank 2). Covers #581 (L, whole), #950 (M) and #1086 (XS,
the 050 deploy-script rider). Branch `051-time-derived-surfacing`, run
2026-08-08.

Two work items that arrived from opposite directions — one from a backup
drill, one from an outage — and are the same mechanism: **a date crossing now
makes something appear.** #581 is a stored date arriving; #950 is the
*absence* of a write past a stored cadence. korg has never had time-derived
state at all, which is why neither could be expressed before.

## The design claim, and why it is load-bearing

#581's own write-up assumes an engine: *"The engine would determine (once
daily?) if any maintenance items are due, then create the WI from the
template."* This sprint does not build one, per korg:1079's analysis:

- The durable node is the **schedule** (template + cadence + anchor).
- **"Due" is a read-time predicate** — `anchor_at + interval(cadence) <=
  now()`. So is "stale" — `last_report_date + cadence + grace < today`.
- **Materialising the WI is an explicit action**, taken when work starts.

No timer, no daemon, no missed-tick recovery, no did-the-job-run-today
question, and no duplicate materialisation from a double-firing tick.

This matters more than it usually would, because **#950 exists precisely
because a scheduled thing died quietly and nobody noticed for eleven days.**
Building korg's answer to time on another unattended scheduled thing would
have been an uncomfortable joke.

Consequence for #581's kmon tie-in, called out in the proposal and unchanged
here: kmon surfaces **due schedules**, not freshly-created work items. No WI
churn when nothing is acted on, and no orphan WIs when a drill is skipped a
cycle. The kmon side is a sibling change, not this sprint.

## #1086 first — the 050 rider

Two lines plus a local declaration out of `scripts/post-deploy-check.sh`'s
`counts()`: sprint 050 removed `/api/topics`, but the script still curled it
with `-fsS` under `set -euo pipefail`, so the stock script failed against any
post-050 build. The 050 deploy ran from a hand-patched copy; this means that
never has to happen again.

Deliberately not fixed by a new fence. `docs_drift` for `scripts/` would be
overkill for one call site, per the WI. What the miss actually exposed is that
sprint 050's removal inventory swept crates, web and docs and never looked at
`scripts/` — so the note lands in the removal checklist, not in CI.

A pre-050 baseline JSON still carries a `topics` key. The compare loop
iterates the *after* snapshot's keys (`$a | keys[]`), so the retired key is
simply not compared — no spurious `LOST` line, and no baseline migration
needed.

## #581 — the `schedule` node

`maintenance_schedule` was the WI's placeholder and it asked for something
shorter: the kind is **`schedule`**. korg has no other schedule concept for it
to collide with.

### Shape

Migration `0025`, the 0008/0010/0017/0023 detail-table pattern. A schedule
carries a project on `node.project_id` (unlike a program, which is the
cross-project layer — a schedule is maintenance *of* something, and that
something is a project).

| column        | why                                                        |
|---------------|------------------------------------------------------------|
| `title`       | the template title; carries substitutions                   |
| `template`    | the template WI body, substituted the same way; nullable     |
| `cadence`     | `once` / `weekly` / `monthly` / `quarterly` / `yearly`       |
| `anchor_mode` | `completed` or `created` — the WI's two anchor styles        |
| `anchor_at`   | the point the interval measures from                         |
| `status`      | `active` / `paused` / `done`                                 |
| `wi_type`     | what the materialised item is filed as; defaults `maintenance` |
| `wi_tshirt`   | template size                                                |
| `last_wi_id`  | the most recent materialisation, so due-ness can exclude it   |

**One `anchor_at` column covers both anchor styles**, which is what kept the
second style from being the seam korg:1079 said to cut. The styles differ only
in *which event advances the anchor*:

- `created` — `materialize_schedule` sets `anchor_at = now()`.
- `completed` — the anchor advances when the materialised work item reaches
  `done`/`closed`, as a **write rule in korg-core**, never a trigger. That is
  0023's own precedent for the awaiting marker: lifecycle rules live in the one
  path both transports share.

Seeding solves the cold start: the quarterly restore drill was last verified
2026-07-08, so its schedule is created with that `anchor_at` and comes due
2026-10-08 — no "everything is due on the day you file it" stampede.

### Due, exactly

```
status = 'active'
AND (last_wi_id IS NULL OR the last materialised item is done/closed)
AND anchor_at + interval(cadence) <= now()
```

`once` has a zero interval, so `anchor_at` *is* the fire date — the DST-recheck
case from the WI, with no second storage shape. Materialising a `once` schedule
moves it to `done`.

The outstanding-item clause is the anti-duplicate rule and it is also the
honest one: while the drill's work item is open, the drill has already
surfaced. The work item **is** the surface; the schedule stops competing with
it.

`resolved` deliberately counts as still outstanding here, unlike elsewhere in
korg. A restore drill that is "implemented, may still need a user test" has not
been performed.

### Substitutions

A closed set of five: `{YEAR}`, `{MONTH}` (long name), `{DAY}`, `{DATE}`
(ISO), `{QUARTER}`. Ken was explicit that his `"korg restore drill - {MONTH}
{YEAR} (Quarterly)"` example conveys the need and is not the spec, so this
stays small and closed rather than growing a syntax.

An unrecognised `{FOO}` is **rejected at write time** with the full allowed set
in the message — korg's standard `vocab::validate` idiom. The alternative is a
typo rendering literally into a work-item title months later, which is exactly
the kind of silent wrong this sprint is about.

Rendering uses **Postgres's clock**, for the reason `BoardRollup.generated`
gives: a date derived from a different clock than the rows beside it goes
subtly wrong on a cached or proxied consumer.

### A distinct type for generated items

`maintenance` joins `WI_TYPES` (7 → 8), so kmon and other automation can find
materialised items as a group — the WI's discoverability requirement, met by
the vocabulary rather than by a naming convention.

## #950 — staleness age-out

### Cadence: inferred, with a declared override

The WI left this open; korg:1079 recommended inferred-with-override and that
is what shipped. Inference is the **median gap** between consecutive
`report_date`s. kmon files daily and its own history proves it, so nothing has
to be configured for the case that motivated this.

The override lives in a new `report_source` table — a plain table, not a node,
the way `project` is. A source has no edges and no comments; it is a property
of a reporting stream, not a thing work happens to.

`retired: true` is how a deliberately stopped source stops nagging without
also silencing a broken one — the WI's other open question. It is an explicit
declaration, so the difference between "we turned this off" and "this died" is
recorded rather than inferred from silence.

### The rule the whole thing exists for

A stale source's `asserts` is **`unknown`**. Not the last known status —
never, structurally, in any code path. Restating July's GREEN from 2026-07-22
in a stale card is the exact failure this ends, so it is fenced by a test that
reproduces the July timeline and asserts the projection says `unknown`.

Two orthogonal fields rather than one overloaded one:

- `freshness` — `fresh` / `stale` / `retired` / `unrated`
- `asserts` — `ok` / `attention` / `problem` / `unknown`

`unrated` is the fourth freshness value because a source with fewer than three
reports has no inferable cadence, and both available lies are bad: calling it
`fresh` reproduces the failure, calling it `stale` cries wolf on every one-off
report and trains people to ignore the panel. `unrated` says what is true —
korg cannot judge this yet — asserts `unknown`, and does not alert. Declaring a
cadence is how a real source leaves that state on day one.

Grace defaults to the cadence itself (floor 1 day), so a daily source goes
stale on the third day of silence. Against the July outage: last report
2026-07-22, stale from 2026-07-25 — nine days before it was actually found,
and found by signal rather than by chance.

### Where it surfaces

`BoardRollup.sources`, beside the Sensor Net reports the board already
carries. The WI is explicit that it must surface on the same surface a real
problem would use, "since the whole lesson here is that a channel nobody looks
at is not a channel."

**korg-dash's Today page is out of scope** and that is deliberate, per
korg:1079: korg-dash is a separate project, so rendering there is a sibling
proposal. The derivation lives in korg's API; korg-dash consumes it.

## Decisions

- **D-1** — Kind is `schedule`, not `maintenance_schedule` (#581 asked for
  shorter).
- **D-2** — No scheduler. Due and stale are both read-time predicates;
  materialisation is an explicit write.
- **D-3** — One `anchor_at` column serves both anchor styles; they differ only
  in which event advances it.
- **D-4** — The `completed` anchor advances via a korg-core write rule on work
  item completion, not a DB trigger (0023's precedent).
- **D-5** — An outstanding materialised item suppresses due-ness. `resolved`
  counts as outstanding here specifically.
- **D-6** — Five substitutions, closed set, rejected at write time on a typo.
- **D-7** — `report_source` is a plain table, not a node.
- **D-8** — Cadence inferred by median gap, overridable per source.
- **D-9** — A stale source asserts `unknown`, structurally. Fenced by a test
  replaying the July timeline.
- **D-10** — `unrated` rather than guessing for sources with too little
  history.

## What shipped

**Migration** `0025_time_derived_surfacing.sql` — DDL only, purely additive.
`schedule` joins `node_kind_check` (eight kinds); the `schedule` and
`report_source` tables. Note the rollback asymmetry the header spells out: a
clean re-tag *until* the first schedule is filed, a restore after, because
schedule rows are `node` rows.

**Vocabulary** — six new sets (`SCHEDULE_CADENCES`, `SCHEDULE_ANCHORS`,
`SCHEDULE_STATUSES`, `SCHEDULE_SUBSTITUTIONS`, `SOURCE_FRESHNESS`,
`SOURCE_ASSERTIONS`), `maintenance` added to `WI_TYPES` (7 → 8), and three
fences: the schedule live/terminal partition, `SOURCE_ASSERTIONS` being exactly
`REPORT_STATUSES + unknown`, and one that forces a decision about alerting when
a freshness value is added.

**Relationship label** `materializes` (schedule → workitem), directed,
cross-project. `same_project` is false on purpose: `covers` enforces it because
a proposal is a single-project *bundle*, but a materialisation edge is
provenance, and provenance that refuses to record a cross-project fact lies.

**Surface** — 44 → **51 MCP tools** (`create_schedule`, `get_schedule`,
`list_schedules`, `update_schedule`, `materialize_schedule`,
`list_report_sources`, `set_report_source`) and seven REST routes.
`BoardRollup` gained `sources`, uncapped and alert-first.

**Web** — a `/schedules` page (due/in-flight/waiting, `preview_title` so you see
what Materialise would create, materialise/pause/resume) and a source-freshness
strip above the Reports list.

**Tests** — `crates/korg-core/tests/sprint051.rs`, 24 cases. The load-bearing
ones: `a_source_that_filed_daily_and_stopped_asserts_unknown_not_its_last_green`
replays the July timeline and also asserts the serialized row has no field that
*could* carry a last status; `an_outstanding_item_suppresses_the_schedule_and_
force_does_not_lift_it`; `creating_a_due_schedule_materializes_nothing`, which
pins the no-scheduler claim rather than trusting it.

## Two things worth knowing next time

**`--` comments cannot go in a `\`-continued Rust SQL string.** No newline
survives the continuation, so the comment eats the rest of the query and
Postgres answers `syntax error at end of input` — pointing nowhere near the
cause. Cost about ten minutes on `list_report_sources`; the explanations now
live in that function's doc comment, with a warning.

**`date + bigint` is not an operator.** `current_date - report_date` yields
`integer`, but a `::bigint` projection fed back into date arithmetic needs an
explicit `::int`.

## Verified before shipping

**Migration rehearsal against production data.** Today's dump
(`korg-20260808-032425.sql.gz`, migration 23 — it predates the 050 deploy)
restored into a throwaway container and brought up under this build: migrations
**23 → 25**, node count **992 → 985**. That −7 is exactly 0024's predicted drop
(6 daily-plan items + 1 topic), so 0025 was rehearsed landing *on top of* 0024
rather than in isolation. Both new tables present and empty.

**The staleness derivation, run against the real corpus** — and it works,
including finding something:

| source | freshness | asserts | last | n | cadence |
|---|---|---|---|---|---|
| `kyac` | **stale** | `unknown` | 2026-07-14 | 4 | 2 (inferred) |
| `kmon:vmlab-01` | unrated | `unknown` | 2026-07-04 | 1 | — |
| `t3-probe-037` | unrated | `unknown` | 2026-08-02 | 1 | — |
| `kaed-journal-pass` | unrated | `unknown` | 2026-08-06 | 1 | — |
| `kmon` | fresh | `ok` | 2026-08-07 | 23 | 1 (inferred) |

Three things this confirms at once: kmon is correctly `fresh` and carries its
real status; the one-off probes are correctly `unrated` and do **not** alert
(the cry-wolf failure `unrated` exists to prevent, on live data); and **`kyac`
has been silently stale for 21 days** — a real finding, which is the whole
point.

**Playwright e2e** (out of CI by design; setup.md says run it whenever a sprint
touches `web/`, and this one does): **60 passed**, against the restored
production-sized database. `a11y.spec.ts` passes on all eleven routes including
`/schedules`, and `work-item-edit.spec.ts` passes with `materializes` as the
eighth relationship label.

Two spec fixes were needed and are in this sprint:

- `work-item-edit.spec.ts` — the relationship-label list. Its own comment
  records this going stale in 025 and again in 044; caught here **during** the
  ship by reading that comment rather than by a later run, which is the first
  time the warning has actually worked as intended.
- `a11y.spec.ts` — its route list was missing `/programs` (added in sprint 044
  and never added here) as well as `/schedules`. Both added; both pass.

## Follow-ups

- **kmon should surface due schedules**, not freshly created work items — the
  consequence korg:1079 flagged and said to confirm with the kmon side rather
  than assume. Nothing in korg presumes it either way; `list_schedules(due_only:
  true)` is the read it would call.
- **korg-dash's Today page** rendering `sources` is the sibling proposal
  korg:1079 named. The derivation is here and unchanged by whether korg-dash
  consumes it.
- **Seed the real schedules.** The quarterly restore drill (last verified
  2026-07-08, WI #234) and Ken's unentered recurring cases are the reason this
  sprint moved to rank 2, and none are filed yet — the feature is empty until
  they are. Seed anchors with real last-done dates, not today.
- **Declare kmon's cadence** rather than leaving it inferred, once there is a
  reason to: inference is right today, but a declaration is what survives a
  gap in history.
- **`kyac` has not filed since 2026-07-14** — found by this sprint's own
  feature during pre-ship verification, not by anything else. Either it is
  broken (the #950 case, exactly) or it was deliberately stopped, in which case
  `set_report_source("kyac", retired: true, note: …)` is the one-call answer.
  Worth deciding rather than leaving as korg's first standing alert.
- **`cards-dnd.spec.ts` fails on a production-sized database**, and its own
  guard says why: Backlog renders 1284px tall in a 720px viewport, so grab and
  drop points cannot both be on screen. Unrelated to this sprint (the nav bar
  measures 48px — one row — with the `/schedules` link added, so nothing
  shifted). The board has outgrown what one pointer drag can span; the spec
  needs to scroll, seed into a short column, or use a smaller board.

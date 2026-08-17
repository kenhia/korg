# Scoped re-review: the sprint 044–059 surface (2026-08-17)

Sprint 061, proposal korg:1348, WI #1346 — the re-review the agent-surface
program close-out (plan §16) triggers on "new surface or session friction".
Both prongs fired: 19 sprints landed after the program closed 2026-08-02, and
a session deferred reading the 8,151-line `repo.rs` (since split, sprint 060 —
this review's prerequisite, and it held: every module below was read whole).

**Scope**: the agent/REST/UI surface and repo/query layer added in sprints
044–059 — programs + awaiting (044), board rollup (045), curated layer (046),
schedules + cadence (051/052), board events + deconfliction (053), node kinds
+ generic preview (054/055), images (056/057), MCP 2026-07-28 (058), backlog
flow (059). Sprints 047/048 are deploy infrastructure, not agent surface, and
got a records-level pass only. **Out of scope**: everything the 2026-07-21
REVIEW.md covered that is now machine-checked (docs_drift, one-contract-source
bindings, collection-read contract).

**Method**: T-style per plan §5 — every owning module read in full
(`repo/{board,programs,schedules,reports,flow,awaiting,attachments,preview,
relationships,work_items,proposals,planning,page,common,selectors}.rs`,
`vocab.rs`, `relationships.rs` registry, `korg-mcp/{lib,tools}.rs`,
`korg-api/{lib,img}.rs`, `docs/api.md`), then representative operations
traced against **production** (kubsdb :5674, 2026-08-17 ~04:25Z): `get_board`,
`list_work_items` (default + `parked`), `list_schedules status:"all"`,
`list_report_sources`, `work_item_flow`, `get_program 1374`, a `relate`
near-miss probe, and REST probes of `/api/projects/korg/plan`,
`/api/img/stats`, `/api/reports`. Findings are labeled **observed** (seen
live), **code-confirmed** (verified in the owning code, not yet manifest in
production data), or **preference**. Severity: high / medium / low / nit.

**Probe residue: none.** Every probe was a read except one `relate` call with
a deliberately unknown label, which is refused during validation before any
write; nothing was created, archived, or edited. (The proposal-1348
`proposed → active` transition visible in the board's ticker is `start-sprint`
doing its job, not review residue.)

---

## Findings

### F-1 — `parked` is invisible in every covered-work rollup — **medium, code-confirmed**

Sprint 054 added `parked` as a fifth `wi_status` and carefully classified it
on both vocabulary partitions (live + unfinished), fixed `SCHEDULE_DUE_SQL`
(054 D-5) — and missed the three per-status rollup aggregates, all of which
hardcode four buckets:

- the board's `cov` CTE — [board.rs:450-453](../../crates/korg-core/src/repo/board.rs#L450-L453)
  (`FILTER (WHERE w.wi_status = 'open'/'resolved'/'done'/'closed')`)
- `get_program_detail`'s slice rollup — [programs.rs:292-295](../../crates/korg-core/src/repo/programs.rs#L292-L295)
- the row types `BoardProposal` and `ProgramSlice` carry exactly
  `open`/`resolved`/`done`/`closed`, and `BoardProposal`'s doc comment still
  claims *"The four counts sum to `covered_count` — `WI_STATUSES` is exactly
  these four"* ([board.rs:58-59](../../crates/korg-core/src/repo/board.rs#L58-L59))
  — false since 054.

**Effect**: a parked covered item lands in `covered_count` and in **no**
status bucket. The four counts stop summing to the total; any consumer that
derives `open = covered_count − (resolved+done+closed)` (which is what a
progress bar renderer naturally does) silently counts parked as open, and one
that trusts the documented sum miscounts outright. Blast radius:
`get_board.active/queue`, `get_board.programs[].slices`, `get_program.slices`,
kfdc's Fire Missions + Operations panels, korg's own `/programs` progress
triplet (`sliceProgress` in `domain.ts` computes work-complete =
`resolved+done+closed` over `covered_count`, so parked reads as unstarted).

**Latent today, by luck**: production's one parked item (#376, kmon) is
covered by no live proposal — verified live. Parking a covered item
mid-sprint is a natural move; the first time it happens the board quietly
miscounts.

**Root cause is a fence gap**: `wi_statuses_partition_cleanly` fences the
vocabulary's *partitions*, but nothing asserts that a per-status rollup's
bucket set covers `WI_STATUSES`. This is the same shape as 054's own D-2
lesson (a fence that lists what it fences is not a fence) one level up.

**Target state**: a `parked` count on `BoardProposal` and `ProgramSlice`
(additive; ts-rs regen; kfdc absorbs one field), both SQL aggregates derive
their `FILTER` list from `WI_STATUSES` (or gain the fifth literal plus a
fence), the doc comment corrected, and a test asserting bucket-set ==
`WI_STATUSES` so status number six cannot repeat this.

### F-2 — Due schedules surface nowhere anyone looks — **medium, observed**

A schedule's due-ness is computed on read (correct, 051 D-2) — but the only
reads that compute it are `/schedules` and `list_schedules`, and nothing
routes "something is due" to any surface with daily traffic:

- `get_board` — the read whose own description says it answers "the WHOLE
  state of the work" — has **no schedules panel**: `BoardRollup` carries
  reports, sources, events, awaiting, blocked… and nothing time-derived from
  schedules ([board.rs:269-332](../../crates/korg-core/src/repo/board.rs#L269-L332)).
- The Today page (`/`) renders awaiting, the report pill, proposals, cards —
  zero schedule references (verified in `web/src/routes/+page.svelte`).
- The kmon integration (051's named follow-up: "kmon surfaces due schedules")
  remains undone.

**Observed live**: schedule 1112 (architecture infographic, weekly) has been
**due since 2026-08-08 — nine days** — unmaterialized and invisible unless
someone happens to open `/schedules`. Schedule 1383 (the 2026-08-21 kaed
rotation reminder) says in its own notes "whoever sees it due (daily flows,
list_schedules due_only) materializes it" — there is currently no such flow.
This is #950's founding lesson — *a channel nobody looks at is not a channel*
— reproduced by the feature built in #950's honour. (1112's every-other-due
rhythm is Ken's call; the finding is not "1112 should have been materialized",
it is that nobody was *shown* it was due.)

**Target state** (pick one, or two cheaply): a `due_schedules` block (or the
due rows of `list_schedules due_only:true`) on `BoardRollup`, so kfdc and
korg-dash inherit it for free; a due-count pill on Today beside the report
pill; and/or the kmon slice. The board addition is the one that fits the
existing architecture — the board is exactly the "one read so nobody crawls"
surface, and due-ness already has one definition to reuse
(`schedule_due_sql`).

### F-3 — `relate`'s description: "these eight labels and no others" — the registry has nine — **low-medium, observed**

[tools.rs:250](../../crates/korg-mcp/src/tools.rs#L250) enumerates eight
labels and asserts closure ("and no others"); the registry
([relationships.rs:43](../../crates/korg-core/src/relationships.rs#L43)) has
**nine** — `materializes` (schedule → workitem, sprint 051) is missing. The
description's endpoint-validation list ("`covers`, `includes`, `finding`, and
`has_handoff` also validate endpoint kinds") also omits `materializes` (both
ends pinned) and understates `has_attachment` (right end pinned).

**Probed live**: `relate` with label `materialize` → `invalid_input` naming
all **nine** registered labels and suggesting `materializes` — the runtime is
right, the description lies. An agent trusting the description believes it
cannot write a provenance edge it can in fact write, and believes the
vocabulary is smaller than it is. T2's rule applies: behaviour of a tool goes
in its description, and this description was hand-updated for 046/056's labels
but not 051's.

**Root cause is structural**: the label enumeration in the description is a
hand-written string, while the error message is registry-derived. `docs_drift`
fences the `docs/api.md` registry table but not tool-description prose.

**Target state**: derive the description's label list from `REGISTRY` (it is
a `&'static str` today, so either build it once with a `LazyLock` or fence it
with a test that every registry label appears in `relate`'s description).

### F-4 — Tool descriptions still describe the four-status world — **low-medium, observed**

The `parked` status (054) reached the derived schemas (the `wi_status` enums
correctly list five values — verified in `tools/list`) but not the
hand-written prose around them:

- `list_work_items` description: "Defaults: everything not terminal (`open`,
  `resolved`, `done`)" — and its `wi_status` param description repeats the
  three-value list ([tools.rs:234](../../crates/korg-mcp/src/tools.rs#L234),
  `ops.rs` param doc). The real default includes `parked`; api.md says so, the
  description does not.
- `update_work_item` description: lifecycle line names four statuses, no
  `parked` ([tools.rs:236](../../crates/korg-mcp/src/tools.rs#L236)).
- `list_schedules` description: "its last materialized work item is not still
  open/resolved" ([tools.rs:269](../../crates/korg-mcp/src/tools.rs#L269)) —
  the outstanding set is `WI_UNFINISHED_STATUSES`, which includes `parked`
  (and 054 D-5 exists precisely because parking a materialised item must keep
  its schedule quiet).
- Internal doc nits of the same class: `ScheduleRow.outstanding` doc says
  "open/resolved"; `ScheduleOmitted.not_due` doc's first line says "of the
  **returned** rows" (it counts *hidden* rows).

An agent deciding "will my parked item show in the default list?" or "does a
parked drill item suppress its schedule?" from the descriptions gets the wrong
answer to the first and no answer to the second.

**Root cause, same as F-3**: the 050 `docs_drift` extension checks vocabulary
enumerations in `docs/` prose against `vocab.rs`, but tool-description strings
in `tools.rs` are outside its reach — and they are the prose agents actually
read.

**Target state**: sweep the descriptions (five minutes), then extend the
vocabularies-in-prose fence to backticked status enumerations inside
`tools()` descriptions, so status six cannot re-open the gap.

### F-5 — The disposal doctrine table omits the three post-039 kinds — **low, code-confirmed**

`docs/api.md`'s disposal table (WI #855 — "which disposal do I write") lists
work items, cards, proposals, reports, handoffs, projects, links, areas,
comments and edges. Three kinds have shipped since it was written and none is
in the table:

| Kind | Actual ending | Where it says so |
|---|---|---|
| `program` | `archived` only (ProgramPatch) | nowhere |
| `schedule` | `archived` only (SchedulePatch); `paused`/`done` are lifecycle, not disposal | nowhere |
| `attachment` | **hard delete that does NOT refuse when referenced** — `DELETE /api/img/:id` cascades edges and comments ([attachments.rs:447-456](../../crates/korg-core/src/repo/attachments.rs#L447-L456)) | Images §, but not the table |

The attachment row matters most: the doctrine's load-bearing sentence is "a
hard delete refuses when the row is referenced", and attachments are the one
deliberate exception (an image is bytes, not history; discard is the point).
An agent applying the doctrine table to an attachment reaches the wrong
conclusion about what `DELETE` will do to a linked image. The exception is
correct; the table just predates it.

**Target state**: three rows plus one sentence naming the attachment
exception. (docs_drift's disposal-table check verifies prose↔vocab agreement,
not kind coverage — a coverage assert against `NODE_KINDS` would close the
class.)

### F-6 — The awaiting lane can keep ghosts for cards — **low, code-confirmed**

`set_awaiting`'s description invites marking "ANY kind: work item, proposal,
program, **card**", but the D-7 clearing machinery knows only three kinds:

- `settle_awaiting` clears on WI `closed`, proposal `done`/`declined`,
  program `done`, archived — no card arm
  ([awaiting.rs:155-174](../../crates/korg-core/src/repo/awaiting.rs#L155-L174));
  `update_card` calls it (it inherits the archived clause) but a card reaching
  `Done` or `Cut` clears nothing.
- `list_awaiting`'s ghost filter likewise checks WI/proposal/program statuses
  only ([awaiting.rs:131-142](../../crates/korg-core/src/repo/awaiting.rs#L131-L142)).

So a card marked awaiting and then moved to `Done` — or `Cut`, where the ask
is moot the way archived is — stays in the Commander's Call lane until
someone clears it by hand. D-7's own rationale ("a state only Ken sets"
clears the marker) *applies*: the kanban is Ken's board, and `Cut` is
unambiguous. Latent — production's lane holds two rows, both work items.

**Target state**: add card `Done`/`Cut` to both the write rule and the lane
filter (statuses from `CARD_STATUSES`, not literals), or amend `set_awaiting`'s
description to say card asks clear only manually. The first is ~6 lines and
symmetric; preference: do that.

### F-7 — `materialize_schedule` re-checks nothing inside its transaction — **low, code-confirmed**

The outstanding/active/due gates run on a pre-transaction read
([schedules.rs:711-745](../../crates/korg-core/src/repo/schedules.rs#L711-L745));
the transaction then takes `FOR UPDATE` on the schedule row for template
rendering but never re-verifies the gates. Two concurrent calls — the
realistic case is an agent retrying a timed-out call — can both pass the
pre-checks, serialize on the row lock, and **both** create work items: two
open copies of the drill, `last_wi_id` pointing at the second, one
`materializes` edge each. That is precisely the duplicate-materialisation
failure the no-scheduler design names as the thing it prevents (051 D-2), and
`force`'s docstring says the outstanding check is the one thing force never
lifts.

Single-user korg makes this unlikely and self-evident when it happens;
severity is honesty about the contract, not observed damage. **Target
state**: re-evaluate the outstanding/status/due predicate inside the
transaction after `FOR UPDATE` (one query, same SQL fragment), refusing as
the pre-check would.

### F-8 — The plan payload cannot signal truncation — **low, code-confirmed**

`GET /api/projects/:name/plan`
([lib.rs:555-571](../../crates/korg-api/src/lib.rs#L555-L571)) fetches **one
page** of `LIST_LIMIT_MAX` (500) work items, discards the envelope's `total`,
and returns bare `{items, edges}`. A project past 500 items gets a silently
truncated frontier — oldest-first, so precisely the newest items vanish — and
the payload carries nothing a consumer could notice it by. `/plan` and the
`plan-status` skill both compute the frontier from this.

Checked live: korg, the largest project, holds 178 items (every status,
archived included — and the plan read does include archived, `archived:
None`, which is itself a quiet deviation from the archived-excluded default
worth a deliberate yes/no). No project clips today; the repo's own doctrine —
"you can tell a complete answer from a clipped one" — is what this violates,
and #762 is the precedent for how these ceilings go from latent to live.

**Target state**: walk the pages (or raise a `total` into the payload and let
consumers decide), and decide archived inclusion on purpose.

### F-9 — Nits (all **nit**, all observed in code)

1. `list_comments` description says "on a node (work item or card)" —
   comments attach to any node kind, and since 055 every kind's UI surface
   shows them ([tools.rs:241](../../crates/korg-mcp/src/tools.rs#L241)).
2. The registry module doc and one test still say "free-form labels stay
   legal" ([relationships.rs:41-42](../../crates/korg-core/src/relationships.rs#L41-L42),
   `unknown_labels_are_caller_defined_and_keep_their_direction`) — the
   vocabulary closed at LB-2; `relate` rejects unregistered labels. The
   rationale text predates the closure.
3. `docs/api.md`'s images example curls `http://kubsdb:5674`; production
   serves `https://kubsdb.encke-wahoo.ts.net:5674`.
4. Staleness uses Postgres `current_date` (UTC session) while the flow series
   buckets in `KORG_TIMEZONE` — a source can flip stale up to ~8h off the
   board's local day. Grace ≥ 1 day makes it immaterial; worth one sentence in
   api.md if anyone ever chases a "why is this stale already" report.
5. `get_attachment`'s not-found message names the decimal node id even when
   the caller passed an `img-<hex>` id — accurate but momentarily confusing.

### F-10 — Observation, not a defect: probe report-sources never leave Sensor Net

`sources` derives one row per distinct `report.source`, reports have no
disposal (by design), so the four one-off probe sources (`t3-probe-037`,
`kaed-journal-pass`, `kaed-live-test`, `live-test-014`) sit in the panel as
permanent `unrated` rows. Correctly not alerting — the vocabulary is doing
its job — but they are noise rows with exactly one exit:
`set_report_source(<source>, retired: true, note: …)`. **Recommendation**
(one call each, Ken's or wrap-up's): retire all four; adopt the convention
that harness probes either reuse a retired-probe source name or get retired
in the same session that writes them (the report-side sibling of the EVAL
category rule).

---

## Works as documented — the ledger, with the calls

- **The flow series** (059): default 6-day window ending the *local* today;
  `horizon: 2026-08-08`; `timezone: America/Los_Angeles`; and the invariant
  the docs promise held exactly — today's `backlog` **157** ==
  `list_work_items` default `total` **157**, same instant. Durable split
  carrying real signal (08-16: closed 38, closed_durable 16); `added_durable`
  structurally zero at the 6-day window, as documented.
- **The board** (045/046/053): one call, every panel. `blocked` served a real
  deterministic entry (184 ⟂ 176, `via: proposal`, `sequenced_by: null` —
  correct: no live program orders that pair). `proposal_edges` carried live
  curator provenance (`origin: "kfdc-curator"`). Three live `⟦curator⟧`
  synopses rendered with their own `updated`. `events` showed only real
  transitions (a `done → closed` and a program `holding → done` among them —
  the no-op guard's work visible in the stream's shape). `depth` carries
  `status` for all 49 projects. `sources` uncapped, alert-first. No counters
  block.
- **Programs** (044): `get_program 1374` — a nine-slice, four-repo program
  (`span: [agent-skills, kaed, klams, krot]`) rolled up in one call, ordered
  by edge rank (0..9 with a gap — stable), per-slice status counts correct,
  ten comments inlined with `comments_truncated: false` at exactly the cap
  boundary. The program layer's design case — and its comment thread is a
  worked example of a program *as* the cross-repo log.
- **Schedules** (051/052): the due predicate live and honest — 1112 `due:
  true` (anchor+7d elapsed, nothing outstanding), 1109 (`once`) `done` with
  `materialized_count: 1` and its history intact, 1383 not due until its
  anchor. `list_schedules status:"all"` returned the done row with
  `omitted.done: 0` — the asked-for class correctly not counted as hidden.
- **Cadence span gate** (052): kyac `unrated` (span 5d, 4 reports — the
  fixed false alarm), kmon `fresh`/`ok` (32 reports, span 40d, inferred 1d),
  `kmon:vmlab-01` `unrated` on 2 reports despite a 37-day span (the
  min-history gate doing its separate job — D-3's "not redundant" claim,
  observed). Zero stale alerts, correctly.
- **Awaiting** (044): two rows, oldest first, notes carried, statuses
  resolved per kind.
- **Lean work-item list** (042/#861 baseline): `omitted` cascade honest
  (`closed: 749`, `archived: 21` hidden behind a 157-row live view);
  membership markers live (`proposal_node_id`, `has_handoff` populated);
  `parked` present in the schema enum and returnable as a filter.
- **Label registry** (LB-2 + 044/046/051/056 additions): the near-miss error
  is exemplary — names all nine labels and suggests `materializes` for
  `materialize` (the #890 separator/near-miss fix, working).
- **Images** (056/057): production has 3 linked / 0 pending attachments,
  `total_bytes` 533,987 — the feature is in real use; stats carry `root` and
  `pending_ttl_hours` as documented.
- **MCP 2026-07-28** (058): this review is itself the live gate re-run — a
  Claude Code ≥2.1.227 session enumerated 54 tools and drove focused reads,
  rollups, list envelopes, an error envelope and a REST fallback against the
  deployed build without a single transport anomaly.
- **repo.rs split** (060): the per-domain modules made this review's method
  possible — every owning module was read whole, which is what the split was
  for. No behaviour wobble found at any module seam.

**Plan §5 exit measurements, re-run**: (1) the global `~/.claude/CLAUDE.md`
still carries no korg block — PASS. (2) All findings "works as documented"? —
**FAIL, mildly**: two medium findings (F-1 latent contract break, F-2 design
gap), the rest low/nit. The gaps are another tier smaller than T3's (which
were themselves a tier smaller than T2's): no misleading default, no token
bomb, no silent wrong answer being served today. The loop is converging;
these are the residue of three weeks of fast surface growth, mostly
description drift plus two places where sprint 054's fifth status and sprint
051's due-ness didn't reach every consumer.

## Ranked shortlist (benefit ÷ effort), with the WIs filed for each

Every actionable finding is backed by a work item (filed 2026-08-17, tag
`review-2026-08`), per Ken's direction for this review.

1. **F-2 — put due schedules where eyes are** (M) — **korg #1385**: the only
   finding where production is *currently* failing someone — a due signal sat
   invisible for nine days. Board block + Today pill; kmon slice optional
   after.
2. **F-1 — the `parked` bucket** (S) — **korg #1386**: latent contract break
   with a fence gap behind it; cheap now, confusing later. Fix +
   vocabulary-derived-buckets fence.
3. **F-3 + F-4 + F-9.1 — description-drift sweep + fence** (S) — **korg
   #1387**: the entire class in one pass — registry-derived label list in
   `relate`, parked-aware status prose, `list_comments` — then extend the
   vocabularies-in-prose fence to `tools()` descriptions so the class is
   closed, not just cleaned.
4. **F-5 + F-9.2/3/4 — doc corrections** (XS) — **korg #1388**: disposal rows
   for program/schedule/attachment (+ the refuse-exception sentence), registry
   module doc, URL + timezone notes.
5. **F-6 — card ghosts in the lane** (XS) — **korg #1389**: six lines,
   symmetric with D-7.
6. **F-7 — materialize re-check under lock** (XS) — **korg #1390**: honesty
   fix for a promise the code already makes in prose.
7. **F-8 — plan payload completeness** (S) — **korg #1391**: latent ceiling;
   fold into whatever next touches `/plan` or `plan-status`.
8. **F-10 — retire the four probe sources** (one call each): data hygiene, no
   code, no WI — a wrap-up action, not a sprint.

Items 2–7 (#1386–#1391) are one comfortable fix sprint — the 038 shape; item
1 (#1385) wants a small design decision (which surface(s)) before its build.

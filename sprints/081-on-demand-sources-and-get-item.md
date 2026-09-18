# 081 — On-demand report sources, a type-agnostic `get_item`, and a gate on the registry assertion

Proposal korg:2814, slice 2 of program korg:2816 ("Clear the korg backlog").
Run as an overseen karc leg (`korg-d9fdac`) on kai.

Covers WI 2183, WI 2446, WI 2172.

## Goal

The three items share one shape, which is why they are one branch: **korg holds
a registry, and something outside it keeps a hand-copy that drifts.** 2183 is
freshness *inferring* a cadence because nobody can declare the absence of one;
2446 is every caller hand-deciding a node's kind before it can read it; 2172 is
a test hand-copying the relationship-label registry.

## Premise check

All three hold, and 2183's is now stronger than when it was written.

- **2183 — holds, and the predicted failure is live.** Read from
  `list_report_sources` on 2026-09-17: `kyac` is `freshness: stale`,
  `overdue_days: 3`, against `cadence_days: 2` with **`cadence_declared:
  false`** — a cadence korg invented. Its own note says `unrated` is the
  correct state for it. The 7× span gate added in sprint 052 specifically to
  exclude kyac now passes it: the median gap stayed 2 while the history span
  grew from 5 days to 64, so the ratio crossed. The guard did not fail, it
  **expired** — which is the argument for a declaration over a cleverer
  inference, now with a live instance behind it.
- **2183's urgency claim — drifted, and the drift does not change the
  direction.** The proposal says `kfo-soak` is the clock. Measured:
  `report_count: 2`, `history_span_days: 2`, `freshness: unrated`. It cannot
  promote at three reports either — `median_gap` is a continuous median, so
  with three reports the span is about twice the median and cannot reach 7×.
  The urgency is kyac, which is already wrong, not kfo-soak, which has runway.
- **2446 — holds.** No `get_item` tool exists; `NODE_KINDS` has nine kinds and
  a caller holding a bare id must guess among the typed reads.
- **2172 — holds.** `web/tests/e2e/work-item-edit.spec.ts` asserts the picker
  against a hand-copied ten-label list in registry order, and
  `relationships::REGISTRY` has ten entries. Playwright is outside `just check`
  and CI, so the next addition breaks it invisibly.

## Guiding plan

korg routes to `korg+/` in the cross-project planning index. Decisions this
sprint touches, cited rather than rediscovered:

- **GP-13** — a figure the consumer cannot compute is korg's to return. So
  `on_demand` travels on the `SourceHealth` row as data; kfdc must not infer it
  from `cadence_days IS NULL AND NOT retired`.
- **GP-13's state half / GP-14 / GP-19's sequencing lesson** — korg owns the
  vocabulary and grows the literal; the consumer meets an undecorated
  `on-demand` and renders it neutral until it has a treatment. korg ships
  first, kfdc follows. Undecorated is survivable; mis-decorated is what #1444
  cost. This is why the kfdc Sensor Net item is filed when this lands and is
  not in this sprint.
- **GP-19's per-transition reading** — `on_demand` is *declared*: korg never
  sets it and never clears it, on either transition. That is `parked`'s
  position, not `soaking`'s, and it is worth saying because the last two
  statuses korg grew were both partly maintained by korg.

## Decisions

### `on-demand` is a fifth freshness literal, not a pinned `unrated`

WI 2183 offered "or, minimally, pinned `unrated`". Rejected: `unrated` means
*korg cannot judge yet and more reports will fix it*, and `history_span_days`
exists specifically to show progress toward being rated. An on-demand source is
not progressing toward anything — more reports must never promote it. Reusing
`unrated` would make the two indistinguishable on the exact axis a reader cares
about.

### `retired` outranks `on_demand` in the `rated` CASE

A source can carry both flags. `retired` wins: it is the stronger claim (this
ended) and `on_demand` describes how a *live* source files.

### Sort position

`stale → fresh → unrated → on-demand → retired`. On-demand goes below the line
with the two non-alerting states, and above `retired` because it is live.

## What shipped

### WI 2183 — `on_demand` report sources

Migration `0035_on_demand_sources.sql`: one `BOOLEAN NOT NULL DEFAULT FALSE` on
`report_source`. DDL only, no backfill; the kyac/kfo-soak declarations are data
written after deploy, deliberately not baked into a migration that must also
apply to a fresh install.

`on-demand` is a fifth `SOURCE_FRESHNESS` literal. It asserts `unknown`, never
alerts (`alerts()` is still `freshness == "stale"`, and
`exactly_one_freshness_is_the_alert` was bumped 4 → 5 as the deliberate
decision that fence exists to force), and sorts
`stale → fresh → unrated → on-demand → retired`.

**The short-circuit is in `judged`, not `rated`, and that is the load-bearing
choice.** Pinning only the freshness literal would leave `cadence_days` holding
the inferred number, and `grace_days`/`due_by`/`overdue_days` are all computed
from it — the row would have read "on-demand, 3 days overdue", a contradiction
worse than the plain `stale` it replaced because it looks deliberate. Nulling
the cadence at source makes all four fall out together and puts the declaration
ahead of all three inference constants, so no threshold can reach the outcome
and none can expire into it again.

`set_report_source` gains the fourth override and refuses a row that would carry
both `on_demand` and a declared cadence. The check is against the **resulting
row state**, not this call's fields, so the contradiction cannot be assembled
over two calls — which is the likelier path, since the caller who meets it is
usually correcting a cadence declared months ago. The refusal names the fix
(`cadence_days: null` or `on_demand: false`), not just the problem.

Nine tests in `crates/korg-core/tests/sprint081.rs`, including a reproduction of
the live kyac timeline (6 reports, gaps 56/2/2/2/2, span 64) that asserts korg
is currently **wrong** — the before half of the before/after, kept so the
expired gate stays pinned in the suite rather than remembered as an argument.

### WI 2446 — `get_item`

`get_item(node_id)` resolves the stored kind and dispatches to that kind's typed
read, returning `{kind, item}` where `item` is byte-for-byte what the typed read
returns. `kind` travels with the payload because the caller reached for this
tool precisely not knowing it; a bare body would make them reconstruct the one
fact they came here missing.

`repo::node_kind_of` is the public counterpart of `common::node_kind`, which is
`pub(super)` and spells absence as an error. A dispatching caller wants absence
as a value — one more branch, not an error to propagate.

`get_item_covers_every_node_kind` is **behavioural, not a source grep**. An arm
proves nothing here: an arm calling the wrong typed read reproduces exactly the
guess-and-fail this tool removes, while looking correct in source. The test
seeds one node of each of the nine kinds, asserts `get_item` reports the right
kind and returns that node's payload, and compares the covered set against
`NODE_KINDS` — so a tenth kind fails the build until it has an arm.

**The vocabulary half is not shipped, deliberately — it is Ken's ruling.** See
"Open for Ken" below.

### WI 2172 — the registry assertion is gated

`the_e2e_label_picker_list_matches_the_registry` in korg-mcp's `docs_drift`
suite parses the literal array out of `work-item-edit.spec.ts` and compares it
against `relationships::REGISTRY` — membership **and** order, since `toHaveText`
is positional and a reordering would otherwise be just as invisible as an
addition.

Gate rather than derive, and the reason is the whole point of the item: deriving
the list from `vocab.ts` would be more elegant and would still only fail where
Playwright runs, which *is* the problem. This fails in `just check`, in front of
the sprint that caused it. Proved by perturbation before shipping — adding a
label to the spec fails the test with both lists in the message.

The spec's staleness log now records that the count stops at four, and why.

## Repaired in passing

**`set_report_source` read and wrote outside a transaction, so concurrency could
build the row the check refuses.** Found by the overseer's round-2 review
(handoff korg:2820).

The contradiction check deliberately reads the *resulting row state* rather than
the call's fields, which closes the two-call assembly path by **sequence**.
Without a row lock the identical row is reachable by **timing**: against a row
carrying neither declaration, one call sending `on_demand: true` and another
sending `cadence_days: 7` both read the clean state, both pass, and both write.
The result carries both — and is then invisible, because `judged` short-circuits
on `on_demand` and never consults the cadence again.

Fixed by wrapping the read and the upsert in one transaction with
`SELECT … FOR UPDATE`. Not a CHECK constraint: migration 0035 already argues
that case on error-message grounds — core names both fields and the fix, a
constraint violation names neither — and a transaction keeps every one of those
messages intact. `FOR UPDATE` locks nothing on a row that does not yet exist, so
the first-insert race is not closed by the lock; `ON CONFLICT DO UPDATE` makes
the loser an update, and that residue needs an advisory lock, which is not worth
one. Recorded in the code comment rather than left implicit.

**The first version of the test was wrong and is worth recording as such.**
Spawning the two competing calls and asserting the outcome passed **5/5 against
the unfixed code** — two fast calls do not collide on their own. That is the
mirror-image failure GP-14 warns about: a test that asserts the opposite of its
own name while nothing ever says so. Replaced with one that forces the
interleave — hold the row lock in an explicit transaction, assert the competing
call *blocks*, then commit a contradictory declaration and assert the call is
refused when it re-reads. That version fails 3/3 without the lock and passes
with it, checked by perturbation the same way the 2172 gate was.

**korg's own web app had no treatment for the literal korg was about to grow.**
Found because korg is its own consumer here — `web/src/routes/daily-reports`
renders source freshness, so GP-19's "consumer ships later" sequencing does not
apply to it the way it applies to kfdc. All three are in this repo, verifiable
by the gate already running:

- `isScheduledSource` is an exclusion list (`{retired, unrated}`), so
  `on-demand` — the *most* explicitly unscheduled row korg can serve — would
  have been listed in the collapsed bar as a source korg holds to a cadence.
  The exclusion default failed in the safe direction (surfacing beats hiding on
  this panel, which is exactly what its comment argues for) but it was still
  wrong. `on-demand` added to the set.
- The per-source detail's `{:else}` arm prints "every {cadence_days} days". An
  on-demand source has no cadence by construction, so it would have rendered
  **"every undefined days"** on the row whose entire content is that there is no
  schedule. Given its own arm, which states what it reports on instead — an
  operator note replacing the generic line, the same rule the `unrated` arm
  follows.
- Both freshness colour maps got an explicit `on-demand` entry. The `??`
  fallback already yields the right muted treatment, so this is GP-14's "pin the
  specimen" rather than a fix.

## Cross-repo changes made

**`cross-project-planning` — GP-14 amended** (`3ac85f6`, committed to main).
Executed inline under the plan's own amend rule (GP-8's other half: "a sprint
that contradicts or refines a GP-n decision amends this file in the same ship —
edit, commit to main, reference the id in the commit message"), which is a
documented procedure callers execute, with no new decisions of its own.

The instance is routine — korg grows a literal, kfdc meets it undecorated and
renders neutral, `on_demand` travels as data per GP-13. The half worth the
amendment is general: **a threshold tuned against measured data is a sample
too, and it expires rather than fails.** The 052 span gate was picked against
two live sources and deliberately set "well clear of both ends"; it excluded
kyac for five weeks and then stopped, with no code change, no bad data, and
nothing to notice — kyac's median gap stayed 2 while its span grew 5 → 64 days
and the ratio crossed on its own. Only one side of a ratio was ever bounded, so
raising 7 buys time, not correctness. That is GP-14's "a sample cannot support
a domain claim" in a fourth register, and it is why the fix is a declaration
that makes the inference unreachable rather than a better inference.

Offered for the overseer's ruling.

## Open for Ken — the node-reference vocabulary (2446's second half)

Not shipped, because it is a convention change reaching kfdc, the skills and
Ken's own typing, and the proposal is explicit that it is his call.

Chasing it turned up a sharper fact than the item records. korg's `search`
already returns a `locator`, and it is **kind-ambiguous for seven of the nine
node kinds**: work items get `WI-<n>`, and a proposal, card, handoff, program,
report, schedule and link *all* spell as `korg:<node_id>`. So Ken's
`wi:` / `prop:` / `prog:` sketch is not a competing dialect for the same job —
it is asking for a distinction the current spelling genuinely cannot make, and
which is exactly what cost the session that burned two reads on node 2440.

Three options, no recommendation banked:

1. **Extend the existing locator** to carry a kind prefix for non-work-item
   kinds (`prop:2814`, `prog:2816`), keeping `WI-<n>` as-is. One spelling, more
   informative. Changes what `search` returns, which kfdc and the skills read.
2. **Leave it and say so in the docs** — `get_item` now removes the cost of not
   knowing the kind, which was the practical complaint. The ambiguity stays but
   stops mattering.
3. **`korg:<kind>:<n>`** — unambiguous, but a third spelling alongside `WI-<n>`
   and bare `korg:<n>`, and the plan's own advice is not to introduce a second
   dialect.

Option 2 is free and already landed; 1 and 3 are contract changes that want a
slice of their own.

## Follow-ups

- **kfdc Sensor Net rendering an on-demand source** — not filed yet, by the
  program's own rule (korg:2816 notes: a follow-up filed against work that has
  not shipped is a guess). To be filed at ship, once the literal is live.
- **The `kyac` and `kfo-soak` declarations are data, not migration.** They must
  be written after deploy: `set_report_source("kyac", on_demand: true)` and the
  same for `kfo-soak`, each with its note rewritten to drop the paragraph
  explaining why it was lying. kyac is the live acceptance check — it should
  leave `stale`/`overdue_days: 3` the moment it is declared.

## Deployed

**2026-09-18, to kubsdb** (`https://kubsdb.encke-wahoo.ts.net:5674`) via the
`deploy-kubsdb` skill, from merged `main`.

- Image `kubsdb.encke-wahoo.ts.net:5000/korg:43d68e7f26e6`, digest
  `sha256:630bb7af0ff0…`, both tags pushed (SHA first, then `latest`).
- Revision assertion passed in-deploy: running
  `43d68e7f26e64850ebc51c6d8aae82f6a6170343`, the commit built. Previous
  release was `eb0d91b3c5fc` (sprint 080), still in the registry as the
  rollback target.
- `post-deploy-check.sh --compare`: **OK**. Migrations **34 → 35** (0035
  applied). Every row count unchanged — cards 30, links 21, projects 59,
  proposals 490, reports 81, work items 1633, nodes 2719. Nothing dropped.

### Verified live, against the deployed release

**`get_item` (WI 2446)** — 57 tools advertised, up from 56. Called on four
kinds through the deployed MCP endpoint, each returning the right `kind` and
that node's payload: 2183 → `workitem`, 2814 → `sprint_proposal`, 2816 →
`program`, 2819 → `handoff`. A bare id naming no node returns
`{"code":"not_found"}`, which is the unambiguous case the tool exists to give.

**`on_demand` (WI 2183) — the acceptance, with numbers.** Declared against the
deployed release, not a fixture. `kyac` before and after:

| field | before | after |
|---|---|---|
| `freshness` | **`stale`** | **`on-demand`** |
| `overdue_days` | **3** | **0** |
| `cadence_days` | **2** | **null** |
| `cadence_declared` | false | false |
| `grace_days` | 2 | null |
| `due_by` | 2026-09-15 | null |
| `asserts` | unknown | unknown |
| `report_count` | 6 | 6 |
| `history_span_days` | 64 | 64 |

The invented cadence and everything computed from it fall away together, which
is what putting the short-circuit in `judged` rather than `rated` bought. The
history is still reported — the declaration stops korg *judging* this source,
not remembering it.

**An open question from the clearance, answered by doing it rather than
assuming:** kyac did **not** need `cadence_days: null` in the same call. The
contradiction guard reads the *stored* declaration, and kyac's cadence was
inferred — stored `NULL` — so the declaration passed on its own. A source with
a genuinely *declared* cadence would have been refused, which is the behaviour
`on_demand_refuses_a_row_that_already_declares_a_cadence` pins.

`kfo-soak` likewise moved `unrated` → `on-demand`, `cadence_days` null,
2 reports, and can no longer be promoted by any amount of history.

Live ordering confirmed on `/api/report-sources`: `fresh → on-demand →
retired`, with `on_demand` travelling as data on the REST surface as well as
MCP. **The panel now carries zero `stale` sources** — kyac was the only alert
on it, and it was a false one for days.

Both notes were rewritten in the same calls: kyac's drops the paragraph
explaining why `retired` was being refused, and kfo-soak's drops its
instruction to fall back to `retired: true`, which no longer applies.

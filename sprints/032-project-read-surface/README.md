# Sprint 032 — the tiered project read surface

Proposal `korg:816`, **still not closing it**. This is Phase B of the program in
`sprints/planning/2026-07-31-project-routing-program.md`; gate 2 (#758's checker
and its data pass) and Phase C (the behavioural acceptance test) come after.
**Do not let `sprint-ship` mark the proposal `done`.**

Covers **#828**, **#673** and **#674**, implementing the gate-1 field review
recorded as a comment on **#757**.

The review's verdict, which is the sprint in one line: *the schema is mostly
innocent; the read surface is guilty.* All nine stored fields earn their
storage — but a no-arg tool returning 10 columns × 35 rows makes every consumer
pay tier-3 payload for a tier-2 question.

## What shipped

**#828 — three tiers, because there are three questions.**

| Tier | Surface | Question |
|---|---|---|
| 1 | MCP `instructions` roster | "what projects exist" |
| 2 | `list_projects`, lean default | "does this belong here?" |
| 3 | `get_project(name)` | "tell me everything about this one" |

`list_projects(status?, detail?)` defaults to **active only, lean** — `name`,
`description`, and `status` only when it is not `active`. It returns
`{items, omitted:{archived}}` rather than a bare array, which is the one
deliberate break with korg's collection-read convention (WI #579) and the
decision worth defending: a filtered list that looks bare is how "there is no
such project" gets concluded from "you didn't ask for archived ones". Silent
truncation is the exact failure this program exists to treat.

**#673 — the reword**, applied to all six sites sharing the string, not just
`create_work_item`. It now names an example, states that a wrong name returns
`not_found` rather than mis-filing, and demotes `list_projects` to the
genuinely-ambiguous case.

**#674 — the roster**, rendered at `initialize` from live data.

## Deviations from the review, and why

Four, each recorded because a later reader will otherwise think they were
oversights.

**1. `get_project` inlines areas, not comments.** The review specified "full row
+ inline comments, the `get_work_item` pattern". That is not implementable:
`comment.card_node_id` references `node(id)`, and a project is not a node —
`node.project_id` points *at* projects. Inlining comments means making `project`
a node kind, a far larger change than this WI. Areas are the faithful
adaptation: the intent was that a focused read spares the caller a second
round-trip, and for a project the second call an agent actually makes is
`list_areas`.

**2. The roster lists every active project, alphabetically — no ranking, no
category exclusion.** #674 specified top-N by trailing work-item count,
excluding `category = 'Fun'`. Both fall out once the review ruled that the
roster carries **names only**:

- Ranking existed to decide which projects deserved scarce *description* tokens.
  With names at ~4 tokens each and 27 projects fitting whole, nothing is being
  rationed — and a complete roster cannot cause the misroute an omitted project
  can. Alphabetical is also stable across sessions with no window to tune.
- The `Fun` exclusion's stated reason was that "a fun project churning for a
  week shouldn't evict a daily driver" — eviction from ranked slots. No slots,
  no eviction, and `hv-simulator`, `kapollo` and `mortars` receive real work
  items. Hiding them would produce precisely the guess this replaces.

**3. `inactive` was dropped from the status vocabulary too** (Ken's call), not
just `maintenance`. After the last three inactive projects were archived on the
ruling that they are not work-item targets, `inactive` inherited exactly the
description that justified dropping `maintenance`: zero rows, no behaviour
distinct from `archived`. Status is now `active | archived`.

**4. The default corpus is `active`, not `active + inactive`.** The review's
principle — *the default must include every row that could be a correct routing
answer* — is unchanged and is what produces this. Its worked example for
including inactive rows was "resume `trt-llm-explore`", and `trt-llm-explore` is
one of the three that were archived precisely so it stops being a target.

## The rehearsal caught a migration that would have failed on any un-fixed database

0020's postcondition refuses to leave rows outside the vocabulary the app is
about to enforce — 0018's lesson, applied again. Rehearsed against the morning's
nightly dump, it **failed with `off_vocab=3`**.

Not a false alarm, and not a problem with production: the dump was taken at
03:17, hours *before* Ken hand-moved the last three `inactive` projects to
`archived`. Production would have migrated cleanly by luck. **Any database
restored from an older backup, and every dev copy, would have crash-looped the
container on startup.**

So 0020 converts before it asserts, and the mapping preserves each value's
actual prior behaviour rather than collapsing both to one side:

- `maintenance` → `active`. It was rail-visible by default (WI #246 showed
  active + maintenance), i.e. a live project in a quiet phase.
- `inactive` → `archived`. It was rail-hidden, and Ken's ruling is explicit that
  formerly-inactive projects are not targets for new work.

Re-run against the same dump, the migration reaches 27 active / 8 archived
unaided — the state production had only because someone did it by hand.

## `notes`, and the cap that bit its own exemplar

`description` is now a routing contract capped at 160 characters, enforced both
app-side (so the caller gets `invalid_input` naming the overage, not a raw
constraint violation surfacing as `internal`) and by a CHECK constraint. The
long form moves to a new unbounded `notes` column, returned by `get_project` and
REST and never by the lean list or the roster.

`notes` exists because capping alone would have destroyed real content. The
review predicted four over-length rows; production has **six**:

| project | chars |
|---|---|
| `klams-mind` | 528 |
| `kprojects` | 437 |
| `agent-skills` | 333 |
| `klams-view` | 311 |
| `kyac` | 190 |
| `homelab-health` | 183 |

The last one is the finding. The review named `homelab-health` *"the corpus's
best row"* — its unprompted worked example of the contract being defined — and
at 183 characters it fails that contract's cap. Not treated as grounds to raise
the cap: it trims to ~130 without losing either clause, so 160 demands concision
rather than forbidding the form.

The transient is real and accepted: active projects with a null `description` go
from 11 to 17 until the data pass runs. The prose is not lost — it is in `notes`
on the same row, which is what `notes` is for, and `homelab-health`'s original
text is the model the backfill should start from.

## Verification

`just check` green.

**Against the real corpus** — the 2026-07-31 nightly dump in a throwaway
`postgres:18-alpine`, with a local `korg-api` on :8092. Never production.

- **Roster**: 27 names + one pointer line = **427 chars, ~112 tokens**. Over the
  review's ~100 budget, and reported rather than rounded down: the names are
  ~87 tokens of it and are irreducible. Compare the old always-pay-on-demand
  cost below.
- **`list_projects` payloads**: default **1,910 chars** vs **10,090** for
  `status:"all", detail:"full"` — a **5.3×** reduction, better than the review's
  estimate of 800–900 tokens. The field projection is the saving, as predicted;
  the row filter is the smaller half.
- **Defaults behave**: 27 items, `omitted:{archived:8}`, and no row carries a
  `status` key. Under `status:"all"`: 35 items, `omitted:{archived:0}`, and
  exactly 8 rows carry `status`.
- **`detail:"full"`** returns all ten columns including `notes` — #758's single
  call rather than 35 `get_project`s.
- **`get_project("homelab-health")`** returns the full row plus its two areas
  (`kai`, `vmlab-01`), with the exemplar prose intact in `notes`. An unknown name
  returns `{"code":"not_found"}`.
- **The cap enforces**: 161 characters is rejected with
  `invalid_input … caps it at 160. Put the long form in "notes"`; 160 saves.
- **The dropped vocabulary enforces**: `status:"inactive"` is rejected with
  `expected one of: active, archived`.

Four repo guard tests fired during the work and all four were right — the
tool-count assertions, `every_advertised_tool_is_dispatched` (no fixture for
`get_project`), `collection_reads_return_the_shape_the_instructions_promise`
(`list_projects` no longer bare), and the `docs_drift` suite (api.md's shape
table, its tool catalogue, README's count). The instructions now classify three
shapes rather than two, and the guard reads all three.

## Deploy

`deploy-kubsdb` after merge. **0020 is a mild boundary**: an old image ignores
`notes` happily, but it renders the six moved rows blank. Reversal is
`UPDATE project SET description = notes, notes = NULL WHERE description IS NULL
AND notes IS NOT NULL`, plus dropping `project_description_routing_line` — not a
dump restore.

Expect `migrations` 19 → 20 and two NOTICEs in the container log: the `kcard`
line from 0019, and 0020's list of the six moved descriptions.

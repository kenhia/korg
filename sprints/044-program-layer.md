# 044 — The program layer: `program` node + awaiting-Ken marker

**Proposal:** korg:972 (rank 4.5) · **Work items:** #968 (feature, L), #969 (feature, S)
**Branch:** `044-program-layer`
**Context:** kfdc Phase 0, slice 3. Handoff korg:974 ("kfdc kickoff") carries the
standing decisions; the relevant one is **#3: proposals stay single-project;
programs are the cross-project layer (a program covers proposals, ordered).**
Sequenced after 971/sprint 043, which cleaned the corpus first. The board rollup
read (#970, proposal korg:973) consumes both halves of this sprint — see
"What 973 can rely on".

## Goal

Two additive data-model changes, bundled because they touch the same
node/edge/MCP stack and both exist to feed kfdc panels:

1. **#968 — `program` node type.** The sanctioned home for "this piece of work
   spans several repos". A program covers *proposals*, ordered. This makes
   first-class what already exists as markdown program plans in this very
   repo — `sprints/planning/2026-07-31-project-routing-program.md` says it
   outright: *"The proposal is the korg-engine scope; this document is the
   program."*
2. **#969 — an "awaiting Ken" marker.** Nothing in korg can say "this moves only
   when Ken acts", which is the most valuable lane on the kfdc board
   (Commander's Call).

## The measurement (production, 2026-08-05)

Run against kubsdb before any decision, per the 043 precedent:

| | |
|---|---|
| nodes | 884 (workitem 672 · sprint_proposal 138 · card 30 · report 26 · handoff 7) |
| live proposals | **16**, across **10 projects** |
| cross-project `covers` edges | 38 (the residual 043 left; 4 proposals live) |
| nodes carrying any tag | 672 of 884 (**76%**) |

**Live queue by project** — korg 4 · hv-simulator 2 · kapollo 2 · k-homelab 2 ·
agent-skills, claude-cleo, klams-view, kmon, kprojects, kyac 1 each.

### What the four live cross-project proposals actually span

These are the demand signal 043 handed forward. Every one of them is a program
wearing a proposal's clothes:

| proposal | filed under | spans (items per project) |
|---|---|---|
| 818 eval/drill traffic indistinguishable in three stores | korg | kagent 1 · klams-mind 1 · korg 1 |
| 819 ansible-k archived, took observability with it | k-homelab | hv-simulator 1 · k-homelab 4 |
| 820 finish klams-view | klams-view | klams-view 3 · kwebi 1 |
| 822 weekend LLM sidecar, two consumers | korg | korg 1 · kvllm 1 |

818 is the clearest case: three projects, **one item each** — three one-item
proposals under one program, not one proposal that lies about its project.

### The tag vocabulary already tried to express awaiting-Ken, and failed

Free-form tags in use include `decision` (1 node) and `needs-decision` (1 node)
— two spellings, one concept, two rows. Somebody reached for exactly this
mechanism and it did not converge. That is evidence *for* the feature and
*against* implementing it as a tag convention.

## Decisions

### D-1 — A new label `includes`, not an extension of `covers`

`covers` is declared `left_kind: sprint_proposal → right_kind: workitem` with
`same_project: true` ([relationships.rs:44-51](crates/korg-core/src/relationships.rs#L44-L51)).
Extending it to also mean program→proposal requires making both endpoint fields
multi-valued **and** making `same_project` conditional on which pair matched —
i.e. one registry entry with two meanings, which is precisely what the registry
exists to prevent. Sprint 043 put `same_project` on the entry rather than
special-casing `relate()` for exactly this reason: "the rule sits where a reader
already looks for *what does this label mean*."

So: a fifth… sixth registered label.

```
includes | directed | program **includes** proposal | program → sprint_proposal | no
```

**`right_kind` is pinned to `sprint_proposal` in v1.** #968 says the edges point
at proposals "occasionally standalone WIs". The registry can express one kind
per end, and all four demand-signal cases are proposals, so v1 takes the
narrower, validated shape. Widening later means generalizing `right_kind` to a
kind *set* — a small, mechanical change to five entries — and that is cheaper
than shipping an unvalidated `right_kind: None` now and never tightening it.

### D-2 — Slice order lives on the edge (`relationship.rank`), not on the program

Three candidates:

| where | why not |
|---|---|
| a `program_slice` join table | abandons the generalized-edge model; 0008 explicitly refused a join table for `covers` |
| reuse `sprint_proposal.rank` | that is the *global queue* rank — a different axis, and shared by every program |
| **nullable `rank NUMERIC` on `relationship`** | chosen |

The decisive argument: position is a property of *the containment*, not of the
proposal. A proposal that appears in two programs has two positions, and only an
edge column can hold both. It is also the same drag-orderable `NUMERIC` idiom as
`card.rank` and `sprint_proposal.rank`, so the web UI already knows the pattern.

Cost is one nullable column on `relationship` (450+ `covers` rows keep `NULL`,
no backfill). Ordering is `rank NULLS LAST, right_id` so an unranked slice is
stable rather than random.

### D-3 — Awaiting-Ken is two nullable columns on `node`, not a reserved tag

A reserved tag is the smallest mechanism, and it is wrong here — for a reason
that only shows up in the write path:

> **Tags are written wholesale.** `update_work_item`, `update_proposal` and
> `set_node_tags` all do `UPDATE node SET tags = $2`
> ([repo.rs:1021-1026](crates/korg-core/src/repo.rs#L1021-L1026)). An agent
> editing tags for an unrelated reason and echoing back the array it read a
> minute ago silently clears the marker. 76% of nodes carry tags, so that is a
> hot path, and the failure is invisible: the item just quietly leaves the lane
> Ken is supposed to be looking at.

For the board's most important lane, a marker that can be destroyed as a
side-effect of an unrelated write is disqualifying. The corpus agrees — the two
existing spellings never converged.

So, on `node`, beside `archived` — 0001's stated design is that cross-cutting
attributes live on `node` "so they are shared by every kind", and a work item, a
proposal **and a program** can all be waiting on Ken:

```sql
awaiting_since TIMESTAMPTZ,   -- NULL = not awaiting. Non-null = awaiting, and carries age.
awaiting_note  TEXT,          -- what is being asked. Only meaningful with awaiting_since.
CONSTRAINT node_awaiting_note_needs_since CHECK (awaiting_note IS NULL OR awaiting_since IS NOT NULL)
```

A timestamp rather than a boolean because the lane's whole value is "this has
been waiting nine days" — free with `awaiting_since`, an extra column with a
boolean. The note is what makes the lane actionable without opening every row;
it is the one piece here that could be cut, and it is kept deliberately.

**Column names carry no person.** korg is single-user, but `awaiting_ken_since`
would bake an identity into the schema for no gain. "Awaiting" means "awaiting
Ken" in this instance and the docs say so.

### D-4 — Program statuses: `active` · `holding` · `done`

#968's list, adopted as-is. Partitioned the way `vocab.rs` requires of every
status set: **live** = `active` + `holding`, **terminal** = `done`, with a test
fencing the partition so a fourth value cannot be added without deciding which
side it falls on. Declared as a `CHECK`, not a Postgres `ENUM` — 0008 used an
enum for proposals and 0010 moved to a CHECK for reports; korg-core is the
authority and the DB is the backstop, and a CHECK is extensible.

### D-5 — `get_program` rolls up; nobody crawls

`get_program(node_id)` returns the program fields plus `slices` — ordered, one
entry per covered proposal, each carrying the proposal's `node_id`, `title`,
`status`, `project`, `covered_count`, and its **work-item status rollup**
(open/resolved/done/closed counts). A consumer renders a program without a
second call. This is the shape #970 was blocked on.

### D-6 — A program carries **no** project; its span is derived from its slices

*(Raised by Fable's design review on korg:972, 2026-08-05. It is the decision the
first draft of this plan silently skipped, and skipping it would have rebuilt the
exact lie the layer exists to kill.)*

`node.project_id` is nullable (0001), so a program *could* be filed under a
project — and the 818 case shows what that produces: work in kagent, klams-mind
and korg, filed "korg", because the writer had to pick one. A program whose
project is a single name is a proposal that got promoted without fixing
anything.

So programs carry `project_id IS NULL`, **enforced**, symmetric with 0022's
`node_sprint_proposal_has_project`:

```sql
CONSTRAINT node_program_has_no_project CHECK (kind <> 'program' OR project_id IS NULL)
```

- `create_program` **does not accept** a project or `project_id` — passing one is
  `invalid_input` naming this rule, not a silent drop.
- `get_program` and `list_programs` return a derived **`span`**: the distinct
  project names of the covered proposals, ordered, with a count. That is the
  honest answer to "which repos does this touch", and it stays correct when a
  slice is added.
- Any project-scoped view finds a program through its slices, never through its
  own row.

A single-project program stays legal (this repo's own agent-surface program is
one) — its `span` is simply one name. The constraint is about where the fact
*lives*, not about how wide a program may be.

### D-7 — The marker's lifecycle: cleared by Ken acting, and never haunting the lane

An `awaiting_since` that outlives its node turns the Commander's Call lane into a
graveyard — D-3's failure mode one door over. Two layers:

**The read never shows ghosts.** `list_awaiting` filters to unarchived nodes
whose own status is non-terminal, so even a marker that escaped the write rules
cannot haunt the board.

**The write rules clear it when Ken has, by definition, acted.** The trigger is
*not* "terminal" — it is **"a state only Ken sets"**:

| transition | clears? | why |
|---|---|---|
| work item → `closed` | **yes** | `vocab.rs` says `closed` is "Ken only" — if Ken closed it, the ask is answered |
| work item → `resolved` / `done` | **no** | "implemented, may still need a user test" is *the* canonical awaiting-Ken state; clearing here would delete the lane's best rows |
| proposal → `done` / `declined` | **yes** | both are Ken's calls on the bundle |
| any node → `archived` | **yes** | the ask is moot |

Getting this backwards — clearing on `resolved`/`done` — would have emptied the
lane of exactly the items it exists to show.

### D-8 — `set_awaiting` sets *and* clears, and re-setting does not reset the clock

`set_awaiting(node_id, awaiting: bool, note?)`, any node kind:

- `awaiting: false` clears both columns. An agent that raised a question and got
  its answer mid-session **retracts its own marker** — without this, every
  resolved ask waits on a click from Ken, and the lane fills with questions that
  are already answered.
- `awaiting: true` on a node already awaiting **preserves the original
  `awaiting_since`** and updates only the note. The age is the entire point of
  D-3's timestamp; a re-set that restarts the clock would make a nine-day-old
  ask look fresh every time an agent touched it.
- The web UI's one-click clear is the same core call, not a second path.

### D-9 — Re-relating with a rank reorders in place

`relate()` dedups an exact duplicate via `ON CONFLICT … DO UPDATE SET left_id =
relationship.left_id` — a deliberate no-op that preserves `created`/`origin`
(D-17). With a rank in play that leaves no reorder path short of
`unrelate` + `relate`, which churns provenance to move a slice up one position.

So the conflict clause becomes `SET rank = COALESCE(EXCLUDED.rank,
relationship.rank)`:

- re-relate **with** a rank → reorders in place, provenance intact;
- re-relate **without** one → still a no-op, so an unrelated re-relate cannot
  silently wipe a slice's position.

Documented in `relate`'s tool description, since it is behaviour an agent has to
be able to predict.

## What 973 (#970, board rollup read) can rely on

Written down here because 973's shape depends on it and it is the coordination
surface between the two sprints:

- `list_awaiting()` — the lane, one call, any node kind, ordered oldest-first,
  each row carrying `node_id`, `kind`, a resolved display title, `project`, the
  node's own **`status`** (resolved per kind, so a consumer renders state without
  a follow-up read), `awaiting_since` and `awaiting_note`. Ghost-free by D-7.
- `list_programs()` — `{items, omitted}`, live-by-default, same shape rule as
  `list_proposals`; each row carries the derived `span` (D-6).
- `get_program()` — the D-5 rollup, plus a `related` block like the other focused
  reads. A program is precisely the kind that accrues a handoff, and LB-3 says a
  `has_handoff` ref must surface where the reader already is.
- The `includes` label is stable and directed program → sprint_proposal, and
  re-relating with a rank reorders in place (D-9).

## Build order

1. **Migration 0023** — `program` in `node_kind_check`; `program` detail table;
   `node_program_has_no_project` CHECK (D-6); `relationship.rank`;
   `node.awaiting_since` / `node.awaiting_note` + CHECK. Purely additive, so a
   re-tag reverts it; header states that.
2. **`vocab.rs`** — `PROGRAM_STATUSES` + live/terminal split + partition test.
3. **`relationships.rs`** — the `includes` entry + registry tests.
4. **`repo.rs`** — program CRUD, the D-5 rollup, `set_awaiting` / `list_awaiting`,
   `rank` accepted on `relate`.
5. **`korg-mcp/tools.rs`** — 6 tools (`create_program`, `get_program`,
   `list_programs`, `update_program`, `set_awaiting`, `list_awaiting`); `relate`'s
   description gains `includes`.
6. **`korg-api`** — REST routes for the same.
7. **`just gen`** — ts-rs bindings + MCP schema snapshot.
8. **`web/`** — `/programs` list + detail (minimal; kfdc is the real consumer),
   and a one-click clear for `awaiting` wherever the marker shows.
9. **Docs** — `docs/api.md` (tool catalogue, registry table, a Programs section,
   the awaiting contract) + README tool count 50 → 56. `docs_drift` fails
   otherwise, including its `CATEGORIES` const, which needs "Programs".
10. **Tests** — `crates/korg-core/tests/sprint044.rs`.

## What shipped

### Migration 0023 — additive, five things

`program` in `node_kind_check`; the `program` detail table (`title`, `aim`,
`notes`, `status`, `rank`, `pinned`, CHECK-based status like 0010 rather than
0008's enum); `node_program_has_no_project`; `relationship.rank` (nullable,
no default, so all 450 `covers` edges read identically); `node.awaiting_since` /
`awaiting_note` + their CHECK and a partial index.

**Rehearsed on kubsdb, 2026-08-05** — and the first thing the rehearsal found
was that **the nightly dump was the wrong starting state**. `korg-20260805-031756.sql.gz`
was taken at 03:17, *before* the 043 deploy applied 0022; it contains no
`node_sprint_proposal_has_project`. Rehearsing 0023 on it would have exercised a
v21 corpus and told us nothing about the one production actually has. A fresh
`pg_dump` of live (migrations 22, constraint present) was taken instead.

```
baseline    nodes 884 · workitems 672 · proposals 138 · cards 30 · reports 26
            · handoffs 7 · links 4 · topics 1 · projects 39 · edges 608
            · covers 450 · comments 393 · node 1..976 · migrations 22
apply       clean, no NOTICE — it is pure DDL, there is nothing to backfill
after       every count above identical, node_min/node_max unchanged
new         program table · relationship.rank · node.awaiting_since/_note
            · both CHECKs · node_awaiting_idx · 'program' in node_kind_check
            0 programs, 0 ranked edges, 0 awaiting rows — nothing invented
re-apply    every guard fired (IF NOT EXISTS / pg_constraint probe), counts
            unchanged, still exactly 2 new constraints
```

Both new constraints were then made to bite on the live corpus, not just
asserted to exist: an `INSERT` of a program **with** a project raised
`check_violation`; one **without** was accepted (and rolled back); an
`awaiting_note` with no `awaiting_since` raised `check_violation`.

### The write rules (`korg-core`)

- `create_program` refuses `project`/`project_id` with an error naming the rule
  and pointing at the derived `span` — a refusal, not a drop, because a caller
  who passed one believes the program is filed somewhere.
- A slice that is not a proposal is refused outright, resolved **before** the
  transaction opens so a refusal leaves nothing behind.
- `relate()` grew a `rank`, and its `ON CONFLICT` clause — a deliberate no-op
  since LB-1 — now does exactly one thing: `rank = COALESCE(EXCLUDED.rank,
  relationship.rank)`. Reorder in place with a rank; no-op without one.
- `settle_awaiting()` runs at the end of `update_work_item`, `update_proposal`
  and `update_program`. Centralised there rather than at each status branch, so
  it is correct regardless of *which* field triggered the transition, and in
  core rather than a trigger (LB-2).

### The surfaces

Six MCP tools (`create_program`, `get_program`, `list_programs`,
`update_program`, `set_awaiting`, `list_awaiting`) — 50 → **56**. Matching REST
routes. `related_context` learned the `program` kind so a program's title
resolves in anyone else's `related` block.

Web: `/programs` list + `/programs/[node_id]` detail, and `AwaitingLane` on
Today. The detail page renders from **one** call — that is the D-5 rollup
justifying itself, since the pre-044 way would have been program → neighbors →
`get_proposal` per slice → work items per proposal. The lane renders **nothing**
when empty: an "awaiting you" panel that shows an empty state every day is one
Ken learns to skip, and this one has to be read on the days it has rows.

### Tests

`crates/korg-core/tests/sprint044.rs`, 24 tests. The ones that pin a decision
rather than a behaviour:

- `an_unrelated_tag_write_does_not_clear_the_marker` — the D-3 argument, executable.
  If the marker were the reserved tag #969 guessed at, this fails.
- `resolved_and_done_keep_the_marker_but_closed_clears_it` — D-7, the one that is
  easy to get backwards. Clearing on `resolved`/`done` would empty the lane of
  its best rows.
- `re_relating_with_a_rank_reorders_in_place_and_keeps_provenance` and its
  no-rank twin — both halves of D-9, asserting `created`/`origin` survive.
- `includes_is_deliberately_cross_project` (in `relationships.rs`) — a guard
  against a future tidy-up giving `includes` `same_project: true` "for symmetry
  with `covers`", which would leave no way to express a multi-repo program at all.

Plus REST sweep coverage for both new surfaces, and fixture/count updates in the
drift guards. The `relate` signature change touched 28 test call sites.

## Not in this sprint

- **No backfill of the four cross-project proposals.** Converting 818/819/820/822
  into programs means *splitting* each into per-project proposals, and where the
  split falls is a scope judgement, not a rule a migration can apply. 043 set the
  precedent by leaving its residual to Ken explicitly. The layer ships; the recipe
  gets written down; the four stay as they are until Ken says otherwise.
- **No enforcement that a program's slices differ in project.** A program over
  one project's proposals is legitimate (this repo's own agent-surface program is
  exactly that).
- **No LLM anywhere near it** — standing decision #1 from handoff korg:974.

## Coordination

Two-pane protocol (`sprints/planning/2026-08-korg-agent-surface.md` §7) is in
force: this session is the **worker** and owns the branch. **Explicit paths only
on `git add`** — never `-A`, `.`, or `commit -a`.

---

## Log

*(written as the sprint runs)*

- **2026-08-05** — Branched, measured production, decisions D-1…D-5 recorded above.
- **2026-08-05** — Design review from Fable on korg:972 (comment 410). Four gaps
  closed as **D-6** (a program carries no project — the decision the draft
  skipped), **D-7** (marker lifecycle vs terminal/archived), **D-8** (`set_awaiting`
  clears too, and re-setting preserves the clock) and **D-9** (re-relate with a
  rank reorders in place). Both minors taken: `get_program` carries `related`,
  `list_awaiting` rows carry the node's own status.
- **2026-08-05** — Built in the order above. `just check` green. Migration
  rehearsed against a **fresh** dump after the nightly turned out to predate the
  043 deploy; see "What shipped".

# 043 — Proposals are single-project, enforced

**Proposal:** korg:971 (rank 4) · **Work item:** #967 (bug, M) · **Branch:** `043-single-project-proposals`
**Context:** kfdc Phase 0, slice 2. Handoff korg:974 ("kfdc kickoff") carries the
standing decisions; the relevant one is **#3: proposals stay single-project;
programs are the cross-project layer.**

## Goal

"One proposal, one project" was a convention the `refill-queue` and
`start-sprint` skills happened to follow. Nothing in the schema or the write
path said so, which is why Ken kept opening a sprint he thought was one repo and
finding another project's work items in it. Three things, from #967:

1. Require a project on every new proposal.
2. Refuse a `covers` edge whose two ends are in different projects, with an
   error that names both projects and points at the program layer.
3. Backfill the legacy proposals a rule can resolve.

## The measurement moved — re-run before building

#967 quotes the 2026-07-23 linking-layer review: *33 cross-project `covers`
edges, 32 of them artifacts of early project-less proposals, 6 mechanically
backfillable, exactly 1 real.* That number is two weeks and one migration stale,
so it was re-run against the deployed instance before anything was written
(2026-08-05):

| | 2026-07-23 (in #967) | 2026-08-05 (measured) |
|---|---|---|
| cross-project `covers` edges | 33 | **52**, across 17 proposals |
| project-less proposals | 7–11 | **0** — 0016 §5 already fixed these |
| mechanically resolvable | 6 | **4** (14 edges) |
| genuinely ambiguous | 1 (#175) | **13**, four of them *live in the queue* |

The shape of the problem changed, not just the size. 0016 §5 backfilled every
proposal with **no** project; what is left is a different class — proposals with
the *wrong* project (five pre-project-model bundles under the retired
`Agent-Plan` umbrella, one kprojects proposal under `claude-cleo`) — plus a
growing tail of proposals that genuinely span repos, which is the demand signal
for the program layer rather than a data defect.

So the sprint's third item is smaller than #967 estimated and its residual is
much larger. That is reported, not hidden: see "What is left" below.

## What shipped

### The write rules (`korg-core`, the one path both transports share)

- **`create_proposal` refuses without a project** — `invalid_input` naming both
  selectors and saying why a proposal needs one, not a bare rejection.
- **`covers` is single-project.** Declared on the registry entry as
  `same_project: true` rather than special-cased in `relate`, so the rule sits
  where a reader already looks for "what does this label mean" and
  `docs/api.md`'s registry table can carry a column for it. `relate()` consults
  the flag; `create_proposal`, whose bundled insert does not go through
  `relate()`, applies the same check before its transaction opens.
- The refusal names **both** projects, the offending work item by `wi_number`
  and title, and the program layer (#968) as the sanctioned path — proposal
  971's notes asked for a refusal that teaches the workflow.

Two deliberate calls:

- **A cross-project work item is a hard refusal, not a drop.** `propose_sprint`
  drops `wi_number`s that do not resolve (F-06, echoed back in `covered`). A
  number that *does* resolve but lives elsewhere is a different class: silently
  omitting the item Ken meant to include is the exact failure #967 exists to
  stop, so the whole create is refused and nothing is written.
- **An unfiled work item (no project) is not refused.** It is unfiled, not filed
  elsewhere, and refusing it would be a stricter rule than #967 asked for.
  Production holds zero project-less work items, so the hole is currently empty;
  `crates/korg-core/tests/sprint043.rs` pins the choice as a test rather than
  leaving it an accident of the SQL. If it ever fills, tightening is one `if`.

`depends_on` explicitly keeps *no* project rule — the homelab-ai plan's whole
structure is cross-repo dependencies. A registry unit test asserts `covers` is
the only label carrying the flag.

### Migration 0022

Backfills by **rule**, not by an id list — 0016's rehearsal is the reason: a
snapshot's ids are coupled to the instant they were read, while "a proposal
whose covered work items are unanimously in one project moves to it" converges
any snapshot to the same end state. Adds
`node_sprint_proposal_has_project` (CHECK, `kind <> 'sprint_proposal' OR
project_id IS NOT NULL`), valid immediately.

No constraint on cross-project `covers`: it would have to be a trigger (the two
projects live in two `node` rows), and LB-2 already settled that edge rules live
in core, never in a trigger that can drift from it. Thirteen legacy violations
mean it could only be `NOT VALID` anyway.

**Rehearsed** against `korg-20260805-031756.sql.gz` restored into
`korg_rehearse` on kubsdb, per `docs/operations.md`:

```
baseline   proposals 138 · projectless 0 · covers 450 · cross-project 52 · max migration 21
apply      UPDATE 4
           NOTICE: 13 legacy proposal(s) still cover work across projects, 4 of them live
after      proposals 138 · projectless 0 · covers 450 · cross-project 38 · nodes 884 · workitems 672
re-apply   UPDATE 0            (idempotent)
constraint INSERT INTO node (kind) VALUES ('sprint_proposal') -> rejected
```

The four moves, with `updated` preserved (the touch trigger is disabled across
the backfill — a mis-file correction is not a content edit):

| | from | to | status |
|---|---|---|---|
| 599 korg dogfood fixes | Agent-Plan | korg | done |
| 601 kapollo core UX | Agent-Plan | kapollo | done |
| 602 hv-simulator dashboard pass | Agent-Plan | hv-simulator | done |
| 747 kprojects installable anywhere | claude-cleo | kprojects | declined |

Only `cross_project_covers` moved (52 → 38). Every other count is identical.

### Test-corpus churn

Requiring a project broke ~32 tests across 11 files, all of them building
project-less proposals. `korg-test-support` grew `TEST_PROJECT` / `test_project`
and `new::proposal_in`, and `new::proposal` now names `TEST_PROJECT` — so an
affected suite gained one line (`test_project(&pool).await`) rather than a
routing story it has no opinion about. `korg-api`'s `app_with_pool` seeds it
directly; a REST suite should not have to POST `/api/projects` before it can
test anything else.

Two suites needed real thought rather than a rewrite:

- `contract.rs`'s `?project=` filter test used an *unassigned* proposal as its
  contrast case, which is now impossible — it contrasts two projects instead.
- `sprint036.rs` seeds proposals straight into the tables to test 0021 against a
  pre-0021 corpus; those raw inserts now carry a project, which is what the
  production rows they stand in for had by then anyway.

## What is left (Ken's call, not the migration's)

Thirteen proposals still cover work across projects. Four are **live in the
queue** as of 2026-08-05:

| | project | spans |
|---|---|---|
| 818 | korg | kagent, klams-mind, korg |
| 819 | k-homelab | hv-simulator, k-homelab |
| 820 | klams-view | klams-view, kwebi |
| 822 | korg | korg, kvllm |

The other nine are done/declined history (175, 485, 600, 603, 626, 654, 689,
698, 718) and are best left as the record of how those sprints actually ran.

Nothing breaks: enforcement is write-side, so an existing edge stays readable
and the proposals still start. What they cannot do is *grow* another
cross-project edge. The honest resolution for the live four is either a split or
the program layer — which is proposal **korg:972**, the next slice of kfdc Phase
0, and this is its demand signal.

## Follow-ups

- **korg:972** (program layer) is now sequenced correctly: the corpus is as
  clean as a rule can make it, so programs arrive to a known residual of 13.
- A project-less *work item* is still creatable and still coverable. Zero exist;
  if that changes, the same rule tightens by one condition.
- The agent-skills repo still says `cn_path` where korg's field is `src_path`
  (noted in handoff korg:974, not this repo's to fix).

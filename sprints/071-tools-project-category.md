# 071 — the `Tools` project category

Proposal korg:1506. Covers #1505 (XS).
Branch `071-tools-project-category`.

## Goal

#1505, in full: *"Categories are code changes, let's add 'Tools'."*

The work item is one sentence and the sprint is one value, but the
sentence is doing two things. The second clause is the request. The
first is Ken naming, correctly, the reason it has to be a sprint at all:
`project.category` is a closed vocabulary (WI #678), korg-core is its
only enforcement, and there is no admin surface that adds one. So the
interesting question was never *what does this cost* — `vocab.rs` says,
in a doc comment, exactly what it costs — but whether that comment is
still true. It was not, quite.

## What the doc comment claimed, and what the path actually is

`PROJECT_CATEGORIES`' comment promised: *"one edit here plus its hue in
`CATEGORY_HUE` in `web/src/lib/domain.ts`, then `just gen`."*

Walking it turned up five steps, not three:

| step | forced by | comment said |
|---|---|---|
| the value in `vocab.rs` | — | yes |
| `CATEGORY_HUE` | `Record<ProjectCategory, number>` stops compiling | yes |
| `just gen` (generated vocab + MCP schema snapshot) | `gen-check` | yes |
| `docs/usage.md` ×2 | `docs_drift` | **no** |
| `RAIL_ORDER` | **nothing** | **no** |

The docs omission is benign — the fence fails loudly and names the file,
so the cost is one red test, not a wrong doc. Both failures were taken
first, before the docs were touched, which is the whole evidence that
the fence covers this path:

```
`AI | Dashboard | EVAL | Fun | Infrastructure | Ops | Other` reads as a
vocabulary enumeration but matches no vocabulary in vocab.rs
```

**`RAIL_ORDER` is the one that matters**, because nothing forces it and
the fallback is wrong rather than absent. `CATEGORY_ORDER` appends a
category missing from `RAIL_ORDER` instead of dropping it — deliberate,
and the right call — but it appends it *last*, below `EVAL`. `EVAL` is
last for a stated reason ("it exists to make an eval-sandbox leak
findable, not to be navigated to"), so the silent default hands a new
category the one slot the list's own logic reserves for the thing you
are least meant to reach for. A new category ships looking deliberately
de-prioritised and nothing complains. The comment now says so.

## Decisions

- **Hue 110**, by the rule `EVAL` established: the midpoint of the widest
  remaining gap on the wheel. That was 64 (`Infrastructure`) → 157 (`AI`),
  at 93° much the largest. Keeping to the rule is what stops a rail
  scanned by colour from degrading one arbitrary pick at a time.
- **Third in the rail, after `Ops`** — Ken, mid-sprint, when the first
  draft had it there on my reasoning and he confirmed it as the
  requirement: *"In the project lists (Work Items and Planning pages),
  Tools should appear after 'Ops'."* Worth recording that both pages
  were named, because they get it from one list: Work Items and Planning
  both group through `CATEGORY_ORDER`, so the placement is decided once.
  That is now in `RAIL_ORDER`'s comment, since the next person to place a
  category will otherwise wonder which page they are editing.
- **Alphabetical in `vocab.rs`, so `Tools` sorts last** — after `Other`,
  not after `Ops`. The proposal's notes said "after `Ops`" and were
  wrong; the array is alphabetical and `Ops < Other < Tools`. Two orders
  coexisting is not an accident to tidy up: the vocabulary order is what
  the MCP schema enum, the generated TS tuple and the edit panel's select
  inherit, where alphabetical is what a reader scanning for a value
  wants; the rail's order is Ken's by-frequency one. Both comments now
  name the other.
- **What `Tools` means**: the homelab's own instruments, as against
  `Infrastructure`, which is what they run on. Written into both
  `vocab.rs` and `docs/usage.md`, because a category with no stated
  boundary is how `tooling`/`infra` drifted before #678 closed the set.

## Deliberately not done

- **No migration.** `project.category` has been nullable `TEXT` with no
  `CHECK` since 0011 — korg-core is the only enforcement. 0018 was
  data-only, and its `NOT IN (...)` is a point-in-time postcondition over
  the values that existed in July, not a live constraint; widening the
  vocabulary invalidates nothing it asserted. Schema count unchanged at
  30, so this deploy's rollback stays a clean re-tag.
- **No project re-categorised.** Which projects become `Tools` is a data
  edit through `update_project` or the edit panel's select, and it is
  Ken's call — the vocabulary change is the code half and is complete
  without it. The category ships usable and empty; an empty category
  renders no rail group, so nothing looks broken in the meantime.

## The cross-project angle

korg is in `cross-project-planning/index.md` → `korg+/`, so the plan was
read before starting. Nothing to amend, and the reason is worth keeping:
**GP-13 and GP-14 anticipate this exact change.** GP-13's consumer half
says *"where korg emits a literal the consumer has no treatment for,
render neutral"*, resting explicitly on the fact that *"korg owns the
vocabulary and grows it between deploys — the consumer will meet an
unrendered literal in production, by design."*

Since sprint 068 (#1414), `category` rides `PlanningRollupRow`, so kfdc
and korg-dash do see project categories. They will meet `Tools` with no
row for it, and under GP-13 that is correct behaviour on both sides, not
a coordination the plan owes. No consumer needs to ship anything, and no
GP-n is contradicted or refined — so, per the amend rule, no edit.

## Verification

`just check` is the entire gate, and every step above has a fence in it:
`gen-check` for a missed `just gen`, `svelte-check` for a missed hue,
`docs_drift` for either docs line. `sprint010.rs` iterates
`PROJECT_CATEGORIES` and round-trips every value through
`update_project`, so `Tools` picked up write-path coverage for free —
including the assertion that an off-vocabulary value is still rejected
and leaves the prior value alone.

# Sprint 031 — `cn_path` → `src_path`

Proposal `korg:816`, **not** closing it out. This is Phase A of a program that
spans two korg branches and two external sessions — see
`sprints/planning/2026-07-31-project-routing-program.md` for the whole shape, and
**do not let `sprint-ship` mark the proposal `done` when this merges.**

Covers **#675**, narrowed at sprint start to its korg-engine half: the rename,
the semantics, and the value fixes that need no judgement. Everything requiring a
look at a project's actual source moved to the one pass that visits each project
once.

## Why this slice ships alone

The proposal's remaining four WIs are all gated on #757's field review, which is
a deliberately fresh-session job. #675 is the only part the review explicitly
excludes ("already decided, do not re-litigate"), so it is the only part that can
move today.

Shipping it separately is also the safer order on its own terms. This change
touches a migration, two published contract artefacts and the web client at once.
If its blast radius has a miss, that is worth finding in isolation rather than
tangled with a roster and a new read surface.

## The rename is a semantics fix

A bare path means nothing without a host. Once `machines` and `deploy_to` split
development from production per project, `cn_path` never said which one it
described — and nothing in the schema said either, which is arguably what let the
field drift in the first place.

`src_path` answers it by construction: **the working copy on the project's
development machine.** korg's own row is the worked example — `machines: ["kai"]`,
`deploy_to: ["kubsdb"]`, `src_path` is the kai checkout.

That sentence is now in three places an agent actually reads: the `ProjectPatch`
field doc (which schemars renders into the `update_project` tool schema), the
`ProjectRow` field doc (which ts-rs renders into `korg.ts`), and `docs/api.md`.
Writing the convention down is half of #675 — the other half was never going to
hold without it.

## The constraint is `NOT VALID` on purpose

Canonical form: `~`-relative, no trailing slash, no whitespace, no parentheses —
a path and nothing else. `project_src_path_canonical` (migration 0019) enforces
it.

It is added **unvalidated**, which is the design rather than a hedge. One live row
cannot satisfy it yet:

```
kcard  ~/.archive-src/kcard (kai; archived 2026-07-09, was ~/src/tools/kcard)
```

A sentence of archive history in a path field. Extracting that prose poses a
question a migration cannot answer — which of the two paths is meant, and where
does the history belong? So `NOT VALID` splits the difference exactly where the
knowledge splits: **every INSERT and UPDATE from here on is checked**, which is
what stops the field re-drifting, while the one known-bad legacy row is tolerated
until the pass that owns it lands. Promotion is then a one-liner and a real exit
criterion for that work:

```sql
ALTER TABLE project VALIDATE CONSTRAINT project_src_path_canonical;
```

It correctly does **not** flag `feedhub`
(`~/src/ai-agents/harness-eval-runs/run_02/04-kprojects`), which is well-formed
but points inside an eval-run tree. That is a truth problem, not a form problem;
a constraint that tried to catch it would be guessing.

## The constraint immediately caught a writer, which is the point

`korg-migrate` imports kwi's `cn_path` into korg's project table *after*
migrations run — so with 0019 in place, kwi's raw values (`src/ai/klams`,
`/home/ken/src/tools/korg`, `~/obsidian/gratch/`) would have been **rejected at
import**. The suite is gated on `snapshots/*.dump` being present, so a clean
checkout would not have seen it; this machine has them, and `just check` failed
loudly.

The fix is not to weaken the constraint. korg's column now has a stated canonical
form, so a writer must produce canonical values — the importer canonicalizes on
the way in, via `repo::canonical_src_path`, shared so the Rust and SQL rules
cannot drift apart.

That changed what fidelity *means* for this field, and the test says so:

```rust
// Not byte equality: korg's `src_path` has a canonical form the column
// enforces (0019), so the importer canonicalizes on the way in. Fidelity
// here means "the same path, said the one legal way" — asserting the raw
// string would be asserting a bug.
```

`canonical_src_path` deliberately handles only the three mechanical rules. It
does not strip whitespace or parentheses: truncating `kcard`'s value to its first
token would produce a plausible, unverified path, which is worse than a loud
failure.

The rename stops at the legacy boundary. `KwiProject.cn_path` keeps its name
because that is the source schema; `import.rs` is where the two names meet, and
`fidelity.rs` says so in a doc comment so the disagreement reads as deliberate.

## A correction worth recording: `category` needed nothing

#674 says `category` has casing drift (`trt-llm-explore` holding `"AI"` against
everyone else's `"ai"`) and asks for a normalization pass. Planning this sprint
took that at face value and scoped one. **It was already done.**

Migration `0018_project_category_vocabulary.sql` (WI #678) trued the whole corpus
up to a closed, capitalized vocabulary — `AI`, `Dashboard`, `EVAL`, `Fun`,
`Infrastructure`, `Ops`, `Other` — with a postcondition, and `update_project`
validates against `vocab::PROJECT_CATEGORIES` in the same release. 0018's own
header records the exact corpus #674 describes (`ai` ×8, `AI` ×1, `tooling` ×3,
`infra` ×2, `fun` ×2, NULL ×15) as the *before* state it fixed.

So the defect is the opposite of the one filed: **#674's own exclusion literal is
wrong.** `category = 'fun'` should be `'Fun'`, or better, match against
`PROJECT_CATEGORIES` so a vocabulary change cannot silently re-break it. Recorded
as a comment on #674; no data work, no migration, and Phase A lost an item.

Worth keeping as the sprint's own worked example of what this program is for: a
WI accurately describing state that a later sprint changed underneath it, costing
a planning cycle. That is precisely what the recurring drift check exists to
prevent — and it is a WI-level instance of the project-level problem.

## Verification

`just check` green.

**Against the real corpus**, not just fixtures: the 2026-07-31 03:17 nightly dump
restored into a throwaway `postgres:18-alpine` on kai — never production. All 35
projects, 24 with a non-null path.

Pre-migration, exactly five rows failed the canonical form, which is what #675
predicted: `gratch` (trailing slash), `klams` and `kpidash` (missing `~/`), `korg`
(absolute), `kcard` (prose).

Post-migration:

```
total=35  non_null=24  bad_prefix=0  trailing_slash=0  non_canonical=1
NOTICE: src_path rows left non-canonical (constraint added NOT VALID;
        owned by the project-metadata pass): kcard
```

All four mechanical fixes landed (`~/obsidian/gratch`, `~/src/ai/klams`,
`~/src/kpidash`, `~/src/tools/korg`), `feedhub` correctly untouched, `cn_path`
gone from `information_schema`, and `convalidated=false` on the constraint.

Four behaviours checked rather than assumed:

1. **A bad write is rejected** — `UPDATE … SET src_path='src/nope/'` fails on the
   constraint. This is the anti-drift guarantee, and it is the one that matters.
2. **The tolerated row can still be fixed forward** — setting `kcard` to
   `~/.archive-src/kcard` succeeds and takes `non_canonical` to 0.
3. **Promotion then works** — `VALIDATE CONSTRAINT` succeeds, so the exit
   criterion is real and not aspirational.
4. **The fix-up did not forge provenance.** `project_touch_updated` (0013) is
   disabled across the normalization, same as 0016 and 0018, because a fix-up is
   not a content edit. `gratch` 2026-05-31, `klams` 2026-05-18, `kpidash`
   2026-04-05 all keep their old timestamps. `korg` reads 2026-07-31, which was
   already its value in the dump (`03:04:32`, before the 03:17 backup) — checked
   rather than assumed, since it is the one row that would otherwise look bumped.

## Deploy

`deploy-kubsdb` after merge.

**This migration DOES create a rollback boundary** — the opposite of 029/030 and
of 0018. An image built before 0019 selects `cn_path` and fails against the
renamed column, so rolling back across this deploy is **not** a clean image
re-tag. It needs the column renamed back:

```sql
ALTER TABLE project RENAME COLUMN src_path TO cn_path;
ALTER TABLE project DROP CONSTRAINT project_src_path_canonical;
```

Trivially reversible; the point is that it is not automatic. The nightly dump
holds the prior values regardless.

## Not in this sprint

Gate 1 — #757's field review — blocks everything else in the proposal: #828 (the
tiered read surface), #674 and #673. Run it in a fresh session; its `details`
carries the measured field inventory so it does not re-derive it.

## Deployed 2026-07-31

- **Image**: `korg:3fe5caef20d3` — revision
  `3fe5caef20d3a56991c09a0706521058ba71d4fb` (the squash-merge of PR #32). The
  container's `org.opencontainers.image.revision` label matches
  `git rev-parse HEAD` exactly.
- **Rollback target**: `korg:e2a08a4adde7` (sprint 030). **Not a clean re-tag
  this time** — 0019 renames a column, so that image selects `cn_path` and
  fails. Reversal is two statements, not a dump restore:
  `ALTER TABLE project RENAME COLUMN src_path TO cn_path;` and
  `ALTER TABLE project DROP CONSTRAINT project_src_path_canonical;`
- **CI**: green on PR #32 (rust 3m18s, web 25s).

**The migration announced itself in the container log**, which is the cheapest
possible confirmation that step 3 of 0019 does what it claims:

```
INFO sqlx::postgres::notice: src_path rows left non-canonical
  (constraint added NOT VALID; owned by the project-metadata pass): kcard
```

**Verified live:**

- `post-deploy-check.sh --compare`: **OK**. Every row count unchanged
  (work_items 561, cards 29, links 4, proposals 115, reports 23, projects 35),
  `node_count` 741 and `node_id_seq` 828 stable, and `migrations` **18 → 19** —
  the one value that *should* move, and the only one that did.
- **Production data matches the rehearsal row for row.** All four mechanical
  fixes landed (`~/obsidian/gratch`, `~/src/ai/klams`, `~/src/tools/korg`,
  `~/src/kpidash`), `kcard` is the sole non-canonical row, `feedhub` untouched,
  24 of 35 non-null. Identical to what the restored-dump run predicted, which is
  what a rehearsal is for.
- **The constraint is live and unvalidated**, read back from
  `pg_constraint`: `convalidated=false`, definition
  `CHECK (src_path IS NULL OR src_path ~ '^~/[^[:space:]()]*[^[:space:]()/]$')
  NOT VALID`. Every write from here on is checked.
- **`cn_path` is gone**: `information_schema.columns` holds exactly one of
  `{cn_path, src_path}` for `project`, and it is `src_path`.
- **The agent-facing contract shipped, not just the database.** `tools/list`
  over the live MCP endpoint returns `update_project` carrying an `src_path`
  parameter whose description begins *"Path to the working copy on the
  project's DEVELOPMENT machine (its `machines` entry), not the deploy
  target."* That sentence reaching agents is half of what #675 was for, so it
  is checked rather than assumed.
- **Web renders**: `/`, `/work-items` and `/plan` all 200 over the tailnet
  HTTPS endpoint.

**Not verified by attempting it**: whether the constraint rejects a bad write.
Proving that against production means issuing a write that must fail — and if
the constraint were somehow absent, it would instead corrupt a row. The
enforcement path was proven on the restored dump, and the constraint definition
was read back from `pg_constraint` here; that is the same evidence without the
downside.

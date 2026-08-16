# 060 — Split repo.rs along its section seams

Proposal `korg:1347`, work item #1345. Purely structural: no contract change,
no behaviour change, no migration. Rank 1 ahead of the scoped 044–059 surface
re-review, which wants to read per-domain modules rather than one 8K-line
file.

## Goal

`crates/korg-core/src/repo.rs` had reached **8,151 lines** — 1,834 at the
2026-07-21 review, so ~6,950 lines added across 31 commits in three weeks. A
session had already deferred reading it, which is the failure this sprint
exists to remove: the repository layer is where every surface's behaviour is
actually decided, and a file nobody opens is a file nobody reviews.

Split it into `crates/korg-core/src/repo/`, one module per domain, along the
`// --- section ---` markers the file had already grown. `mod.rs` re-exports
everything, so `repo::` stays the flat namespace every call site outside
korg-core already uses.

## What shipped

21 modules plus `mod.rs`, largest 888 lines (`work_items.rs`), against a
target of "well under 2,000":

| | | | |
|---|---|---|---|
| `comments.rs` 77 | `page.rs` 93 | `planning.rs` 103 | `areas.rs` 154 |
| `awaiting.rs` 174 | `flow.rs` 198 | `common.rs` 219 | `cards.rs` 279 |
| `selectors.rs` 286 | `handoffs.rs` 302 | `links.rs` 330 | `relationships.rs` 366 |
| `preview.rs` 448 | `programs.rs` 521 | `attachments.rs` 542 | `projects.rs` 574 |
| `reports.rs` 589 | `proposals.rs` 684 | `board.rs` 758 | `schedules.rs` 880 |
| `work_items.rs` 888 | | | |

Sections that had accreted in three places merged into one module: work items
were create (588), read views (1867), the lean list (2113) and update (6493);
cards were create, read and the move-plus-rank update; projects were the row,
the tiered read surface and the writes. That interleaving is most of why the
file had stopped being navigable — the markers said "cards" three times.

## Decisions

- **`mod.rs` re-exports with globs, not an explicit list.** An explicit list
  is a second place to forget a new item, and the guarantee worth having is
  that moving a function between these modules cannot change what a caller
  sees. `rustc` still refuses an ambiguous glob, so no two modules can export
  the same name — the property an explicit list would be protecting is
  enforced by the compiler either way.
- **Two plumbing modules, not one.** `common.rs` is the existence/kind checks
  every mutation opens with plus the `node.updated` and transition-log writes
  that follow one; `selectors.rs` is the name-or-id resolution layer (WI #575)
  and keeps its long rationale comment as module docs. `touch_node` and
  `record_transition` sat inside the selectors marker in the old file and are
  not selectors; they moved to `common.rs`.
- **`flow.rs` split out of the board section.** The backlog flow series
  (#1318) shipped beside `board_rollup` and sat inside its marker, but it is a
  time series over the transition log rather than a snapshot of now. 198 lines
  that read as their own thing.
- **Cross-module private helpers became `pub(super)`, not `pub(crate)`.**
  `pub(super)` from `repo::common` is visible throughout `repo` and nowhere
  else — the exact translation of "was private to repo.rs". 19 items needed
  it; everything else stayed private to its module.
- **`report_date_fmt` moved to `common.rs`.** The `time::serde` adapter was
  declared in the reports section and used by the flow series too, so it is
  shared plumbing now rather than something `flow.rs` reaches into `reports.rs`
  for. `time::serde::format_description!` takes a visibility before the name.
- **Section markers became module docs verbatim.** The prose blocks under
  `// === schedules ===`, `// === programs ===`, `// --- attachments ---` and
  the rest carry real design rationale; they became `//!` docs with no words
  changed. The marker-only sections got a written doc line each.

## Verification

The split was done by line-range extraction rather than by hand, and checked
the same way: strip comments and `use` lines from the old file and from the
concatenated new modules, sort both, diff. Every surviving difference is one
of the 19 visibility bumps (with rustfmt's reflow of the six signatures that
`pub(super) ` pushed over the line limit), the two `Page`/`PageQuery` method
visibilities, `report_date_fmt`, and `mod.rs`'s own declarations. **No code
line was lost, added or altered.**

`just check` green: fmt, no generated-artefact drift (`just gen` output
unchanged — the ts-rs bindings and the MCP schema snapshot are byte-identical,
which is the contract check that matters), web-check, `clippy --all-targets
-D warnings`, and 480 tests across 49 binaries including every DB-backed
integration suite. **No test file was edited.**

## Follow-ups

Nothing was fixed inline, per the proposal — and nothing needed to be. The
move surfaced no bug and no contract wobble, so there is nothing to file.

Three pointers named the deleted path and were updated as fallout of this
change, not as scope: `CLAUDE.md` and `.github/copilot-instructions.md`
"Read first" entries, and two assertion messages (`sprint042.rs`,
`vocab.rs`) that told a future reader which file to go edit. Historical sprint
records still say `repo.rs` and were deliberately left alone — they are a
record of what was true when written.

The scoped 044–059 surface re-review (rank 2) is now unblocked and is what
this sprint was for.

## Deployed

2026-08-16, to kubsdb (`:5674`), built from merged `main` `a7053ae`.

| | |
|---|---|
| Image | `kubsdb.encke-wahoo.ts.net:5000/korg:a7053ae5bdb9` |
| Digest | `sha256:4e75fb8a0abebaed7ad7850df6a5206dbe142d3eadc63732e3bca06339fa484c` |
| Rollback target | `471636f3aefe` (sprint 059) — confirmed present in the registry before building |
| Migrations | 27 → 27 — **no migration**, as a pure refactor should have |

The deploy script's revision assertion passed (running label equals the commit
built), and `post-deploy-check.sh --compare` was clean with **every count
unchanged**: work items 921, cards 30, links 4, projects 48, proposals 239,
reports 42, nodes 1274, `seq_last` 1375 on both sides.

### Verified live

A refactor's smoke test is that nothing moved, and this one rewrote the module
every read path runs through — so the check was one read per split module,
all 200:

| Module | Probe |
|---|---|
| `work_items.rs` | `/api/work-items?limit=2`, `/api/work-items/1345` |
| `cards.rs` | `/api/cards?limit=2` |
| `links.rs` | `/api/links?limit=2` |
| `relationships.rs` | `/api/nodes/1345/neighbors` |
| `preview.rs` | `/api/nodes/1347` |
| `projects.rs` | `/api/projects`, `/api/projects/korg/plan` |
| `comments.rs` | `/api/nodes/1345/comments` |
| `areas.rs` | `/api/areas?project=korg` |
| `proposals.rs` | `/api/proposals`, `/api/proposals/1347` |
| `programs.rs` | `/api/programs` |
| `schedules.rs` | `/api/schedules` |
| `awaiting.rs` | `/api/awaiting` |
| `planning.rs` | `/api/proposals/rollup` |
| `board.rs` | `/api/board` |
| `flow.rs` | `/api/work-items/flow` |
| `reports.rs` | `/api/reports`, `/api/report-sources` |
| `handoffs.rs` | `/api/handoffs/1128` |
| `attachments.rs` | `/api/img/stats` |
| web UI | `/plan` deep link |

Four probes captured before the deploy came back byte-for-byte equal after it:
work-items `total` 900, projects 48, flow 6 days at horizon `2026-08-08`, and
the MCP `tools/list` payload. Which is the whole claim this sprint makes — the
surfaces did not move.

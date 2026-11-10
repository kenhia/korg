# 098 — `cut`: a second terminal work-item status

Proposal korg:4367, slice 1 of program korg:4368 (the November 2026 backlog
drawdown). One work item, WI 4366. Ran as a karc leg on kai (`korg-645170`),
under oversight. It runs first because the drawdown itself cuts items and
wants the status to exist before it does.

## Goal

Ken: "mechanically it's the same as `closed`, but it has a different reason
for reaching the terminal state". Cards have `Cut` beside `Done` and proposals
have `declined` beside `done`. Work items were the one kind whose single
terminal state made "finished" and "decided not to do" indistinguishable
afterwards.

## Premise check

- **Holds.** No `cut` existed anywhere in the vocabulary, SQL or UI.
- **No migration was needed.** `workitem.wi_status` is bare `TEXT NOT NULL`
  (0001), so korg-core's vocabulary is the only enforcement. The WI's
  "DB constraint/migration" acceptance line has nothing to act on.
- korg-gate carries no status-specific rule. `update_work_item` passes
  through on its schema, so `cut` follows `closed`'s rule there: none.

## Decisions

The overseer's seven calls (proposal notes) all held; no pushback.

- **Classified on both axes exactly as `closed`:** in `WI_TERMINAL_STATUSES`
  (hidden by default) and `WI_FINISHED_STATUSES` (never blocks, restarts a
  `completed`-anchored schedule like `closed` does). The new
  `cut_mirrors_closed_on_both_axes` fence pins that.
- **`omitted.cut` is its own key.** It is never folded into `closed`.
- **Flow's `closed` means left the backlog.** `work_item_flow` counts a move
  into *either* terminal status. A `closed → cut` relabel is not a second
  exit. Splitting the series would have broken its own identity
  (`backlog_before + Σ(added − closed) == backlog`), and kfdc's Rate of Fire
  relies on that. The field name stays the same, and the description and
  `docs/api.md` say what it now counts.
- **The web progress maths counts `cut` as both work-complete and
  Ken-verified.** It is his other terminal call, and an item he decided not
  to do is not work left on the slice.
- The review page's one-click **Close** stays `closed`-only. That page is a
  queue of finished (`done`/`resolved`) items to accept, and `cut` is for work
  that was not done. Status selects and filters pick `cut` up from the
  generated vocabulary.

## What shipped

- `vocab.rs`: `cut` added to `WI_STATUSES`, `WI_TERMINAL_STATUSES` and
  `WI_FINISHED_STATUSES`, with doc comments and a fence test.
- `list_work_items`: `omitted.cut`.
- Rollups: `cut` buckets on `BoardProposal` and `ProgramSlice`. The SQL was
  already derived from `WI_STATUSES`. The sum fences in sprint062 caught the
  new bucket as designed.
- MCP descriptions (`update_work_item` states the reservation and the
  closed/cut distinction; list_work_items, search, set_awaiting, get_board,
  get_program, work_item_flow, create_schedule) and the server instructions.
- `docs/api.md`, `docs/usage.md`.
- Web `domain.ts`: hidden set, satisfied set, slice progress, close-out check.
- `tests/sprint098.rs`: six behaviour tests: hidden and counted apart, search
  live scope, Awaiting-Ken clears, a cut blocker frees its dependent, rollup
  sums, flow counts one exit.

## Repaired in passing

Every one is proven by `just check` and `sprint098.rs`:

- `awaiting.rs`: the lane filter and the clearing rule both hard-coded
  `'closed'`. Both now read `WI_TERMINAL_STATUSES`. `sql_list` moved to
  `common.rs` so planning and flow could share it.
- `planning.rs`: the rollup denominator hard-coded `<> 'closed'`. It now
  derives from the vocabulary.
- `flow.rs`: four `'closed'` literals now derive from the vocabulary.
- `search.rs`: the `const DOCS` cannot interpolate, so it lists both values.
  A new unit test fences it to `WI_TERMINAL_STATUSES`.

## Consumer sweep

kaed `search` over `*:src` (kai, kubs0, kubsdb) for `wi_status` readers,
status-set literals, rollup field types, `omitted.closed` and
`deny_unknown_fields`.

- **No strict parser breaks.** Every out-of-repo reader types `wi_status` as
  a string: kmon, kyac, kfdc, kmuster, khound (`wi_status=all`), kodds evals
  and koverwatch. None declares a closed enum, union or `Literal`. Extra
  rollup and `omitted` keys are additive.
- **Semantic drift, fixed inline (Branch A):** kmon
  `collectors.py` `CLOSED_WI_STATUSES` gained `cut`, so a `[[drift]]` entry
  citing a cut item expires. kmon#49 is open. Its gates were green locally
  (`lint typecheck test korg-contract`, 1215 passed) and the repo has no CI.
  karc's gate refuses the merge outside the ship turn, so it merges on the
  ship turn.
- **Contract vendors:** kmon and kyac vendor `contract/read-shapes.json` and
  hash it against live korg, so both go red on deploy until they are
  re-vendored. Both offline shape suites pass against the new file
  (21 and 17). Re-vendoring is a ship-turn step after the deploy.
- **Semantic drift, named for routing** (each is an arithmetic term plus an
  interface field, more than the brief's one-line bound):
  - kfdc `src/lib/board.ts:327-337` `progress()`: complete/verified omit
    `cut`, so cut items read as unfinished on the board.
  - koverwatch `src/lib/server/korg.ts:23-35,151`: `KorgSlice` lacks `cut`,
    and "N/M items resolved" omits it.
  - kfo `soak.md:31`: "skip a soak WI whose `wi_status` is `done` or
    `closed`" should also say `cut`.
  - agent-skills `sprint-ship/SKILL.md:620` (both trees): the
    `open + resolved + done + closed` arithmetic. Skills are out of scope per
    the notes.
- **A decision, not a defect:** kmuster `korg.rs:162`
  `suppresses_refile()` treats `cut` like `closed`, so a cut drift finding is
  re-filed the next week. That matches "mirrors closed", but a cut finding is
  one Ken decided not to act on, so re-filing it may be noise.
- **cleo-only (korg-vs, kctrldeck), not reachable from kai:** check for any
  `WiStatus` enum or union, rollup progress arithmetic over
  `resolved/done/closed`, and an `omitted` shape typed as exactly
  `{closed, archived}`.

# 090 — Expandable comment box (and the work-item walk bound)

Proposal korg:3762, covering **WI 3761** and, added mid-sprint at Ken's call,
**WI 3763**. Started by hand on kai.

## Goal

Ken hit the limits of the ~3-line new-comment box while writing playtest notes
into korg:3716. Make the default 5 lines, and put an Expand button under Add
that grows the same box to 50 lines without losing anything typed.

## Premise check

- **WI 3761 holds.** `Comments.svelte` had the new-comment `<textarea>` at
  `min-h-[3rem]` in a `flex gap-2` row, with Add beside it (`self-start`).
- Cross-project plan: korg routes to `korg+/`, but no decision there covers
  the comments UI or the work-item list read.

## Decisions — the comment box (WI 3761)

- Height comes from `rows={expanded ? 50 : 5}` and nothing else. The element
  is never re-keyed, so `bind:value`, the caret and pending paste-ledger
  entries all survive the toggle. `min-h-[3rem]` is gone, because `rows` now
  sets the floor.
- Add and Expand stack in a `flex-col` column. At 5 lines, the column fits
  inside the box's own height (checked by screenshot at 1280 px and 375 px).
- The toggle carries `aria-expanded`, and its accessible name says what it
  will do ("Expand comment box" / "Collapse comment box").
- A successful post resets `expanded`, so the box goes back to 5 lines. A
  failed post leaves both the text and the height alone.
- The optional stretch of the same toggle on the in-place **edit** textarea was
  left out. The AwaitingLane reply box was out of scope from the start.

## WI 3763 — how it got into this sprint

The full Playwright suite against the 2026-10-12 nightly had six failures that
reproduced identically on `origin/main` (`2e58b8c`): `link-up.spec.ts` ×4 and
`handoff.spec.ts` ×2. The cause is that the corpus (2014 work items, 1866 of
them closed) passed `allWorkItems`' 4 × 500 bound. The walk ran in ascending
order, so the rows it cut were the *newest*. I filed that as WI 3763 with
three candidate fixes.

Ken then pointed out that Link Up had been retired. It turned out to be only
half retired: sprint 029 took it off the nav, but the route kept serving (and
`usage.md` said so). Once Link Up was set aside, a rerun showed where the bound
really bites: **Work Items → All projects**. That page hides closed rows by
default, yet it spent its 2000-row budget on them and pushed the newest open
items behind Load all. `work-items-completeness` "the list holds every work
item" says exactly that and failed intermittently. Ken chose to walk
newest-first, over skipping closed rows or raising the bound.

## Decisions — WI 3763

- **Newest-first walk.** `GET /api/work-items` takes `order=asc|desc`, with
  `asc` as the default so every other caller is unchanged. Anything else is
  `invalid_input`. In the repo this is `WorkItemQuery.newest_first`.
  `allWorkItems` always walks `desc` and returns the rows sorted ascending, the
  order the page displays, so nothing visible reorders. When the bound stops
  the walk short, it now trims the oldest rows, which are nearly all closed.
  Load all still fetches everything.
- **De-duplication.** Walking by offset from the top means a create during the
  walk shifts it down one row. That can repeat a row across a page boundary but
  never skip one, so rows are keyed by `wi_number`.
- Compared with "skip closed unless shown", this keeps one corpus. The status
  filter, tag chips and footer counts all keep reading from a single array.
  Compared with raising the bound, it does not need revisiting as the corpus
  grows.
- **Link Up removed:** the route, its spec, the layout's `/link-up` width
  check, the a11y route list entry, and the docs (`usage.md`,
  `node-shapes.md`). `error-surfacing.spec.ts`'s failed-load test was about
  link-up's swallowed errors; it now points at `/cards`, which has the same
  shape (the board fails to load → `ErrorNotice`). The historical mentions in
  `ErrorNotice.svelte` and `eslint.config.js` are kept, marked "since removed".
- `work-items-completeness` loses its "footer is quiet" assertion. On a
  production-sized database the oldest rows are behind the bound by design,
  and the mocked "a bounded list says so" spec covers that footer. The newest
  row being visible is still the regression test.
- The handoff specs needed no change once the walk was newest-first.

## Repaired in passing

- **Code blocks are keyboard-scrollable.** `a11y.spec.ts` on `/daily-reports`
  failed intermittently with axe `scrollable-region-focusable`. The page opens
  the newest report pre-expanded, and when a spec had just created one with a
  wide code block, its `<pre>` scrolled with no way to focus it. Real reports
  have the same gap. `markdown.ts` now adds `tabindex="0"` to every rendered
  `<pre>` through a marked `postprocess` hook. Code content is escaped by then,
  so the replacement only ever matches the block's own tag. Asserted in
  `comment-markdown.spec.ts`.

## What shipped

- `web/src/lib/components/Comments.svelte`: the expandable comment box.
- `web/tests/e2e/comment-expand.spec.ts`: defaults to `rows=5`, then type →
  Expand (`rows=50`, more than 5× taller, text intact) → Collapse → Expand and
  post (clears and returns to 5).
- `crates/korg-core/src/repo/work_items.rs`, `crates/korg-api/src/lib.rs`:
  `newest_first` / `?order=`. Tested in `crates/korg-core/tests/sprint090.rs`.
- `web/src/lib/api.ts`: the `allWorkItems` walk, plus `WorkItemListParams`.
- `web/src/lib/markdown.ts`: focusable `<pre>`.
- The `/link-up` route and spec removed; the references listed above updated.

## Verification

- `just check`: green (exit 0).
- Full Playwright suite against the 2026-10-12 nightly
  (`korg-20261012-032537`, restored into a throwaway `postgres:18-alpine`):
  **112/112, twice in a row, `--retries 0`**. Before this sprint it was 109
  passed, 2 flaky, 7 failed.

## Deployed

2026-10-12 22:41Z, by `deploy-kubsdb` (the `.sprint-deploy` skill), from merged
`main` `37b7af1` (PR #96). The image is `kubsdb.encke-wahoo.ts.net:5000/korg:37b7af1136eb`
(also `latest`). The rollback target is `korg:b23797c93c20`, confirmed present in the
registry. There was no migration.

- The revision assertion passed: kubsdb runs `37b7af1136eb8b2606e847254dda7b332782657a`.
- `post-deploy-check.sh --compare`: **OK**. Every row count is unchanged
  (node_count 3673, seq_last 3775).
- Live: `GET /api/work-items?order=desc&limit=1` returns #3773 (newest),
  ascending returns #1, and `order=sideways` returns 400 `invalid_input`. In a
  browser, `/work-items/3761`'s comment box has `rows=5` and an Expand button,
  and `/link-up` renders korg's 404.

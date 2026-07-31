# Sprint 030 — Review completed page

Proposal `korg:763`, closing it out. Covers **#570** — the one item held back
from `korg:623` because it wanted breakpoint tuning on Ken's wide monitor, which
is a different kind of session from sprint 029's sweep.

Front-end only. No migration, no korg-core or API change. Ships via
`deploy-kubsdb`.

## How this one was run

Same recipe as 029: a **restored nightly dump** (2026-07-30) in a throwaway
Postgres container on kai, with a local `korg-api` on :8090 — never production.
The session is nothing but clicking Close buttons, so pointing `KORG_API` at the
real thing would have closed real work items.

The dump had 5 done/resolved items across 34 projects, which is honest steady
state and useless for judging a layout. 26 recently-closed items were flipped
back to `done`/`resolved` **in the throwaway copy** to get a list worth looking
at.

## What #570 actually asked for

The title says "review helper"; the body specifies a page. The ritual it
replaces is:

1. set the status filter to done + resolved,
2. turn on Quick Edit,
3. scan titles, setting each row's status dropdown to `closed`.

Three holes, and **two of them are correctness, not ergonomics**:

1. a mis-set filter lets you close an item that was still open;
2. a dropdown mis-click lands an item in a state nobody chose;
3. a title-only scan misses what lives in `details` or in a comment.

So the page is built as three refusals rather than three conveniences:

- **It cannot show an open item.** The rows come from a status-filtered *server*
  read, so this is not a filter you can set wrong — there is no filter.
- **Its only mutation is `-> closed`.** One button, one destination. No dropdown
  exists on the page for a mis-click to land in.
- **Selecting a row shows everything.** Content, details and comments together,
  unprompted, for every row — the two places a reason *not* to close lives are
  the two the old scan never showed.

`review-completed.spec.ts` asserts all three, because they are the point.

## Why the survey endpoint, not `list_work_items`

The list rows come from `GET /api/work-items/survey?wi_status=…`, which is the
only work-item read with a **server-side status filter**. Filtering
`api.workItems` in the client would have been fewer lines and quietly wrong: that
read is capped at `LIST_LIMIT_MAX` (500) and spends the cap on rows of *every*
status, so a project with 500 closed items would start dropping the done ones
from a page whose whole job is to show them all. That is #762 from the other
direction, and it was worth not shipping twice.

Two requests instead of one — the filter takes a single status — which is the
price of the list being complete for the statuses it claims. The footer still
says so if a total ever exceeds what was fetched.

The survey projection carries no bodies, which is the second reason it fits:
selecting a row calls `get_work_item` for the authoritative read, so nothing on
the page can render a stale body.

## The interactive part

The details pane sits **below** the list until there is room **beside** it. Three
things were decided by looking rather than reasoning:

- **A container query, not a media query.** This route renders inside the
  layout's `roomy` main, which is 80% of the viewport — so viewport width is not
  the width that decides anything. `@container` measures the box the layout
  actually has.
- **The flip is at `@[86rem]`** (~1720px viewport), not the 72rem first tried.
  At 72rem the split happened on a 1600px screen and gave the list 528px, which
  wraps every title to two lines to buy a details pane wider than prose wants.
  Stacking is the better answer at that size; side-by-side is for the
  full-screened wide monitor #570 describes.
- **The details column is `minmax(26rem,38rem)`**, capped rather than `1fr`.
  Uncapped it took 704px of a 1280px container and starved the list. Capped, the
  list gets every pixel above ~608px, which is where prose stops improving.

Both knobs are in one class list on the layout div, with a comment saying so.

When side-by-side, each pane scrolls in its own box — otherwise the table's
sticky header row sticks to a container that has itself scrolled off, which
looks exactly like a header that does not work.

## Smaller decisions

- **Newest first.** The sprint just finished is the one being closed out; it
  belongs at the top, not under a year of history.
- **The project rides in the URL** (`?project=korg`), not in the sticky
  localStorage key Work Items uses. The address bar says what is on screen, and
  a value written by a `pick()` that may not have happened this session cannot
  mislead.
- **Close undoes rather than confirms** (WI #549). A confirm on every row would
  rebuild exactly the friction the one-click button exists to remove; the toast's
  Undo restores the previous status, `done` or `resolved` as it was.
- **Closing advances to the next row** before dropping the closed one, so
  working down a run keeps the details pane moving instead of emptying.
- **↑/↓ move the selection, and there is deliberately no Close hotkey.** This
  page exists because stray input put items in the wrong state; a shortcut next
  to the navigation keys would be a new way to do that.
- **No proposal filter**, per Ken's comment on #570 (2026-07-31): by the time
  you are doing the close-out paperwork the proposal is already `done` and off
  the dropdown.

## Buttons (Ken, 2026-07-31)

`Review` sits beside `Only Prop`, small like it but coloured like
`+ New Work Item` — white on blue. `Quick Edit` was recoloured to match: it is an
action you reach for, not a filter you tick.

That cost Quick Edit its on-state, which used to be "has a background". It now
inverts — bright accent with dark text — which hovering the off-state cannot
imitate, and it keeps the `✓` prefix.

`Review` is an `<a>`, not a button: it is navigation, so back, middle-click and
the keyboard work without any code.

## Row tickers (added mid-sprint)

Ken liked the Review page's comment ticker enough to want it on Work Items, plus
markers for handoff and details. It split cleanly along what the row data can
already answer, so half shipped and half became **#813**.

**Shipped**, because both fields are already on `WorkItemRow`: 💬 with a count,
and 📝 for "has a details section". Both live in `RowTickers.svelte` rather than
inlined per list — that is exactly how two pages end up disagreeing about what a
marker means.

**Not shipped**: the handoff marker. `has_handoff` is an edge in the
`relationship` table, on neither row projection, and there is no `list_handoffs`
to fetch them in bulk — so there is no client-side version. It needs a boolean on
`WorkItemRow` *and* `WorkItemSummary`, which are published contracts appearing in
the MCP schema every agent reads. The details ticker is missing from the Review
page for the same reason: `survey_work_items` carries no bodies, so it shows
nothing rather than a false negative.

**Glyphs are chosen for font coverage, not just meaning.** 🗒️ (U+1F5D2) was the
first pick for details and rendered as a **missing-glyph box** in Chromium; 📝
(U+1F4DD) is old enough to be everywhere. #813 carries the warning that 🫱
(U+1FAF1, Unicode 14) is the least portable of the three and should be checked on
a real browser before it is committed to.

## A 13-second load that was the dev server, not the page

Ken hit one row taking 13s, then ~20s. Measured rather than guessed:

- `GET /api/work-items/1013` — **1–2ms**, indistinguishable from its neighbours.
- The item is 21 bytes of content, one comment, no edges.
- Driving Ken's exact sequence (select, click through others, back to it, off
  and on) through a real browser on an idle box: **44–66ms every time**.

Not reproducible, and nothing about the data could explain it. The likely cause
is the dev loop rather than the code: that `vite dev` had been running since
Jul 29, and `just check` runs `svelte-kit sync`, which rewrites the
`.svelte-kit/generated` files the server watches — timestamped 02:00:53, inside
Ken's window. Each one makes a long-lived dev server invalidate its module graph
and re-optimize, which stalls requests for seconds. The built bundle korg-api
serves has no such step, which is both the check and the reason production cannot
show this.

## Verification

`just check` green.

E2E against the built bundle on a restored dump: **`review-completed.spec.ts`
(3), the new ticker test in `work-items.spec.ts`, the full axe suite (12,
including the new `/work-items/review` route added to `ROUTES`) and
`theme-contrast` all pass.**

**Nine specs fail, and all nine fail identically on `main`** — verified by
building `main` in a worktree and running them against the *same* database, so
code was the only variable. Chasing *why* found something worth the detour.

**Eight of them are #762**, and #762 turns out to be live in production rather
than a future risk:

- The client has sent `limit: 500` since sprint 015, so the WI's "no limit →
  200" premise is stale. The real ceiling is `LIST_LIMIT_MAX` = 500 and there is
  no headroom left to buy.
- Production holds **554 work items**. The Work Items page asks for 500 and gets
  500 — it is truncating today.
- `repo.rs:1404` orders `ORDER BY w.wi_number` ascending *before* `LIMIT`, so the
  rows dropped are the **highest numbers — the newest items**. "All projects" is
  currently hiding roughly the 54 most recent work items.
- The eight specs (`destructive-confirm` ×2, `handoff` ×2, `link-up` ×4) all
  have one shape: create an item, wait for its row, time out because its number
  is past the cap. They pass on a small database, which is why nobody noticed.
  Fixing #762 fixes all eight, and they become its regression test.

All recorded as a comment on #762. **The ninth is unrelated** — `smoke.spec.ts`
still expects the `Topics` nav link sprint 029 removed — and is filed as **#812**.

Neither is this sprint's to fix. Worth noting that the Review page is immune to
#762 by construction: its server-side status filter spends the cap only on rows
it means to show.

One trap worth recording: run Playwright from `web/`, not the repo root. From the
root it fails with `Playwright Test did not expect test() to be called here` on
every spec, which reads like a broken install rather than a wrong cwd.

No horizontal overflow at 2560, 1920, 1600, 1500 or 390px. At 390 the table
scrolls inside its own box the way Work Items does, which puts the Close button
off-screen until you scroll — acceptable for a page whose stated use is a
full-screened wide monitor, and not a regression of anything.

## Deploy

`deploy-kubsdb` after merge. **No migration** — rollback is a clean image
re-tag.

## Deployed 2026-07-31

- **Image**: `korg:e2a08a4adde7` — revision
  `e2a08a4adde7127ce2406fc3742cd0218641098a` (the squash-merge of PR #31). The
  container's `org.opencontainers.image.revision` label matches
  `git rev-parse HEAD` exactly.
- **Rollback target**: `korg:e19256ecf1bd` (sprint 029). No migration in this
  sprint, so an image re-tag fully reverts.
- **CI**: green on PR #31 (rust 3m31s, web 24s).

**Verified live:**

- `post-deploy-check.sh --compare`: **OK**. Every row count unchanged
  (work_items 556, cards 29, links 4, proposals 107, reports 23, projects 35),
  `node_count` 728 and `node_id_seq` 815 stable, and `migrations` held at 18 —
  the correct result for a sprint that adds no migration.
- **The page's defining property holds in production.** The three survey reads
  the page is built on return **only** the status asked for — `done` 2,
  `resolved` 7, `open` 135, with zero off-filter rows in any of them. That is
  the assertion the whole design rests on, checked against real data rather
  than a fixture.
- **Rendered in a real browser** against `https://kubsdb.encke-wahoo.ts.net:5674`,
  read-only — no Close button was clicked against live data. `/work-items/review`
  shows its 9 rows, goes side-by-side at 2200px (`1104px 608px`), and the detail
  pane loads content and comments on select. No page errors.
- **Tickers render on production data**: 💬 and 📝 both present on `/work-items`,
  and the `Review` button is on the toolbar.
- Nicely self-referential: **#570, this sprint's own work item, is sitting in
  the review queue it asked for** — resolved by sprint-ship, waiting on Ken to
  close it with the one-click button it specified.

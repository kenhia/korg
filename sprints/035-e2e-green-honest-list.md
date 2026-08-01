# 035 — Nine e2e failures, three causes: the list stops lying, the suite stops needing a toy database

**Proposal:** korg:817 (rank 4.2) — covers **#762**, **#701**, **#812**.
**Program:** agent-surface (`sprints/planning/2026-08-korg-agent-surface.md`),
§4 rank 2 — *fix the instrument before making more measurements with it*.

## Goal

Every current e2e failure, and the production bug that owns eight of them.

- **#762** — the Work Items list silently truncates. Live, not a future risk.
- **#701** — three `daily-planner.spec.ts` specs that fail on a Monday, or on a
  database another run has written to.
- **#812** — `smoke.spec.ts` still walks the pre-029 nav.

## The measurement this sprint starts from

Taken on kai against a **restored production dump**
(`/gratch/backups/korg/korg-20260801-032006.sql.gz` → throwaway Postgres 17
container → `korg-api` on :8090), because a fresh database is the state all nine
failures hide in.

`GET /api/work-items?archived=all&limit=500` — what the page asks for today:

| | |
|---|---|
| `total` | **572** |
| rows returned | **500** |
| highest `wi_number` returned | **#733** |
| highest `wi_number` in the database | **#840** |
| payload | **950 KB**, 45 ms |
| the same rows without `content`+`details` | **217 KB** |

So the All-projects view is hiding 72 rows today, and they are the newest ones —
everything from #734 up is unreachable, including every work item this program
has filed. The footer says "N items" throughout.

The last line is the one that decided the design: **78% of what the page
downloads is `content` and `details`, and the list renders neither.** `content`
is read only by the client-side search; `details` only by the has-details
ticker.

Other collections on the same cap, for the record: cards 29, links 4, topics 0.
Same shape, not yet bitten.

## Decisions

### D-1 — load every page; don't paginate a view whose filters are client-side

#762 offered three approaches. The measurement rules out the first (raise the
limit — there is no headroom left, 500 *is* `LIST_LIMIT_MAX`) and argues against
the third for now: the Work Items page filters, searches and derives its tag
chips **in memory, by design**, and that is what makes them cheap (~6ms for 108
rows, measured in 029). Server-side pagination does not add to that model, it
replaces it — every filter, the tag row, and the ↑/↓ cursor all become
server round trips.

So the page pages until it holds the whole collection. Today that is two
requests and ~1.1 MB where it was one and 950 KB.

This composes with the next sprint rather than fighting it. #861 (proposal 866)
makes `list_work_items` lean and terminal-excluded; when it lands, the same loop
fetches 217 KB instead of 950 KB, and fewer rows. Paging-until-complete gets
*cheaper* under that change. Server-side pagination would have had to be
rewritten by it.

### D-2 — a bound, so "load everything" cannot become the next silent failure

Unbounded "fetch until done" is honest at 572 rows and dishonest at 20,000: it
would just be slow, with no signal. `allWorkItems` stops after
`AUTO_PAGE_LIMIT` (4) pages — 2000 rows — and reports `complete: false`.

That bound is what makes Ken's four points (#762, 2026-07-29) real rather than
vacuous:

1. **honest count** — `37 of 500 loaded · 2431 total`, the WI's own "honest full
   form", using all three numbers rather than pretending there are two;
2. **quiet when nothing is hidden** — `complete` is true, so the footer reads
   `37 items · ↑/↓ select · Enter open`, exactly as today. This is the normal
   case and stays byte-identical;
3. **amber** — the truncated form is an attention state, not an informational one;
4. **a control on the footer line** — **Load all**, which lifts the bound for
   that view.

One control, not a pager. "Pagination controls" was written before we knew the
client already asked for the maximum; under D-1's model the only thing left to
control is *whether to finish*, and the count next to it says exactly what
finishing costs.

### D-3 — the find-by-ID jump stops depending on what is loaded

#817's acceptance: *"jump to #123 must still work when #123 is not on the
current page."* Under D-1 every row is loaded today, so the guarantee would be
true by accident and would break the first time D-2's bound bites.

`gotoWorkItem` now fetches the item when it is not in `items` and splices it in.
The jump is correct at any corpus size and under any bound, which is the
property the acceptance criterion is actually asking for.

### D-4 — the pager is one fetch wrapper, because `/link-up` has the same bug

Four of #762's eight failures are `link-up.spec.ts`, which calls
`api.workItems()` with no project and hits the identical cap. Both routes call
`api.allWorkItems` — one implementation, one place for the bound to live.

`api.ts` holds "fetch wrappers and nothing else" (WI #541); a wrapper that makes
more than one request is still a fetch wrapper, and the alternative was the same
loop twice.

### D-5 — #701's honest fix is pinning the browser clock, not the server's

The WI sketched `KorgConfig::fixed`, the way korg-api's harness does it. That is
the wrong clock here: `past plan is frozen…` already mocks **every** API route
it touches (`page.route` over daily-plan, topics, cards, work-items), so the
server's notion of today never reaches it. What fails is the *browser's*
`new Date()` versus the Mon–Sun week the planner renders.

Playwright's `page.clock` (1.45+; the repo is on 1.49) pins exactly that, and
`timezoneId` pins the local-time arithmetic the spec and `dates.ts` both do. The
spec is now unrunnable on **no** day of the week instead of one in seven.

## What actually broke #701 — two of the three were not what the WI guessed

The WI called them "three specs that fail for reasons that have nothing to do
with the planner code". One of the three turned out to *be* planner code, and
another was a Playwright limitation, not a database one. Both were found by
reproducing rather than reasoning: the baseline run left the database dirty,
which is exactly the condition the WI describes, and re-running against it made
both deterministic.

**The flaky topic-picker spec was the planner tearing itself down.** `load()`
re-armed `loading`, and the whole planner sits behind `{#if loading}` — so every
mutation on the page (add, tick, drag, remove) unmounted every `TopicPicker` on
screen and rebuilt it. Anything typed into a picker while a refresh was in
flight was discarded. Not re-arming `loading` after the first load fixed the
spec (3/3 where it had been failing) and fixes the same defect for a person
typing into the picker.

**The "non-empty database" spec was a drag that grabbed the wrong row.** The
trace is unambiguous: the second drag issued `POST /api/daily-plan/972/move`,
and 972 was a leftover from an earlier run, not the item the spec created.
Playwright emulates a drag as hover-source → mouse-down → hover-target →
mouse-up, and `dragstart` fires during that last move — so when reaching the
target scrolled the page, the row under the pressed pointer was no longer the
one that had been grabbed. Columns only grow tall enough to force that scroll on
a database earlier runs have written to, which is why it read as a
database-shape problem. Fixed by giving the spec a viewport where both columns
fit, working four weeks out where nothing is planned by hand, and removing what
it plans.

A third, smaller one surfaced on the way: the spec waited on the "Plan updated"
toast before dragging again, but `refresh()` raises the toast *before* awaiting
the reload. It now waits on the new order — which is also the assertion the spec
was missing, having claimed to test reordering while only ever checking that a
toast appeared.

## What shipped

**#762 — the list stops truncating, and says so when it cannot finish**

- `api.allWorkItems` (`web/src/lib/api.ts`) — walks by offset to completion or
  `AUTO_PAGE_LIMIT` pages, returning `WalkedPage {items, total, complete}`.
  `LIST_LIMIT_MAX` is now a named constant rather than a bare `500`.
- `/work-items` holds `total` and `complete` instead of discarding them; the
  footer keeps `N items` verbatim while the list is complete and grows an amber
  `· N of M loaded` clause plus **Load all** when it is not. `loadAll` widens
  the filter option sets with values the arriving rows introduce, rather than
  resetting them — every one of those sets hides what it does not contain, so
  widening the corpus without widening them would have hidden the very rows the
  button was pressed for.
- `gotoWorkItem` fetches and splices a target that is not in `items` (D-3).
- `/link-up` uses the same walk — it owned four of the eight failures.

**#701 — the planner**

- `+page.svelte`: `load()` no longer re-arms `loading`.
- `daily-planner.spec.ts`: clock + timezone pinned for the frozen-past spec;
  taller viewport; four weeks out; order assertion in place of the toast wait;
  plan items cleaned up. Its `**/api/topics` and `**/api/cards` mocks never
  matched (no `?**`, so the query string missed them) and returned bare arrays
  where the client expects a `Page` — corrected, so the spec is now as mocked as
  it always claimed to be.

**#812** — `smoke.spec.ts` walks Today → History → back → Topics → back, then
the top nav, covering the `BackTo` control 029 introduced and nothing else in
the smoke suite touched.

**New — `work-items-completeness.spec.ts`**

- *the list holds every work item, not the first page* — real data; creates an
  item (which takes the highest `wi_number`, precisely the row an ascending
  capped read cannot reach) and asserts both the row and a quiet footer. This is
  the spec that fails on a production-sized database before the fix.
- *a bounded list says so, in amber, and can be finished* — mocked at 2200 rows
  against the 2000-row bound; asserts the amber clause, then that **Load all**
  makes it quiet.
- *find-by-ID reaches an item the list never loaded* — #817's acceptance,
  asserted against a list that genuinely does not hold the target rather than
  one where it is true by accident.

**Docs** — `docs/setup.md` now tells you to run the suite against a
production-sized database (and why an empty one proves nothing), and drops the
"must be fresh" requirement the specs no longer have.

## Verification

Against `korg-20260801-032006.sql.gz` restored into a throwaway Postgres 17
container, `korg-api` serving the built bundle on :8090.

| run | result |
|---|---|
| baseline, before any change | **9 failed** (destructive-confirm ×2, handoff ×2, link-up ×4, smoke ×1), 1 flaky (topic picker), 48 passed |
| after, fresh restore, `--repeat-each=2` | **122 passed**, 0 failed, 0 flaky |

The nine are exactly the nine the proposal predicted, and the eight #762 owns
were fixed by fixing #762 — they are its regression test, as it said they would
be.

Two failures were confirmed by *reproducing* them rather than by argument: the
frozen-past spec fails when its pinned clock is moved to Monday 2026-01-05 and
passes on Wednesday the 7th, which is what shows the pin is carrying it; and the
drag failure was read off a Playwright trace naming the wrong plan item.

## Deployed 2026-08-01

- PR **#39**, squash-merged as `a81bfd8`. Image `korg:a81bfd8ba179`
  (`org.opencontainers.image.revision` confirmed on the running container).
  Rollback target: `korg:58afde4af036`.
- `post-deploy-check.sh --compare`: every count identical to the pre-deploy
  baseline (work items 597, cards 29, links 4, proposals 119, reports 23,
  projects 36; `node_count` 782, `node_max` 869). **No schema migration in this
  deploy** — 20 → 20; the whole sprint is web assets.
- Backups current at deploy time: `korg-20260801-032006.sql.gz` (685 KB, larger
  than the prior night's 589 KB), timer active.

### Verified live, against production data

The bug's precondition is live on the deployed corpus: `GET
/api/work-items?archived=all&limit=500` returns **500 of 597**, topping out at
**#733**, while `offset=500` holds **97 more** rows up to **#869**. Those 97 are
what the All-projects view used to drop.

Driven read-only through the deployed UI (no writes to production):

| | |
|---|---|
| requests the page issued | `offset=0` **and** `offset=500` — the walk pages |
| footer | **133 items**, no truncation clause |
| rows rendered | 133 |
| `#861` visible | yes — `wi_number` > 733, so past row 500 of the ascending order |

133 is exactly the non-closed, non-archived count computed independently from
the API, so the footer's number now describes the whole collection rather than
the filtered remains of an arbitrary first page. Before this deploy the same
view held 500 rows ending at #733 and reported "N items" regardless.

`/plan` and `/work-items` both return 200.

Two dead ends worth recording, because both looked like production faults and
neither was. Asserting on `#869` proves nothing — it is a **klams** item, so it
is present in the sticky per-project view the page opens on. And waiting for the
Project column header is not a "load finished" signal: `pick()` sets
`current = ALL` synchronously, so the column appears while the previous
project's rows are still on screen. Both made the page look like it was serving
one capped read. The honest signal is a row that can only exist if the second
page was fetched.

## Follow-ups

- **`api.cards()`, `api.links()` and `api.topics()` have the same silent cap.**
  Not bitten today — cards 29, links 4, topics 0 against 572 work items — and
  deliberately left alone rather than widened into. `allWorkItems` is the shape
  to copy if any of them crosses 500.
- **The page bound is a UI constant, not a contract.** When #861 lands and the
  list read goes lean, the same walk fetches 217 KB where it now fetches 950 KB;
  `AUTO_PAGE_LIMIT` may be worth raising then, on a fresh measurement.
- **The e2e suite is still out of CI**, so none of this is fenced by anything
  automatic. Nothing in this sprint changes that, but the suite is now worth
  running: it no longer needs a fresh database, and it now fails on the bug
  class it used to hide.
- `/link-up` renders every work item as a selectable row. At 572 that is fine;
  it is the next page that will want a projection rather than a bigger fetch.

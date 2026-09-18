# 082 — get where you meant to go, and see what state things are in

Proposal korg:2813, slice 1 of program korg:2816 ("Clear the korg backlog").
Run as an overseen karc leg (`korg-b1e17e`); the overseer reviews before and
after the ship.

Three of Ken's own reading-the-board reports, all in `web/src`, no migration
and no API change:

| Item | Ask |
|---|---|
| 2380 | Work-item full view needs a "Show in Project" control |
| 2444 | Slices on the program page should be clickable — plus the click-the-ID-vs-click-the-title question |
| 2710 | Program state is not obvious at a glance |

## Premise check

All three hold, and one was re-sized as the proposal's notes instructed.

- **2380 — holds.** `NodeDetail.svelte:144` renders the project as a static
  chip and nothing navigates from it. `projectScope.ts` already holds the
  shared `korg.project` scope, and `work-items/+page.svelte`'s `loadProjects()`
  reads it on mount when nothing is selected yet — so the control really is a
  write plus a navigation. **Re-sized `M` → `S`** per the proposal's notes.
- **2444 — holds.** `programs/[node_id]/+page.svelte:217-218` renders the
  slice's id and title as two plain `<span>`s. Nothing on the row is
  clickable, on a page whose entire job is routing to slices.
- **2710 — holds, and the screenshot is the proof.** Ken's `img-a95` shows the
  six-button status control reading as six near-identical grey chips. The
  program is `done`, whose treatment is deliberately hueless
  (`bg-neutral-800 text-neutral-400`), so the *selected* button is almost
  indistinguishable from the five unselected ones.

## The click convention (2444's second half)

Ken: *"throughout korg and kfdc we seem to have two standards, sometimes click
the ID sometimes click the title."*

Read against the code, korg is **already consistent** — and the two apparent
exceptions turn out to be arguing the same rule, in comments already in the
tree. So this is written down rather than decided:

> **The title is the link; the id is a handle you read, not a control.**
> Where the title is already spoken for by a different navigation, the id
> takes the affordance instead.

- Title links: programs list (`+page.svelte:120`), the program page's soaks and
  Related lists, the proposal page's covered items.
- Id takes it on Reading list and Schedules — and both say why in situ. The
  reading list's title *is* the outbound URL ("it leaves the title doing the one
  thing a reading list exists for"), and a schedule's title is a rendered
  preview rather than a node handle. In both, the id is the affordance because
  the id is what find-by-ID takes.
- `NodeDetail`'s Related list links `kind #id` because `neighbors` returns the
  edge and not the neighbour's title — there is no title to link.

This is why the 2444 fix links the **title** and leaves the id a plain span:
the convention already existed and the slice list was the surface that had
simply never been wired up.

Per the proposal's ruling, kfdc is **not** widened into. A kfdc alignment item
gets filed only if the two actually disagree once korg is consistent.

## Plan decisions this touches

- **GP-16** — a per-node URL is korg's to emit. The slice link is built with
  `nodePage()`, which reads the generated `NODE_ROUTES` table, not a
  hand-written `/planning/<id>`.
- **GP-19** — `parked` is a visibility class korg owns, and a consumer meets a
  new status literal in production before it has a treatment for it. The status
  control keeps `programStatusStyle`'s neutral fallback for exactly that, and
  the new "current" affordance is deliberately **hue-independent** so it works
  for a literal this bundle has never seen.

## Decisions taken in the sprint

**2710's fix is hue-independent, which is a deliberate departure from Ken's
suggested "yellow border and text".** Amber already means `holding` on this
very control and "awaiting Ken" across the app, so painting the *current*
status amber would assert `holding` on a program that is `done`. The problem is
not that the hues are too similar — they are argued at length in `domain.ts`
and are correct — it is that the control gives "this is the state" and "these
are the states you may set" the same visual weight. So the current status keeps
its own hue and gains a ring, a semibold label and a `▸` marker; the settable
options lose their borders and become plainly secondary. Flagged on the
proposal for the overseer.

**The same defect is on the proposal detail page**, whose status control is the
same shape with the same failure (`proposed` and `declined` are as hueless as
program `done`). Fixing only the program control would have left korg with two
answers to one question — the drift `domain.ts` exists to stop, and #1603's
lesson about a third copy of a status idea. So the "current vs settable"
treatment is a shared pair of helpers in `domain.ts`, applied to both controls.
Recorded under "Repaired in passing".

## What shipped

- `domain.ts` — `STATUS_CURRENT_CLASS` (hue-independent "this is the value")
  and `STATUS_OPTION_CLASS` (the settable options, previously a string pasted
  into both controls).
- `programs/[node_id]/+page.svelte` — slice titles link via `nodePage`; the
  status control marks its current value.
- `planning/[node_id]/+page.svelte` — the same status-control treatment.
- `NodeDetail.svelte` — an opt-in `projectRail` prop renders "Show in project",
  which writes the shared scope and navigates; `work-items/[wi_number]` passes
  `/work-items`.
- `docs/usage.md` — the click convention, as an eighth common-behaviour rule.

## Repaired in passing

- **`docs/usage.md`'s "Behaviour common to every page" said "Six rules" over a
  list of seven.** Sprint 074 added "Comments render as Markdown" and never
  touched the count. No gate covers that sentence, which is why it drifted —
  `docs_drift` checks the tool catalogue, the REST table and the vocabulary
  bullets, not prose counts. Now reads "Eight rules" with 074 and 082 added to
  the attribution list.
- **`svelte-check` caught a narrowing hole in the new control** and it is worth
  recording rather than quietly fixing: `node.project!` inside the click
  handler type-checked as an assertion but was unsound — the template's
  `{#if node}` narrowing does not reach into a handler, because the handler runs
  later. Replaced with a handler that re-checks. The gate found this, which is
  the argument for `--all-targets`-style honesty in the web gate too.

## Verification

Run on kai against a production-sized database, per `docs/setup.md` §195: the
morning's nightly (`korg-20260918-032207`, 46 programs / 1633 work items)
restored into a throwaway `postgres:18-alpine`, `korg-api` on `:8090` serving a
built bundle. `docs/setup.md` says to run the Playwright suite whenever a sprint
touches `web/`, and this sprint touches nothing else.

- **Three specs written failing-first**, then made to pass:
  `show-in-project.spec.ts`, `program-slice-links.spec.ts`,
  `status-control-current.spec.ts`. Confirmed failing against unchanged code
  (4 failed / 1 passed — the 1 being the opt-in guard, which was vacuous until
  the control existed and is meaningful now).
- **Full e2e suite: 107 passed, 0 failed**, including the axe a11y and
  theme-contrast suites, which is the pair that matters for a styling change.
  No unrelated drift found, which is not the usual outcome for this suite.
- **`just check`: green** (exit code read from `PIPESTATUS`, not from the
  `tail` it was piped into). `docs_drift` re-run alone: 19 passed.
- **The status-control fix was checked by eye, on the program Ken screenshotted
  (2232, `done`) rather than on a seeded fixture.** Before: six interchangeable
  grey chips. After: `done` carries a ring in its own ink and a semibold label
  against five plain options. Also checked `active` (2816) to confirm the hue
  semantics are untouched, and `proposed` on a real proposal (2815).

`status-control-current.spec.ts` asserts no colour anywhere, deliberately: a
treatment that only worked for the coloured statuses would pass a colour test
and still be the bug Ken filed.

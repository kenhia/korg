# Sprint 029 — korg UI: interactive polish pass

Proposal `korg:623`, closing it out. Covers **#622** (the interactive polish
pass) and **#671** (bright border on active proposals). The other three items —
#621, #583, #678 — shipped in sprint 028.

**#570 was moved out** to its own proposal (`korg:763`) rather than shipped
here. Its details pane wants breakpoint tuning on Ken's wide monitor, which is a
different kind of session from this one.

Front-end plus one small korg-core change. No migration. Ships via
`deploy-kubsdb`.

## How this one was run

Against a **restored nightly dump** (2026-07-28) in a throwaway Postgres on kai,
with `vite dev` + a local `korg-api` — not against production. That mattered
more than it sounds: the session involved clicking Close buttons, toggling
filters, and planning real proposals onto real days. Pointing `KORG_API` at
production would have made every one of those a live write. 490 work items, 87
proposals, 33 projects — enough that layout and volume problems were visible.

Ken drove; fixes landed as we went.

## Navigation

Reordered by where Ken actually starts, not by when each page was built. `Plan`
moved to the end — it is a graph you consult, not a place you begin. `Link Up`
left the bar entirely: linking nodes and slotting work items are agent requests.
**Its route is deliberately kept** — "it may come back in the future" — so this
is a nav change, not a deletion.

`History` and `Topics` also left the bar, becoming buttons on Today beside the
week control. Both gained a Back button and Escape through a shared `BackTo`
component, so the two cannot drift apart.

## Today, as a planning overview

The page Ken opens first, so it should answer "what am I doing, and is anything
wrong" without further clicks.

- **Clicking a day sets the add target.** The capability already existed as an
  "Add to" dropdown buried in the tray; nothing about a dropdown says *this is
  where things land*. The dropdown is gone; the day headers are real buttons, so
  the keyboard path survives.
- **The target is amber, not green.** Green was the obvious choice and the wrong
  one: every plan item on this page carries a completion checkbox, so a green
  day header reads as *this day is done*. Amber also coexists with the blue
  "today" border, and a day is frequently both at once.
- **Weekend columns carry a violet wash** so the week has a shape at a glance.
- **The tray split**: Proposals (open) above Cards (collapsed), each with a
  count in its header so the count is readable while collapsed, and each with a
  quick-view slide-over — "I will forget what this title encompasses".
- **Work items left the tray.** They are still plannable everywhere else; they
  were drowning the list Ken picks from.
- **The latest daily report's status** renders as a pill beside the week
  heading, linking through to the report.

### Proposals had to become plannable

`create_daily_plan_item` accepted `workitem | card | topic` and rejected
everything else, so "which sprint am I pushing on today" was unanswerable by the
planner. `PLANNABLE_KINDS` in korg-core now includes `sprint_proposal`, with its
title resolution and a test asserting both the new kind and that the set stays
closed (a link is still refused).

The two SQL fragments that resolve a source's title were duplicated with
different join lists; they are now one pair of constants used by both queries,
so adding a kind cannot half-land again.

### One definition for the report pill

`statusStyle` lived on the Daily Reports page. Today needed the same pill, so it
moved to `domain.ts`. A second copy is exactly the drift that module exists to
prevent — two pages disagreeing about what "attention" looks like is worse than
no colour at all.

## Cards

The week planner moved below the board. It is genuinely useful there — dragging
a card straight onto a day beats going to Today — but it is the secondary act on
the page, and sitting above the board it pushed the cards below the fold on
arrival.

## Work Items

**Filter to a sprint proposal.** `Only Prop` narrows the list to the work items
a proposal *covers*, read from the `covers` edges. The `sprint` field looks like
it should answer this and cannot — it is free text and usually empty. Set apart
from the filter row, and applied on a button rather than on selection: choosing
a sprint is a navigation decision and it costs a round trip.

Three choices worth recording:

- It **layers** on the other filters rather than replacing them. A proposal
  covering a closed item will not show it unless `closed` is also ticked, and a
  cross-project proposal viewed from inside one project shows only that slice.
  One control silently overriding three others would be worse.
- Entries carry the **project and its rail colour**. A native `<option>` renders
  as plain text, so the project cannot be tinted separately from the title —
  colouring the whole entry was the honest version of "project in another
  colour", and it reuses a mapping already in Ken's head from the rail.
- The dropdown **stays live while filtered**, so switching sprints is one action.
  Locking it would have recreated the "isn't natural" problem it was fixing.

**Tag filtering**, AND semantics, in a collapsed disclosure above the filter row.

The question was whether this needed a `tags_in_use` table to be affordable. It
did not, and measuring said so: `list_work_items` already returns `tags` on
every row and the page already holds every row, so the chip row is derived from
data in memory before it renders — **zero extra queries**, on a fetch that takes
~6ms for korg's 108 items. The cheap-looking option would have cost more and
delivered less.

Chips derive from the *filtered* set, so each click narrows the row to tags that
still co-occur and a dead-end combination is never offered. Alphabetical, not by
count: a chip that moves when you click a different one is a chip you have to
re-find. Collapsed by default because the agents tag heavily — korg alone has 55
distinct tags — with the **selected count in the summary**, since a hidden
filter that silently shortens the list is the one thing collapsing must not cost.

`+ New Work Item` moved up beside find-by-ID: it acts on the project, not the view.

## Planning

**The project dropdown bug.** Pick a project, navigate away, come back: the
filter still said `klams` but the dropdown offered only "All projects", with no
way out without clearing it.

The page asked the server for one project's proposals and rebuilt the dropdown's
options *from that result*. A filtered fetch cannot contain the projects the
filter excluded, so the option list was only ever correct on an unfiltered load
— and `onMount` restores the sticky project *before* the first load, so the
rebuild was skipped entirely. Now it fetches unfiltered (87 rows, one request)
and scopes in the client; covers are still fetched only for what is on screen.

**#671** — active proposals carry a 2px border at hue 42. Ken described it as
"the colour claude-cleo is in", which was a name-hash accident when he wrote it
and is now the **Ops** category colour from #678 — stable rather than arbitrary.
The transparent border is always present so a status change does not shift
neighbouring cards.

## Verification

`just check` green. Two measurements taken rather than assumed:

- **390px**: no horizontal overflow on any of the nine pages, including the new
  three-row toolbar and the tag chips. Ken's verdict: "more than good enough to
  do something on the phone if I was away from the apartment."
- **axe**: all 11 scans pass after the changes.

**Not covered, and deliberately so**: tab order through the new controls (55 tag
chips could be a tab trap), focus-ring visibility on the new controls, and
contrast of the three new colours — `theme-contrast.spec.ts` samples painted
pixels but only for what it already knows to look at. All need eyes rather than
automation, and none block anything.

Two bugs were found and filed rather than fixed:

- **#762** — the Work Items list silently truncates at the server's 200-row
  default while "All projects" holds 490. Found while measuring the tag work:
  "already in memory" turned out to be true only for the first 200. Ken's
  direction (honest count, attention colour, footer pagination) is on the WI,
  along with the wrinkle that three different numbers are in play.
- **#701** — the daily-planner e2e specs fail every Monday (they assert on a
  "yesterday" outside the rendered Mon–Sun week). Filed in sprint 028.

## Deploy

`deploy-kubsdb` after merge. **No migration** — rollback is a clean image
re-tag.

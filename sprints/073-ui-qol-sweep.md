# 073 — UI QoL sweep: project label, proposal copy, collapsed Sources

Proposal korg:1602. Covers #1601 (S), #1550 (S), #1523 (S), #1498 (XS).
Branch `073-ui-qol-sweep`.

## Goal

Four papercuts, three of them the same shape: *the UI makes me do an extra
thing*. All in `web/src/routes/…`, all Svelte, one branch.

- **#1601** — the project rail is long enough now that selecting a project
  low in a category scrolls the selection out of view by the time you are
  reading its work items. Nothing else on the page says which project you
  are in.
- **#1550** — kfdc's pane opens a proposal's detail page directly, so
  deciding to *start* the thing you were reading meant backing out to
  Planning purely to reach the copy button.
- **#1523** — the Daily Reports Sources panel pushed the newest report
  below the fold, while carrying one thing worth keeping: the FRESH signal.
- **#1498** — a rejected `writeText()` reported "Copy to clipboard failed"
  and stopped, which tells the one audience that meets it nothing useful.

#1498 rode along because it lives in exactly the surface #1550 touches.
That turned out to be the right call for a reason the proposal did not
predict: #1550 moved the button into a component, which is where #1498's
failure handling then had to be written once instead of twice.

## The copy button, and why it became a component (#1550 + #1498)

`copyStart` + `legacyCopy` + a `copiedId` state variable lived inline on
the Planning page. Two call sites is the point at which that stops being
markup, so it is now `$lib/components/CopyStart.svelte` — the glyph, the
✓ feedback, the clipboard branches and the failure message in one place.
The Planning card renders it bare (`⧉`); the detail page renders it with
its label, beside the status controls rather than up in the title row,
because starting a sprint *is* the lifecycle: the row now reads "what
state is this in, and take it".

**#1498's scope was decided before the sprint** (proposal notes, and c1062
on the item): ship the better error message, not the `legacyCopy`-in-the-
catch fallback. The `if (navigator.clipboard) … else legacyCopy(text)`
structure is untouched — the absence branch is the LAN-over-plain-HTTP
case and it works.

What a rejection produces now:

- the composed sentence stays, browser message and all. `reportError`
  still owns the phrasing, which is #547's rule and the thing a bespoke
  "copy is blocked" string would have broken;
- **a second line naming the fix** — `Embedded korg holds clipboard-write
  only if the embedder delegates it — allow="clipboard-write" on the
  iframe`;
- **a clickable escape hatch**, "Open in korg ↗", `target="_blank"`, href
  from `nodePage("sprint_proposal", nodeId)`.

`reportError(e, doing, { hint, action })` plus two optional `Toast` fields
is the whole plumbing. The hint renders *inside* the `role="alert"`
region — announcing "copy failed" and silently drawing the way out is the
accessible version of the message this replaced.

## Decisions

**The hint is unconditional, and that is a reversal.** I built the frame
check first: `window.self !== window.top`, so the `allow` advice only
reached an embedded reader and top-level korg kept the plain message. It
is the better-reasoned behaviour and I removed it, because it cannot be
tested. `window.top` is `[LegacyUnforgeable]` — a spec cannot fake being
framed — and producing a real one means the cross-origin embed with
`allow` stripped that c1061 already priced and declined as costing more
than the XS fix it guards. Adding an untestable branch *to* that fix would
have been the same trade in the other direction. So the sentence is worded
to be true either way: it says what embedded korg needs and asserts nothing
about where you are reading it. The escape hatch earns its keep top-level
too — `Document is not focused` is the everyday rejection and a fresh tab
is a real answer to it.

**The Sources bar excludes by name, not by allow-list.** `isScheduledSource`
is `!{retired, unrated}` rather than `{fresh, stale}`. korg owns this
vocabulary and grows it between deploys; an allow-list drops a value it has
not heard of *silently*, and on a panel whose entire premise is "a source
that stopped filing is itself an alert", silent omission is the one failure
worth designing against. An unknown value lands in the bar uncoloured and
gets noticed. This is GP-13's consumer half aimed inward, at korg's own UI.

**Collapsing Sources does not weaken #950.** A stale source still shouts
from the closed bar, in the same red a `problem` report gets. What the bar
drops is the retired and unrated rows — nobody is waiting on those — and
the per-source prose, which is a paragraph you read once. Colour alone
carries status there (Ken's ask: drop the pill border), which is why
`sourceFreshnessText` exists beside `sourceFreshnessPill` rather than
replacing it: the open rows keep their pills.

**No stickiness on the collapse.** Project Details and Tags on Work Items
are plain `<details>` with no persisted state, and matching them was worth
more than remembering a preference.

## Verification

`just check` green. The full Playwright suite — 91 specs — run twice: once
against an empty database and once against **the 2026-08-23 production dump
restored locally**, which the setup docs are explicit about (an empty
database is the state bugs hide in). Both green apart from `cards-dnd`,
which fails identically on unmodified `main` — confirmed by stashing and
re-running — and one `filter-dismiss` flake that passed on retry and three
further clean runs.

Three new specs:

- `proposal-copy-start.spec.ts` — both surfaces copy the real prompt
  (clipboard read granted to the *test*, never to korg; kfdc withholds
  `clipboard-read` deliberately, GP-17). Then the failure path on both
  surfaces, with `writeText` stubbed to reject on a **present** object —
  deliberately not the absence branch.
- `report-sources-collapse.spec.ts` — shut by default; both scheduled
  sources report from the closed bar; retired is off the bar and still in
  the panel. Colour is asserted as *behaviour* — zero border width, and
  fresh ≠ stale computed colour — not as class names. It creates its
  fixture over `/mcp` (`/api/reports` is read-only, and the transport is
  stateless JSON, so it is one POST) and retires what it created in a
  `finally`: without that, every run leaves a source permanently shouting
  `stale` from the bar of whatever database the suite was pointed at.
- `project-details-label.spec.ts` — absent under "All projects", present
  while the section is shut, and moves when the selection does.

Screenshots taken against the restored production data confirm all three
surfaces match Ken's mocks — including `SOURCES   kmon: FRESH` as one line
where seven rows used to be.

## The cross-project angle

korg is in the `korg+` cluster (`cross-project-planning/index.md`). Two
decisions bear on this scope and neither is contradicted:

- **GP-17** is why #1498's message names the `allow` attribute at all:
  *"the next report of 'X does not work in the pane' goes to the `allow`
  list first"*. kfdc paid for that discovery once (kfdc #1497) and
  korg-dash / kdeskdash / korg-vs are each queued up to pay for it again.
  The message is korg's half of routing that report to the right place.
- **GP-16** is why the escape hatch is built from the generated route
  table rather than a hand-written `/planning/${id}`.

No amendment needed: nothing here changes a contract, and GP-17's
capability-vs-channel line is untouched — the toast is korg talking to its
own reader, not a channel to the embedder.

## Follow-ups

- **korg #1603 (XS, bug)** — a parked proposal's status pill renders
  unstyled on the detail page. Found while adding the copy button: the
  route's local `statusPill` map has four entries, sprint 072 added
  `parked` to the vocabulary, and the selected button gets
  `class="… text-xs undefined"`. Verified against the restored dump.
  Deliberately not folded in — korg:1602's scope was decided — and the
  item argues for the fix one level up: a shared `proposalStatusPill()` in
  `$lib/domain` beside the three sibling helpers that already have the
  `?? neutral` fallback this map lacks.
- `cards-dnd.spec.ts` fails on `main` and is unrelated drift, unfixed here.

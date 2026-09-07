# 078 — The tab names the node you are looking at

korg:1970 · covers #1969 · branch `078-in-page-detail-titles`

## Goal

Sprint 077 put the id in the tab on every detail **route**. Ken checked after
the deploy and it did not meet his expectation — because the routes are not how
he opens a node. Clicking a Work Items row, or a proposal's title on Planning,
opens an in-page surface and changes no URL, so a route-bound title never moved.

Not a defect in 077: #1966 said "anytime I'm looking at a detail" and 077 read
*detail* as *detail page*. Ken: "My mistake in how I worded the work item."

## Premise check

- **#1969 — premise holds.** Re-verified on `main` at the branch point:
  `work-items/+page.svelte:574` (`open()` sets an in-page `detail`),
  `planning/+page.svelte:311-312` (the title button sets `previewNode`), and
  `docTitle` at `domain.ts:553`. **Drifted harmlessly in one number**: the
  proposal said `NodePreview` has 11 call sites; it has **9** real
  `<NodePreview>` mounts — the other two grep hits were a type annotation in
  `NodeDetail` and comments in `Dialog`. Same leverage, same conclusion.

## The scope call: a fourth surface the proposal did not name

Ken reported three cases and the proposal scoped two surfaces. Surveying every
route for in-page node surfaces turned up a fourth with the same shape — the
**cards editor**, a centred modal you reach by clicking a card on the board.
It is included. The whole lesson of 077 was reading "detail" too narrowly, and
having found the surface it would be perverse to leave it out and file a third
round.

The line drawn, and it is the one worth stating: **a surface that takes over to
show you ONE node publishes a title; an inline row expander does not.** That
keeps `reading-list`'s per-row edit form out — the list is still right there,
you have not gone anywhere — while catching the card editor, which is a modal.

## Decisions

### Two mechanisms, on purpose, and the asymmetry is load-bearing

- **In-page surfaces publish a `<svelte:head>`** — Work Items panel, cards editor.
- **The slide-over assigns `document.title` imperatively** in an `$effect`,
  restoring what it found on cleanup.

That looks inconsistent until you measure it. **`document.title` reads the
FIRST `<title>` in the document**, so a second `<svelte:head>` mounted by an
overlay loses to the page underneath. Verified in the browser rather than
assumed:

```
planning/3, then inject a second <title>
  -> {"titleElements": 2, "documentTitle": "korg — proposal 3"}
```

The injected second title changed nothing. So a `<svelte:head>` in `NodePreview`
would have been wrong *only* on pages that already set a title
(`planning/[node_id]`, `handoffs/[node_id]`) and right everywhere else — the
worst failure shape available. Assigning `document.title` writes through
whichever element is first, so the overlay wins for exactly as long as it is
open.

**The precedence rule this sprint owes: the topmost open surface owns the
title.** The slide-over is always topmost — it is a modal `<dialog>` — so
"topmost" needs no bookkeeping, just the effect. `detail-titles.spec.ts`'s last
test is what would catch a well-meaning refactor to `<svelte:head>` everywhere.

### `<svelte:head>` may not sit inside a block

`{#if detail}<svelte:head>…` fails to compile (`svelte_meta_invalid_placement`).
The condition goes in the expression instead — which reads better anyway: the
close case becomes a value (`DOC_TITLE_BASE`) rather than an unmount, so "back
to plain korg when I leave a detail" is stated rather than implied.

### The kind arrives late in the slide-over, and that is accepted

A route knows the kind from its URL; `NodePreview` gets only a `nodeId` and
learns the kind from its fetch, so its title can only be right after the load.
077 designed that flicker *out* for routes. It is accepted here — a late id is
never a wrong one — and `node.node_id` is used rather than the `nodeId` prop
because the payload's is authoritative (both non-nullable on `NodePreview`).
The Work Items panel has no such problem: kind is always `workitem` and the row
carries `wi_number`, so it is synchronous.

## What shipped

- `web/src/lib/components/NodePreview.svelte` — the imperative title effect,
  covering all **9** mount sites at once.
- `web/src/routes/work-items/+page.svelte` — the detail panel's title.
- `web/src/routes/cards/+page.svelte` — the editor modal's title.
- `web/tests/e2e/detail-titles.spec.ts` — four tests: each surface, and the
  stacking case.

The proposal suggested putting the tests in the Work Items and Planning specs.
They went in one file instead: this is one behaviour with one precedence rule,
and the stacking test belongs to no single surface.

## Deployed

**2026-09-07 to kubsdb** (`:5674`), image
`kubsdb.encke-wahoo.ts.net:5000/korg:ac9b022f6bce`, built from merge commit
`ac9b022`. The deploy's revision assertion confirmed the running container
carries that commit rather than taking `compose pull` on trust.

Rollback target `560337877e68` (sprint 077), confirmed present in the registry
during preflight. No schema change: 33 migrations before and after.

`post-deploy-check.sh --compare` clean — every count identical across the
deploy (work items 1276, proposals 357, cards 30, links 15, reports 64,
projects 57; `node_count` 1868).

Verified in a browser against `https://kubsdb.encke-wahoo.ts.net:5674`, driving
the three gestures Ken reported plus the two this sprint added:

| Gesture | Tab title |
| --- | --- |
| Work Items → click a row | `korg — WI 1966` |
| … then Escape | `korg` |
| Planning → click the proposal title | `korg — proposal 1960` |
| … then Escape | `korg` |
| Programs → click the title (worked before) | `korg — program 1480` |
| Cards → click a card | `korg — card 200` |
| … then Escape | `korg` |
| `planning/1970`, preview opened over it | `korg — WI 1969` |
| … then Escape | `korg — proposal 1970` |

The last pair is the precedence rule live: the slide-over takes the title from
the page beneath it and gives it back on close.


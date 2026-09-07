# 077 — Title bar carries the id, not the node title

korg:1967 · covers #1966 · branch `077-title-bar-node-id`

## Goal

The find-by-ID work in 076 gave every detail page a `<title>` built from the
node's *title* (`korg - Comments carry no prov…`). Ken finds it distracting,
and it answers the wrong question: what he needs when a long detail scrolls the
header off screen is the **id**, so he can cite the thing without scrolling
back up. Make the tab read `korg — WI 1961`, `korg — proposal 1480`, and plain
`korg` off a detail page.

## Premise check

- **#1966 — premise holds, every line of it.** The proposal's file/line table
  was accurate at each cite: `NodeDetail.svelte:95` renders
  `{node.title} — korg` (six kind pages at once), `planning/[node_id]:87` and
  `handoffs/[node_id]:103` do the same for their kinds, `programs/[node_id]`
  sets no `<svelte:head>` at all, and `app.html:9` is the plain `korg`
  fallback. The four list titles are as described, `Search · korg` included.

## Decisions

### The kind → word table lives in korg-core, not in `domain.ts` (GP-16)

The proposal's shape was `docTitle(kind, node_id, wi_number?)` in
`web/src/lib/domain.ts`, "keyed off the same kind vocabulary". Written
literally that is a hand-kept kind → word map in the web app, which is the
shape **GP-16** rules out — and korg's own generator says so in as many words,
at `vocab.rs`'s `render()`:

> The route table (#1467). Generated rather than hand-kept in domain.ts so
> korg's own UI reads the same authority `/n/:node_id` and `NodePreview.url`
> are built from — a second copy in the web app is the drift this table exists
> to end, **even though the web app is korg rather than a consumer of it**.

The same argument transfers unchanged: korg grows `NODE_KINDS` between
deploys, and a tenth kind should not be able to reach production with a route
but no title word. So `NODE_TITLE_WORDS` goes in `vocab.rs` beside
`NODE_ROUTES`, fenced by `every_node_kind_has_a_title_word` against
`NODE_KINDS`, and `just gen` emits it into `generated/vocab.ts`. `docTitle()`
stays in `domain.ts` and becomes the same three lines `nodePage` is: a lookup
over the generated table.

This is GP-16 applied rather than amended — it names the kind → *path* map,
and the plan needs no edit to cover a second table built the way it prescribes.

### `wi_number` is not a parameter

The proposal sketched `docTitle(kind, node_id, wi_number?)`. Since the 0009
identity migration a work item's node id **is** its `wi_number`, which is
exactly why `nodeHref`'s `wi_number` argument was deleted in 070 (the comment
where it used to sit says so). `docTitle(kind, node_id)` matches `nodePage`.

### Settled as the proposal proposed

- **Em dash**, matching the existing `korg — schedules`.
- **`WI` uppercase, every other kind lowercase** — `korg — WI 1961`,
  `korg — proposal 1480`.
- **Attachments use `img-7ac`**, not the decimal node id: that is the spelling
  the markdown token and `get_attachment` use. `imgIdFromNodeId` already
  derives it, and it is the server's own rule mirrored (`korg_img::ImgId` is
  the node id in hex).
- **Titles are set from route params, never from the loaded node.** No
  `korg` → `korg — WI 1961` flicker, and the id is still right when the node
  404s or is the wrong kind.
- **An unknown kind gets plain `korg`.** GP-13's consumer half: `kind` is a
  string, a caller can be wrong about it, and the honest answer is the
  `app.html` fallback rather than `korg — undefined 1961`.
- `Search · korg` → `korg — search`, so all four list titles read alike.

### The cost, stated

The node title leaves the tab entirely. With many tabs open, two work items are
now distinguishable only by id. That is the trade Ken asked for — the id is
what he refers back to — but it is worth one look on the live page.

## What shipped

- `crates/korg-core/src/vocab.rs` — `NODE_TITLE_WORDS` + `node_title_word()`,
  two fences (`every_node_kind_has_a_title_word`, `node_title_words_are_distinct`),
  and the generator arm.
- `web/src/lib/generated/vocab.ts` — regenerated.
- `web/src/lib/domain.ts` — `docTitle(kind, node_id)`.
- Four call sites: `NodeDetail.svelte` (six pages), `planning/[node_id]`,
  `handoffs/[node_id]`, `programs/[node_id]` (a `<svelte:head>` it never had).
- `search/+page.svelte` — separator normalized.
- `web/tests/e2e/node-routes.spec.ts` — `toHaveTitle` per kind.

## Drift fixed on the way past

`docs/setup.md` says to run the Playwright suite whenever a sprint touches
`web/`, and to read a failure in an untouched spec as drift to fix. Four
failed. All four were **pre-existing** — reproduced on a stashed tree at the
branch point, before any of this sprint's changes — and all four had one cause:
sprint **076** moved find-by-ID out of the Work Items page and into the nav,
and changed what it does. It no longer filters a list or opens a preview panel;
`submitFind` resolves the id through `/api/nodes/:id` and `goto`s the `url` on
the reply.

- `destructive-confirm.spec.ts` (×2) — its `openWorkItem` helper typed into a
  box that no longer exists (`getByLabel("Find a work item or node by id")`).
  It now goes straight to `/work-items/<n>`, which is where find-by-ID lands.
- `programs-progress.spec.ts` — asserted on the preview panel and its
  "open the node's page" button. Both halves of #982 survive on the page
  itself: the id resolves to a *program* page showing the program's real title,
  not the generic `program #979`.
- `work-items-completeness.spec.ts` — #817's acceptance, restated as "you land
  on its page". Its mocked node also needed the `url` field the real payload
  carries; without it the nav control correctly refuses, so the test had been
  asserting against its own incomplete fixture.

Suite is 95/95 against a fresh database on kai. Fresh, not production-sized —
`docs/setup.md` prefers a restored dump, and this is the weaker of the two.

## Deployed

**2026-09-07 to kubsdb** (`:5674`), image
`kubsdb.encke-wahoo.ts.net:5000/korg:560337877e68`, built from merge commit
`5603378` — the revision assertion in the deploy step confirmed the running
container carries that commit, so the pull is not being taken on trust.

Rollback target is `586e113648b4` (sprint 076), confirmed present in the
registry during preflight. No schema change: 33 migrations before and after.

`post-deploy-check.sh --compare` clean — every row count identical across the
deploy (work items 1275, proposals 356, cards 30, links 15, reports 64,
projects 57; `node_count` 1866).

Verified live in a browser against `https://kubsdb.encke-wahoo.ts.net:5674`,
which is the check this sprint actually needed — the title is set client-side,
so a 200 from `curl` proves nothing about it:

| Page | Tab title |
| --- | --- |
| `/work-items/1966` | `korg — WI 1966` |
| `/planning/1967` | `korg — proposal 1967` |
| `/programs/1063` | `korg — program 1063` |
| `/cards/812` | `korg — card 812` |
| `/handoffs/1846` | `korg — handoff 1846` |
| `/search` | `korg — search` |
| `/work-items` | `korg` |

The last row is the half of Ken's ask that came for free: off a detail page the
tab goes back to plain `korg`.


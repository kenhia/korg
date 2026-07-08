# Sprint 009 — find, preview & navigate

Proposal `korg:286`. WIs: #260 (find work item by ID), #231 (Daily Reports
findings open the WI Preview panel), #284 (SPA deep-link 404), #243 (freeze
the top nav). Theme: make any node findable, previewable, and reachable from
the Work Items page — with one shared preview panel behind all of it.

## Shared node preview (#231 + #260)

The preview panel that lived inline on Planning (`previewWi` + a fixed
slide-over) is now `lib/components/NodePreview.svelte`, and it previews **any**
node kind, not just work items. Give it a `nodeId`; it fetches the new
endpoint and renders a uniform view (kind badge, title, status badges,
project/tags, label/value metadata, markdown body, plus a Details section for
work items).

- **Backend:** `repo::get_node_preview(id) -> Option<NodePreview>` dispatches
  on `node.kind` (workitem / card / link / report / sprint_proposal / slot) and
  returns a kind-agnostic DTO: `{node_id, kind, wi_number, title, project,
  tags, archived, badges, fields, body, body_label, details, created,
  updated}`. `wi_number` is `Some` only for work items (where it equals the
  node id). Dates come back as `YYYY-MM-DD` text (`to_char`), so the client
  needs no date parsing. Exposed as `GET /api/nodes/:id` — returns `null`
  (200) for an unknown id so the find box can say "not found" cleanly.
- **Planning** and **Daily Reports** now both render `<NodePreview>`; the
  duplicated inline markup is gone. Daily Reports findings previously
  navigated (`<a href="/work-items?wi=N">`) — they now open the panel in
  place (#231). Covered items / findings are work items, whose node id equals
  their `wi_number`, so the id passes straight through.

## Find work item by ID (#260)

A "find by ID…" box in the Work Items header. On submit it hits
`GET /api/nodes/:id`:

- **Work item** → jump to it: switch to its project (or All if it has none),
  force the row visible past any active filters (`forceShow` bypass, so a
  closed/archived hit still shows), move the cursor there, scroll it into
  center, and flash a ring for ~2s.
- **Any other kind** → open the shared `NodePreview` panel.
- **Unknown id** → inline "No node with id N."

## SPA deep-link 404 (#284)

`ServeDir::new(dir).not_found_service(ServeFile::new(index))` served the shell
body but **stamped the upstream 404 onto it**, so bookmarks/shared deep links
(e.g. `/plan`) loaded with a 404 status. Switching to
`ServeDir::new(dir).fallback(ServeFile::new(index))` preserves the shell's
200. Extracted to `spa_fallback()` with a unit test (`spa_tests`) that needs
neither a DB nor env vars. Reproduced first, then fixed.

## Freeze the top nav (#243)

The layout `<header>` is now `sticky top-0 z-40` (opaque, so content scrolls
under it). That's the whole change: the Work Items / Cards table headers keep
their existing `sticky top-0`, since each table is its own `overflow-auto`
scroll container — those headers stick to the container top, never the
viewport, so the frozen page nav doesn't interact with them. (An earlier
attempt offset them by a `--nav-h` var on the wrong assumption they stuck to
the viewport; that pushed each header down inside its own container and was
reverted.)

## Verified

- `cargo test -p korg-api -p korg-core` green, including the new
  `node_preview_end_to_end` (workitem/card/missing) and `spa_tests`
  deep-link-serves-200 regression.
- `pnpm check` (0 errors) and `pnpm build` (adapter-static) clean.
- Live smoke against the real binary + built bundle (Postgres in Docker,
  `KORG_WEB_DIR=web/build`): `/`, `/plan`, `/planning`, `/work-items`,
  `/daily-reports` all → 200 + HTML shell; `GET /api/nodes/:id` returns the
  right shape for a work item and a card, and `null` for a missing id.

## Deployed

Deployed to `kubsdb` 2026-07-08 (pre-merge, for live UI verification) via the
new `deploy-kubsdb` skill. Redeployed as `sha256:f3110345…` after a first cut
(`f895a0b7…`) shipped the reverted sticky-thead offset; prior production image
`a4e471df…` kept for rollback. Verified live: `/`, `/plan`,
`/planning`, `/work-items`, `/daily-reports` all → 200 + shell (#284);
`GET /api/nodes/:id` returns the right preview for a sprint proposal (286), a
work item (260), and `null` for a missing id (#260). The three pre-existing
`pnpm lint` errors were fixed as part of this deploy prep (`urls.ts` useless
escape, `WorkItemForm` `untrack` snapshot, dead `loading` in link-up) — lint
and `pnpm check` are now both clean.

## Noted, not fixed

- A missing hashed asset under `/_app/...` now returns the shell with 200
  rather than a 404 (standard SPA-fallback behavior); harmless since built
  asset names are content-hashed and always present.

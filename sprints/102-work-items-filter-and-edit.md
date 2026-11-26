# 102 — Work Items: a project filter on the rail, and edit from the direct page

Proposal korg:5078, a slice of the low-hanging-fruit program korg:5082. It
covers WI 5071 and WI 5024, both front-end only. This sprint ran as a karc leg
on kai (`korg-7b9dbc`), under oversight.

## Premise check

- **WI 5071 holds.** The Work Items rail had no text filter. A project that was
  not starred could only be reached by scrolling the rail.
- **WI 5024 holds.** `/work-items/<id>` rendered `NodeDetail` read-only. Its
  own header comment said the list page was where items were edited. Comments
  worked there, the main fields did not.

## Decisions

The overseer settled the behaviour in the proposal notes. These are the
choices that were left to the leg.

- **The filter matches over what the rail is showing.** "Show all projects"
  still decides whether non-active projects are in play. Matching the whole
  roster would let the box surface projects the checkbox says are hidden.
- **The match list is its own block,** rendered in place of the grouped or
  alphabetical rail while the box has text. A starred project shows once, not
  twice as it does in the band. An empty result says "No project matches" so
  an empty rail does not look broken. Escape clears the box. The filter is
  plain component state, so it is gone on navigation, as the notes asked.
- **5024 edits in place, with no fallback.** `WorkItemForm` needs only the item
  row and its project's areas. Nothing ties it to the list's filters or rail,
  so the direct page mounts it as-is. The full row and the areas are fetched
  when Edit is pressed, not on every visit. After a save, `NodeDetail` is
  remounted (`{#key}`) so the read view shows the saved text. Moving to another
  id closes an open editor.

## What shipped

- `web/src/routes/work-items/+page.svelte`: `railFilter`/`railMatches` and the
  filter input under "All projects" (`data-testid="project-filter"`).
- `web/src/routes/work-items/[wi_number]/+page.svelte`: an Edit button
  (`wi-edit`) beside the back control, which opens the shared form in place.
- Two new e2e specs: `project-filter.spec.ts` covers case-insensitive matching,
  flat alphabetical order, a starred match listed once, picking a match, the
  empty result, clearing, and the filter not persisting.
  `work-item-direct-edit.spec.ts` covers edit and save in place, checked
  against the API, and Cancel.

## Verification

- `just check` is green.
- The full Playwright suite ran on kai against a throwaway Postgres 16
  container: 117 of 117 passed, the four new tests among them.

## Follow-ups

None.

## Deployed

- **When:** 2026-11-26 at about 19:30 UTC (11:30 PST), by the `deploy-kubsdb`
  skill (from `.sprint-deploy`) on the karc ship turn, cleared by the overseer
  in korg:5078 comment 5192.
- **What:** merge `7fedae6` (PR #112) as image
  `kubsdb.encke-wahoo.ts.net:5000/korg:7fedae6ce067`, with `latest` also
  pushed. The running container's revision label matches. The rollback target
  is `korg:c49e2b6aae2e`, confirmed present in the registry.
- **Checks:** `post-deploy-check.sh --compare` passed. Every row count and the
  schema were unchanged (work_items 2467, node_max 5098, 39 migrations).
  `/work-items` and `/work-items/5024` returned 200.
- **Live checks, in headless Chromium from kai against production:**
  - The filter gave `hyp` → `khyprland`. `home` and `HOME` each gave
    `homelab-ai`, `homelab-health`, `k-homelab`. Clearing the box removed the
    match list.
  - On `/work-items/5024`: Edit, change the title, Save. The page stayed on the
    same URL and the API returned the new title. The title was then reverted
    the same way, and the API confirmed the original.

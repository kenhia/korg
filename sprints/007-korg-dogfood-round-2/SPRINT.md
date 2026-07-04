# Sprint 007 — korg dogfood fixes, round 2

Proposal `korg:183`. The original dogfood sprint (proposal 174) shipped its
Planning-page fixes but never got back to these two items before being
marked done — picking them up now, plus a third small tweak found since.

## What and why

- **#88** — Today page doesn't reflect "slots": the Cards page shows items
  dropped into slots, but Today only shows the free-text `goal`, not what's
  actually scheduled there.
- **#103** — Better workflow for changing status of a WI: today it takes
  several clicks to flip resolved→closed; wants a "Quick Edit" toggle on the
  list view with inline dropdowns (type/status/size/area) instead of
  requiring the full edit modal, without cluttering the default view.
- **#118** — Planning page: move "Active" above Pinned/Queue. Minor
  ordering tweak found during sprint 005's live review.

## Result

Deployed 2026-07-04.

- **#88**: Today now loads `cards`/`workItems` alongside slots/links and, per
  slot, fetches `neighbors` filtered to `label === "scheduled"` — resolving
  each to a title regardless of kind (card/workitem/link), matching what the
  Cards page already showed. Renders as small chips under the goal input.
- **#103**: added a "Quick Edit" toggle to the work-items list toolbar; when
  on, the Area/Type/Status/Size cells become inline `<select>`s that PATCH
  immediately on change. Items switched to `closed` stay visible (bypassing
  the default "don't show closed" filter) until Quick Edit is turned off, so
  you can keep editing or undo without the row disappearing. Scoping
  decision: area editing only works with one project selected (uses that
  project's already-loaded area list) — viewing "All projects" keeps area
  read-only in Quick Edit rather than fetching every row's own project's
  areas just for this.
- **#118**: Active section moved above Pinned/Queue on `/planning`.

Verification: `cargo test --workspace` and `pnpm build`/`svelte-check` both
green. Existing Playwright e2e suite needs a local dev server
(`127.0.0.1:8090`) not running in this environment — connection-refused on
*every* spec, including untouched pages, so not a regression signal either
way. Deployed to kubsdb and confirmed healthy (`/api/health`); the Chrome
extension used for interactive verification was unavailable all session, so
these three UI changes have **not** been visually spot-checked in a real
browser — worth a look next time you're in the app.

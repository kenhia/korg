# Sprint 008 — directed relationships + Plan view

WIs: #283 (neighbors direction — grew into directed edges), #282 (Homelab
Plan view). Driven by the homelab-ai plan moving into korg (project 26).

## Directed relationships (supersedes WI #84's undirected design)

`relate()` no longer canonicalizes left/right: the label reads
left-to-right (left `depends_on` right). Exact duplicates still dedup via
the (left, right, label) unique constraint; the reverse orientation is now
a distinct edge. `neighbors` entries gain `direction: "out"|"in"` ("out" =
queried node is the edge's left). No migration needed — the constraint
never enforced ordering, only relate() did. Pre-008 labels (covers,
finding) never consumed direction, so nothing breaks; the 10 homelab-ai
depends_on edges (rel 140–149), whose orientation the old canonicalization
had destroyed, were flipped back by hand post-deploy.

## Plan view

- `GET /api/projects/:name/plan` → `{ items, edges }` (edges =
  `[left, right]` depends_on pairs scoped to the project via
  node.project_id on both ends).
- `/plan` page (nav: first after Today, per Ken): per-area progress bars,
  **Frontier** (open, unblocked, un-parked — with "unlocks #N"), Blocked
  (with blockers), Parked, collapsed Done. Project dropdown, defaults
  homelab-ai — any project with depends_on edges gets the view for free.

## Verified

cargo test --workspace green (directed-semantics test rewritten:
duplicate dedups, reverse is distinct, direction asserted from both
ends). Deployed to kubsdb; MCP neighbors(275) shows out/out/out/in as
expected; plan endpoint returns all 15 items + 10 correctly-oriented
edges; frontier computes to 268/273/274/279/280 matching the plan.

## Noted, not fixed

Deep-linking any route (e.g. /plan, /planning) returns 404 — pre-existing
SPA-fallback gap (index.html is served only at /). Worth a small WI.

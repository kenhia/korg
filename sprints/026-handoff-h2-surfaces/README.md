# Sprint 026 — Handoff H-2: surfaces (viewer, entry points, skill, docs)

Proposal `korg:615`, covering WIs #610–613. Second and final bundle of the
[handoff node plan](../planning/2026-07-21-handoff-node-plan.md); builds the
human/agent ergonomics on top of the H-1 backend, which is **live on kubsdb**
(image `korg:24495eb83ef8`). Tag `handoff-2026-07`. **No migration** — deploy is
a normal rebuild, rollback is a clean image re-tag.

## What the survey found (the H-2 reconciliation)

Like H-1, the plan predates the infrastructure that now absorbs most of it:

- **The "handoff viewer" is the existing generic slide-over.**
  `NodePreview.svelte` fetches `GET /api/nodes/:id` and renders whatever the
  server's `get_node_preview` projection returns (title, badges, fields, a
  Markdown `body`). So the viewer is **one Rust arm** in `get_node_preview`, not
  a new component — the same shape `report`/`sprint_proposal` already use.
- **`related` refs already render on both detail pages** (work-items and
  planning) from the LB-3 block — they are just **not clickable**. Making them
  open the slide-over is the core of #611 and is a generic win: every neighbor
  (proposal, WI, handoff) becomes openable, which is exactly "see the handoff
  from the work it belongs to".
- **`get_proposal.related` is already returned** but the planning page ignores
  it (renders only `covered`). Consuming it surfaces proposal handoffs.

Net: the web work is small and mostly wiring. No new REST endpoint, no new API
client method — the viewer rides `api.node(id)`.

## Slices

### §1 — #610 · handoff in the node preview · M (mostly Rust)
- **Rust**: add a `"handoff"` arm to `get_node_preview`
  ([repo.rs](../../crates/korg-core/src/repo.rs)) mirroring `report`: title from
  the detail row, `summary` as a field, `body` (Markdown) with `body_label`
  "Handoff". `just gen` — no TS change (NodePreview type is unchanged).
- **Legible-missing** (plan migration step 5): if the `handoff` detail row is
  absent, the arm leaves a clear title (`handoff #<id>`) rather than a blank
  generic node — a contract test covers the arm.
- **Web**: none required for the basic viewer — the slide-over renders the arm's
  output. *Optional* (assessed during build, generic to all kinds): a related
  section in `NodePreview`. Deferred unless it stays small; the delivered UX is
  the reverse direction (open the handoff **from** its owner).

### §2 — #611 · owning-context entry points · S
- **work-items detail**: make each `related` ref
  ([+page.svelte](../../web/src/routes/work-items/+page.svelte) ~L905) a button
  that opens `NodePreview(n.node_id)` — the `has_handoff` ref then opens the
  handoff. Keep the Remove (✕) affordance.
- **planning detail**: consume `detail.related` (already fetched by
  `get_proposal`), render its non-`covers` refs (includes `has_handoff`)
  clickable into the preview, alongside the existing `covered` list.
- **proposal handoff presence**: surface it where proposal detail is already
  loaded (covers are loaded per-proposal). A dedicated *card* count is **not**
  added — that would need the collection-level `handoff_count` we deliberately
  deferred in H-1; presence shows in the expanded detail instead.

### §3 — #612 · handoff skill + required-context docs · M
- New `handoff` skill (send + receive): template (purpose/state, scope + korg/
  repo refs, decisions + rationale, interfaces/contracts, changes + validation,
  open questions/risks/dead-ends, next actions + completion criteria); send =
  identify owners → `create_handoff` in one call → return `korg:<node_id>`;
  receive = owning node's focused read → fetch every handoff not already inlined
  → truncation flags are mandatory follow-up; file fallback when korg is down
  (reported local-only).
- Update `start-sprint` to read proposal handoffs from `get_proposal`.
- Tool descriptions + agent instructions say handoffs are **required context**.

### §4 — #613 · migrate living handoffs + e2e · S
- **Migration** (done, live): migrated `korg-dash/HANDOFF.md` — "how korg-dash
  should query korg" — into production korg as **`korg:619`** (project
  korg-dash), attached via `has_handoff` to a new bootstrap WI **#618 "Plan out
  korg-dash"** (korg-dash had no node to own it). Verified live:
  `get_work_item(618).related` reveals the handoff (untruncated, titled) and
  `GET /api/handoffs/619` returns the body byte-for-byte; the temporary
  uncommitted file was then deleted (korg:619 is the record). No bulk import;
  klams had no living handoffs, and `~/src/ai/kagent/HANDOFF.md` remains a
  candidate Ken can migrate the same way if still useful.
- **End-to-end acceptance** encoded as tests, not prose:
  `acceptance_discover_and_read_from_a_bare_wi_number` (MCP) walks the full path
  from a bare wi_number — `get_work_item` reveals the `has_handoff` ref
  untruncated → `get_handoff` returns the continue-elsewhere body; truncation is
  covered by `many_handoffs_truncate_exactly`; the browser path by
  `web/tests/e2e/handoff.spec.ts` (open from work-item **and** proposal).

## Skills note (out of repo)

The `handoff` skill and the `start-sprint` edit live in **global** config
(`~/.claude/skills/`), not this repo — handoffs are created from any project, so
the skill must be available everywhere. They are part of this sprint's
deliverable but are not in the korg PR.

## Test plan
- **Core**: `get_node_preview` handoff arm — title/summary/body populated;
  missing detail row stays legible.
- **Web** (svelte-check + any browser tests present): related refs open the
  preview from work-items and planning; a `has_handoff` ref opens the handoff.
- **Skills**: creating a handoff leaves no unlinked doc; `start-sprint` reads
  proposal + covered-WI handoffs; file fallback doesn't claim durability.
- **Fences**: `docs_drift`, `just gen`.

## Deploy
`deploy-kubsdb` after merge — **no migration**, so a normal rebuild + ship +
recreate; rollback is an image re-tag to `korg:24495eb83ef8` (H-1). Verify live:
open a handoff from a work item's related block in the deployed UI; smoke-test
the acceptance path. Completes the `handoff-2026-07` arc; Agent Space planning
([2026-07-23-agent-space-direction.md](../planning/2026-07-23-agent-space-direction.md))
is sequenced after this.

## Deployed 2026-07-24

- **Image**: `korg:893e4bbe6069` — revision
  `893e4bbe6069af94f988e79453473af9596019ba` (the squash-merge of PR #27).
- **Rollback target**: `korg:24495eb83ef8` (H-1). **No migration**, so an image
  re-tag fully reverts — no dump restore needed.
- **CI**: green on PR #27 (rust + web) before merge.
- **Verified live** against the deployed instance:
  - `GET /api/nodes/619` (the korg-dash handoff migrated in #613) now returns
    its real title, `body_label: "Handoff"`, and body — the #610 preview arm is
    live (before H-2 it was a bare `handoff #619`). So the slide-over renders
    handoffs, and with the browser-verified clickable refs the full
    "click has_handoff → open the handoff" flow works from WI #618.
  - `post-deploy-check.sh --compare`: **OK** — every row count stable.
  - Deep link `/work-items` → 200 (UI served).

This closes the `handoff-2026-07` arc: **H-1** node/core/API + **H-2** surfaces —
both shipped and deployed 2026-07-24. Next: Agent Space planning, reconciled
against this architecture.

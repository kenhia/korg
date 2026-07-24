# Sprint 024 — LB-3 edge context in focused reads

Proposal `korg:598`, covering WIs #593–595. Third and final `linking-2026-07`
bundle from the [021 linking-layer deep review](../planning/2026-07-23-linking-layer/SUMMARY.md)
(decision D-20, resolved with Ken 2026-07-23).

Closes review **L-6**: focused reads hide edge context entirely. An agent that
reads a work item today cannot see that it is *covered* or *depended on* without
independently thinking to call `neighbors` — the exact invisible-context failure
the handoff plan exists to prevent. LB-3 generalizes the sprint-012 two-level
contract (collections signal, focused reads inline, truncation flags are exact)
from **comments to edges**.

Sequencing: after LB-2 (shipped 2026-07-24). This bundle is naturally the
handoff sprint's first half — building the block *generically* (not a
`covers`-special-case) is what lets a future `has_handoff` land as one instance
of this shape rather than a bespoke field.

## §0 — Measurement first (#593, the shape decision)

Per D-20 and the handoff plan's own method, the shape is settled by production
measurement *before* code. Measured read-only against production 2026-07-24:

| metric | value |
|---|---|
| total edges | 264 — `covers` 223, `depends_on` 23, `related-to` 13, `finding` 5 |
| **max node degree** | **9** (two proposals, all `covers`) |
| densest **work item** | degree 5 (WI node 275: `depends_on` 4, `covers` 1) |
| degree distribution | 190 nodes deg 1, 45 deg 2, tail thins fast; only 9 nodes ≥ 6 |
| L-6 blind spots | 191 WIs are covered; 26 nodes sit in `depends_on` edges |
| acceptance node | WI #568 has one edge: `covers`-IN from proposal `korg:569` |

**What the numbers decide:**

- **Cap = 25** (`RELATED_CONTEXT_CAP`). The densest node today is 9; a 25-cap
  inlines *every* current node in full with ~2.5× headroom, bounds the payload
  (~25 refs × ~150 B ≈ 4 KB, trivially within tool-output limits), and only
  ever truncates the handoff-attached future.
- **A flat, capped list ordered by `(label, node_id)`** — not a per-label map.
  This matches the existing two-level shapes exactly (`covered`, `comments` are
  flat lists + a `_truncated` bool), and ordering by label groups the refs while
  keeping structural labels (`covers`, `depends_on`, `finding`) ahead of
  `related-to` (and a future `has_handoff`) so the most important context is
  never the part that gets truncated.
- **Each ref carries the neighbor's `title` and `wi_number`**, so the reader
  learns *what* it is related to without a second fetch — the whole point.

## The settled contract (written into docs/api.md as part of #593)

A new compact ref, and a block on each focused read:

```rust
pub struct RelatedRef {
    pub rel_id: i64,             // pass to unrelate; stable handle
    pub node_id: i64,
    pub wi_number: Option<i64>,  // Some when the neighbor is a work item
    pub kind: String,            // neighbor kind (sprint_proposal, workitem, report, …)
    pub title: String,           // neighbor's title/summary/name, resolved across kinds
    pub label: String,
    pub direction: String,       // "out" | "in" (as neighbors reports it)
    pub directed: bool,          // from the registry; ignore direction when false
}
```

- `WorkItemDetail` gains `related: Vec<RelatedRef>` + `related_truncated: bool`
  — **all** of the WI's edges (covers-IN shows which proposal covers it,
  depends_on both ways, related-to, finding-IN).
- `ProposalDetail` gains the same two fields, **excluding `covers`** — those are
  already inlined as `covered`; the block carries only the proposal's other
  edges (D-20's "which labels are excluded").
- Truncation is exact via `count(*) OVER()` (the `neighbors` pattern); past the
  cap, `related_truncated` is `true` and the caller falls back to `neighbors`
  for the full set. `neighbors` stays the generic floor — no consumer is removed
  from it.

## Slices

### §1 — Measure + settle the contract (#593) · S · research
This document + the docs/api.md contract section. **Done first, no code after
it changes the shape.**

### §2 — Focused reads inline the block (#594) · M
One core helper `related_context(pool, node_id, exclude_label)` → `(Vec<RelatedRef>,
truncated)`, built on a single query that LEFT JOINs the detail tables for
`title`/`wi_number` (no N+1) and computes `directed` from the registry in Rust,
exactly as `neighbors` does. `get_work_item_detail` calls it with `None`;
`get_proposal_detail` with `Some("covers")`. Both transports present the same
shape; `just gen` regenerates the TS/schemas. Acceptance: reading WI #568 alone
shows it is covered by `korg:569`; truncation exact past the cap; the densest
production payload stays within limits; contract tests on both transports.

### §3 — Consumers graduate (#595) · S
- Web work-items detail consumes the inlined block and **deletes its second
  `neighbors` fetch**.
- `start-sprint`'s brief and `sprint-ship`'s deferral pass read the inline block
  instead of calling `neighbors` (SKILL.md edits; verify with a dry run).
- `neighbors` remains for everything else — only the hot paths graduate.

## Test plan

- Core: `related_context` returns the right refs, ordered, capped, truncation
  exact; proposals exclude `covers` but keep other labels; titles resolve for
  each neighbor kind.
- Contract tests both transports: `get_work_item`/`get_proposal` carry `related`
  + `related_truncated` with matching shape.
- `docs_drift` fences stay green; `just gen` fence green.
- e2e: work-items detail still renders relationships with **one fewer** request.

## Deploy

Standard `deploy-kubsdb` after merge. No migration (read-path only). Verify the
inlined block on a live focused read (WI #568 shows its coverer) and
`post-deploy-check --compare`.

After LB-3 the linking layer is enforced (LB-2), attributed (LB-2), and
**visible from the read path** (LB-3) — the ground the handoff sprint was
waiting for.

# Sprint 021 — linking-layer deep review

Proposal `korg:569`, covering WI #568. A review-and-document sprint: no
product code changed. The deliverable is
[`sprints/planning/2026-07-23-linking-layer/`](../planning/2026-07-23-linking-layer/)
— [SUMMARY.md](../planning/2026-07-23-linking-layer/SUMMARY.md) (verdict
and resolved decisions) and
[REVIEW.md](../planning/2026-07-23-linking-layer/REVIEW.md) (evidence,
findings L-1..L-10, decisions D-11..D-20, bundles LB-1..LB-3, validation
log).

## What it concluded

**Evolve, don't rework — and no downtime needed.** The generalized-edge
storage model is confirmed by its own corpus (256 edges, 10 endpoint-kind
combinations, every clean edge written by a typed writer). The problems
are all semantic-layer: the sprint-014 registry declares label semantics —
including endpoint kinds sitting unread in `LabelSpec` — and `relate()`
enforces none of it; edges carry no time or author; one in six
hand-written edges uses an off-registry label, and decoded they are
almost all accidental synonyms of existing mechanisms; `covers` still has
the 27-edge legacy dual shape; and focused reads hide edge context
entirely — the exact invisible-context failure the handoff plan fears.

Every live-data claim was verified read-only against production per the
`docs/operations.md` query path.

## Decisions and follow-on

D-11..D-20 (continuing the 2026-07 series) were resolved with Ken on
2026-07-23 — all recommended options accepted: closed enforced
vocabulary, corpus fix-ups first, nullable `created`/`origin` provenance,
legacy bundles converted to real proposals, lifecycle invariants
documented not enforced, two-level read contract generalized to edges.

Filed as three dependency-ordered proposals, tag `linking-2026-07`:

| Proposal | Bundle | Covers |
|---|---|---|
| `korg:596` | LB-1 corpus true-up: data fix-ups + provenance schema | #585–588 |
| `korg:597` | LB-2 enforcement + provenance write path | #589–592 |
| `korg:598` | LB-3 edge context in focused reads (folds into handoff sprint) | #593–595 |

Sequencing: LB-1 strictly before LB-2; handoff sprint after LB-2, with
`has_handoff` as the closed registry's first extension.

## Not deployed

Docs-only sprint — nothing was deployed. The production image's revision
label will trail `main` until LB-1 ships, which is expected and
self-corrects.

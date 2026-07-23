# Linking-layer review — summary and recommended decision

Date: 2026-07-23 · WI #568 / proposal `korg:569` · Full evidence:
[REVIEW.md](REVIEW.md)

> **Resolved 2026-07-23:** Ken accepted every recommended option
> (D-11..D-20). The bundles below are filed as proposals `korg:596`
> (LB-1, WIs #585–588), `korg:597` (LB-2, WIs #589–592) and `korg:598`
> (LB-3, WIs #593–595), tag `linking-2026-07`, at the top of the
> Planning queue in dependency order.

## Verdict

**Evolve, don't rework — and no downtime is needed.** The storage model
(generalized edges over typed nodes) is confirmed: 256 edges, 10
endpoint-kind combinations, zero contortions, and every clean edge in the
corpus came from a typed writer. All the real problems are in the semantic
layer, and all of them close with two ordered sprints: one migration of
data fix-ups plus provenance columns, then registry enforcement at write
time. The offered take-korg-offline appetite is declined with thanks:
the largest fix-up touches 32 rows, every migration is rehearsable against
a restored nightly dump with exact expected counts, and it all rides the
normal deploy window.

## What the live corpus shows (all verified against production 2026-07-23)

- **The registry declares, nothing enforces.** `LabelSpec` already carries
  per-label endpoint kinds; `relate()` never reads them. Any label, any
  kinds, writes today.
- **Free text fragmented exactly as free text does.** One in six
  hand-written edges is off-registry — and decoded, they are two synonyms
  (`related` ≈ `related-to`; `part_of` ≈ the built-in parent field, which
  is NULL on all three linked WIs) and one genuine one-off
  (`follows_from`). The escape hatch produces fragmentation, not
  extension.
- **Edges have no time and no author.** This review could not determine
  which edges postdate the registry; 54 missing rel_ids are unexplainable;
  sprint-ship's deferral deletes vanish without trace.
- **`covers` still has two shapes**: 27 legacy edges from five pre-0008
  `Sprint: …` bundle WIs block hard enforcement.
- **Multi-coverage is legitimate history** (re-proposal, round-2 sprints);
  the lifecycle invariants that matter hold today with zero violations —
  by skill convention alone.
- **32 of 33 "cross-project" covers edges are an artifact** of seven early
  project-less proposals; six are mechanically backfillable.

## The recommended decision

Adopt, in order (decision detail in REVIEW.md §5; D-numbers continue the
2026-07 series):

1. **Close the vocabulary** (D-11) and **enforce it in core** (D-12):
   unregistered labels and wrong endpoint kinds become `invalid_input`
   with errors that name the registry and the near-miss. Extension stays a
   one-line registry edit that `just gen` propagates everywhere —
   `has_handoff` will be its first customer.
2. **True up the corpus first** (D-13..D-18, one rehearsed migration —
   sprint LB-1): consolidate `related`→`related-to` (7 edges, zero
   collisions), convert `part_of` to real `parent_node_id` (3), relabel
   the lone `follows_from`, **convert the five legacy bundles into real
   archived done proposals** and re-point their 27 edges (retiring the
   "one legacy shape" caveat for good), backfill the 6 unanimous
   project-less proposals (Ken names #175's), and add nullable
   `created`/`origin` columns plus the label index.
3. **Stamp provenance going forward** (D-17, sprint LB-2): writers record
   `origin` (self-reported: `web`, skill name, `propose_sprint`, …) and
   `created`; old edges stay honestly NULL. No audit sidecar yet — no
   consumer.
4. **Document, don't enforce, the lifecycle invariants** (D-19): ≤1 live
   covering proposal per WI; done proposals cover terminal WIs.
5. **Stop hiding edge context in focused reads** (D-20, LB-3 —
   naturally the handoff sprint's first half): `get_work_item` inlines a
   capped, truncation-flagged related-nodes summary, the way
   `get_proposal.covered` already proved out.

## Sprint shape

| Bundle | Size | Prereq | Contents |
|---|---|---|---|
| LB-1 corpus true-up | M | decisions resolved | the migration above + rehearsal + api.md updates |
| LB-2 enforcement + provenance writes | M | LB-1 deployed | registry validation in `relate()`, origin plumbing, UI select w/o `custom…`, contract tests |
| LB-3 edge context in reads | M–L | LB-2 | two-level contract for edges on `get_work_item`/`get_proposal`; fold into handoff sprint |

Then the handoff sprint proceeds on a linking layer where its label is
enforced, attributed, and visible from the read path on day one.

## What was deliberately not recommended

Label FK table or DB triggers (re-creates the drift class B4 killed),
namespaced vocabularies (organizes the noise instead of ending it), edge
ordinals (no consumer; node-side ordering suffices), an edge-audit table
(revisit when handoffs make edge history load-bearing), canonicalizing
undirected orientation (D-1 stands), any graph-query DSL (typed reads +
`neighbors` cover every observed workflow).

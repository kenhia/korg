# 046 — The curated layer: collides-with + board proposal_edges/synopsis

**Proposal:** korg:1004 · **Work items:** #1002 (feature, S), #1003 (feature, M)
**Branch:** `046-curated-layer`
**Context:** the korg slice of kfdc sprint 003 (proposal kfdc:999, Phase 2 —
the curator). Filed and landed inside the kfdc sprint window by Ken's call,
tracked here so the korg queue shows the slice as spoken for. Sequencing was
recorded at write time as a typed edge — kfdc:999 `depends_on` korg:1004
(origin `kfdc-sprint-003`) — the first instance of the write-time-sequencing
direction of travel.

## Goal

kfdc's Phase-2 curator writes typed things INTO korg (standing decision:
agents curate korg, the board renders korg — no LLM in the render path).
Two substrate gaps stood between that and kfdc's Deconfliction + Sensor Net
panels:

- No collision label. `related-to` is too vacuous to drive a Deconfliction
  card — "same contract" must be distinguishable from generic relatedness
  without reading prose. Sequencing needed nothing: `depends_on` is already
  directed, any→any, cross-project.
- The board rollup exposed no inter-proposal edges and no comments, so
  nothing the curator writes could reach the panel that renders it.

## Shipped

- **#1002** — `collides-with` in the registry (undirected, any→any,
  cross-project; "the two nodes collide — same contract / fold on landing").
  One `LabelSpec` + `just gen`, the has_handoff path. Relate/dedup/near-miss
  behaviour all inherited from the registry machinery; test-first in
  `tests/relationships.rs` + registry assertions.
- **#1003** — the board's curated layer, pinned by `tests/sprint046.rs`:
  - `BoardRollup.proposal_edges`: every edge whose **both** endpoints are
    `active`/`queue` rows — `{left, right, label, directed, origin,
    created}`. `directed` answered by the registry; `origin`/`created` are
    the **first read surface for D-17's write-side edge provenance** (api.md
    had predicted the handoff flow would be first). Keys off the *board*
    rows, so slice-only rows fetched for Operations contribute nothing.
  - `BoardProposal.synopsis`: the newest comment opening with the
    `⟦curator⟧` marker, `{body, updated}`, else `null`. `CURATOR_MARKER` is
    now a korg-core const — the convention is korg's, not just kfdc's: one
    marked comment per node, updated in place (which is why `updated`, not
    insertion order, picks the newest), body verbatim with its `mined from:`
    trailer.
- Prose: relate/get_board tool descriptions, api.md label table + board
  table + the no-longer-true "no read surface exposes provenance" note.

## Decisions

- **Both-ends-live** is the edge filter — an edge to a done/declined/archived
  proposal is history, not a collision. The consumer never sees it, so the
  panel cannot mislead.
- **Body verbatim** (marker + trailer included): the writer's convention is
  the reader's to render. korg stores and surfaces; kfdc strips and styles.
- **No new list read** — the curated layer rides the existing one-call board,
  two cheap extra queries (live ids ≈ 15 rows), keeping the #976 aggregate
  discipline.

## Deployed

(filled at deploy)

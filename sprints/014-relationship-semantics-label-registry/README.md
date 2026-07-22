# Sprint 014 — relationship semantics: label registry + direction backfill

Proposal `korg:554` — the second bundle (B2) of the 2026-07 deep-review cleanup
(`sprints/review/REVIEW.md`). Covers WIs #530–#533, closing finding F-01 (plus
the `relate` slice of F-02 and the `neighbors` slice of F-09/F-19) and
implementing decision D-1.

Sprint 008 made `relate()` directed and the tool description said so — but
nothing declared *which* labels have a meaningful direction, and two writers
stored their edges id-canonicalized. So `neighbors.direction` was noise for
every `covers` and `finding` edge, and the handoff-node plan couldn't proceed
until this was settled.

## The label registry (#530)

New `korg_core::relationships` is the single place that answers "what does this
label mean, and does its direction carry information?"

| Label | Direction | Reads |
|-------|-----------|-------|
| `covers` | directed | proposal covers work item |
| `finding` | directed | report reported work item |
| `depends_on` | directed | dependent depends on dependency |
| `related-to` | **undirected** | the two nodes are related |
| anything else | caller-defined | korg stores your order without interpreting it |

`Neighbor` gained a `directed: bool` straight from the registry, so a reader
knows when to ignore `direction` rather than having to memorise D-1. Undirected
labels keep whatever orientation they were stored with — no canonicalization
migration, per D-1 — and reverse duplicates of a *directed* label stay distinct
and meaningful (`A depends_on B` + `B depends_on A` is a cycle, not a dup).

## Semantic orientation + backfill (#531)

`create_proposal` and `upsert_report` inserted `(least(id), greatest(id))`.
Both now insert semantically — proposal → work item, report → work item — and
migration `0014_relationship_orientation.sql` backfills what they left behind,
orienting by endpoint kind and asserting its own postcondition (no `covers`
edge may point at a proposal, no `finding` edge at a report; it raises and
rolls back otherwise).

**The bundle's stated premise was wrong about 27 edges,** and finding that out
changed the migration's design. The WI assumed orientation was always
recoverable because "exactly one endpoint is a proposal/report". In production
that holds for 180 of 207 `covers` edges — the other 27 join two **work
items**: before migration 0008 introduced the `sprint_proposal` kind, a work
item titled `Sprint: …` (#108–#112) *was* the bundle. So the migration:

- flips the 176 structurally-determined `covers` edges and the 3 `finding` ones;
- flips the 27 legacy ones bundle → member using the `Sprint:` title, as a
  **best-effort pass that asserts nothing** — a database whose legacy edges
  don't match that shape is left alone rather than blocked from starting, since
  migrations run on every boot;
- asserts only the structural guarantee, which is what is actually always true.

`docs/api.md` documents the legacy shape as the one `covers` orientation whose
left endpoint is not a proposal.

## relate: self-edges rejected (#532)

Endpoint existence-checking already landed in sprint 013 (it was in B1's §4.2
matrix), so this WI reduced to self-edges: `relate(x, x, …)` is now
`invalid_input`, backed by a `relationship_no_self_edge` CHECK. A self
`depends_on` would have kept a node off the frontier forever. Production was
verified to hold **zero** self-edges before the constraint was added; the
migration also deletes any as a safety net for other databases.

## neighbors: filters, bound, stable order (#533)

`neighbors` returned every edge, unbounded, ordered only by neighbor node id —
so two edges to the same node had unstable relative order, and the Planning
page and four skills pulled everything to filter client-side.

It now takes `label`, `kind` and `limit` (default 100, max 500) and returns
`{items, total, limit, truncated}`. `total` counts every match before the
limit, so `truncated` is exact rather than inferred. Ordering is `node_id` then
`rel_id`.

That is a breaking shape change, taken in lock-step: the web client, the
Planning page (which now asks for `label=covers&kind=workitem` instead of
sifting), the work-items detail panel, and the four skills that read
`neighbors` (`start-sprint`, `sprint-ship`, `refill-queue`, `plan-status`) were
all updated in this change.

## Verified

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo test --workspace` — **52 passed, 0 failed** (new
  `korg-core/tests/relationships.rs` plus two REST contract tests).
- **Migration rehearsed against a restored copy of production** (dump →
  scratch container → migrate 13 → 14), which is how the 27-edge exception was
  found. Result: `covers` 180 `sprint_proposal → workitem` + 27 legacy
  bundle-first, `finding` 5 `report → workitem`, postcondition 0/0, per-label
  totals identical before and after (**no edges lost**), CHECK present. All 23
  `depends_on` edges byte-identical, so `plan-status` / `refill-queue`
  semantics are unchanged — the acceptance criterion.
- End-to-end against that migrated copy: proposal `korg:553` returns its 6
  covered work items with `direction: "out"`, the same edge reads `"in"` from
  the work-item end, legacy bundle #109 reads outward with an exact
  `truncated` flag, and a self-edge is refused with `400 invalid_input`.
- `pnpm check` / `lint` / `build` clean; Playwright **26 passed** on a fresh
  database (1 flaky planner topic-picker timing test, green on retry).
  `link-up.spec.ts` called the neighbors endpoint directly and was updated to
  the envelope — it now also asserts `directed === false` for `related-to`.
- Production untouched throughout: the rehearsal ran against a restored dump in
  a scratch container, which was destroyed afterwards along with the dump.

## Deployed

Deployed to `kubsdb` 2026-07-22 (post-merge, from `main` @ `7c3032b`) via the
`deploy-kubsdb` skill, on the loopback+LAN port binding sprint 013 established.
Image `sha256:b5d220a5…`; prior production image `sha256:d2197807…` (sprint
013) retained for rollback. Container healthy, 0 restarts. Today's nightly
backup (`korg-20260722-032426.sql.gz`) confirmed present beforehand, and all
252 edges were captured verbatim as a baseline.

**Migration 0014 ran against production and did exactly what the rehearsal
predicted.** Diffing the verbatim edge dump before and after:

- 252 edges before, 252 after, **identical `rel_id` set** — nothing created or
  destroyed, only orientation rewritten.
- Per-label counts unchanged: covers 208, depends_on 23, related 7,
  related-to 5, finding 5, part_of 3, follows_from 1.
- All 23 `depends_on` edges **byte-identical**, so `plan-status` and
  `refill-queue` are unaffected.
- 204 `covers` + 3 `finding` edges flipped. (The rehearsal predicted 203
  covers; the extra one is proposal `korg:569`, created between the rehearsal
  and the deploy by the *old* build, so it too was stored id-canonically —
  a small confirmation that the backfill catches whatever the previous writers
  left behind.)
- Final orientation: `covers` 181 `sprint_proposal → workitem` + 27 legacy
  `workitem → workitem`, `finding` 5 `report → workitem`. Postcondition 0/0;
  `relationship_no_self_edge` present; zero self-edges.

Verified live over `https://kubsdb.encke-wahoo.ts.net:5674`: proposal
`korg:555` returns its 6 covered work items with `direction: "out"` and
`directed: true`; the same edge reads `"in"` from the work-item end; legacy
bundle #109 reads outward with an exact `truncated` flag under `limit=2`;
self-edges refused `400 invalid_input` over REST and `isError`/`invalid_input`
over MCP; `neighbors` filters work over MCP; `/plan` deep link 200;
`scripts/mcp-roundtrip-check.sh` green.

## Noticed, not fixed

The production edge corpus carries **two spellings of the same idea** —
`related` (7 edges) and `related-to` (5) — plus free-form `part_of` (3) and
`follows_from` (1). The registry declares `related-to`; `related` is treated as
a caller-defined label, which means its direction is reported as meaningful
when it almost certainly isn't. Consolidating them is a data-cleanup call, not
a code change, so it is left for Ken rather than folded in here.

## Out of scope (per the bundle)

List envelopes and pagination for the other collections, `get_proposal`, and
the two-level read generalization are B3 — which is now unblocked, since it
needs covered refs to read oriented edges.

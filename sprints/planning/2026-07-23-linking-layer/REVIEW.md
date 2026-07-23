# korg Deep Review — The Linking Layer, Agent-First

Date: 2026-07-23 (review executed against `main` @ `3e95f7e`, on branch
`021-linking-layer-deep-review`)  
Reviewer: Fable  
Scope: WI #568 / proposal `korg:569`. The *semantic* layer over the
`relationship` table — vocabulary, enforcement, provenance, cardinality,
reads. The storage model (generalized edges over typed nodes) is explicitly
in scope only to be confirmed or refuted, not redesigned by default.  

Evidence notation follows the 2026-07 review: **[observed]** (verified
against code or a live check), **[inferred]** (evidence-backed inference),
**[preference]** (design recommendation). Live checks in this review ran
**read-only against production** (kubsdb), per the query path
`docs/operations.md` documents and sprint 014 established as legitimate.
Nothing was written. All corpus numbers are as of 2026-07-23 and will drift.

Companion: [SUMMARY.md](SUMMARY.md) — the verdict and the recommended
decisions, for reading first. This file carries the evidence.

---

## 1. Executive assessment

**The storage model has won its argument. The semantic layer declares but
does not enforce, records nothing about when or by whom, and the corpus
already shows both costs.** The right move is to *finish* the layer —
enforce the registry, add provenance, run five small rehearsable data
fix-ups — not to rebuild it.

The evidence for the storage model [observed]: 256 edges across 7 labels
and 10 endpoint-kind combinations, including cross-kind (`related-to`
workitem↔card) and cross-project edges, with zero schema contortions across
seven node kinds. The two *typed* writers (`propose_sprint`,
`create_report`) have produced 190 of the 256 edges (74%) and every one of
them is semantically clean. Every problem found in this review lives in the
other 26% — the edges written by generic `relate()` — or in metadata the
schema never recorded. That distribution is the review's central lesson:
**the fewer free choices an agent has at write time, the better the corpus
gets.** Sprints 013–020 already moved korg in exactly this direction
(typed errors, typed reads, generated contracts); this review's
recommendations continue it into the linking layer.

What sprints 013–020 already fixed here, so the delta is honest [observed]:
direction is now *true* (0014 backfill, semantic writers), `relate`
validates existence and self-edges, `neighbors` is bounded/filtered/stable,
`get_proposal` replaced the N+1 dance, the label registry exists and is
generated into the UI's select and `docs/api.md`, and migration rehearsal
against a restored dump is a documented standing recipe. This review found
**no regression** in any of that.

What remains, in one sentence each:

1. The registry **declares** semantics — including per-label endpoint kinds
   sitting in `LabelSpec.left_kind`/`right_kind` — and `relate()` enforces
   **none of it** (L-1).
2. The label space has fragmented exactly the way free text always does,
   and every agent-invented label turned out to be either a synonym of an
   existing mechanism or a need worth registering (L-2).
3. Edges carry **no time and no author**; this review could not even
   determine which edges postdate the registry, and `sprint-ship` deletes
   deferral history irrecoverably (L-3).
4. `covers` still carries two shapes; 27 legacy edges block hard
   enforcement and complicate every reader (L-4).
5. A handful of data-quality debts (project-less proposals, `related` vs
   `related-to`, `part_of` vs the parent field) are each one small UPDATE
   away from clean (L-5, and §2).
6. Focused reads still hide edge context entirely — the exact
   invisible-context failure the handoff plan exists to prevent (L-6).

**On the offered downtime appetite: it is not needed.** Every fix-up in
this review is a deterministic, transactional migration touching ≤32 rows,
rehearsable end-to-end against a restored nightly dump with asserted
postconditions, and running in the normal seconds-long deploy window.
"Getting it right" here costs decisions and roughly two sprints, not an
outage or a database rebuild (§7).

---

## 2. The corpus, live

All queries ran 2026-07-23, read-only, against the production `korg`
database on kubsdb; full query list in §8. Baseline: **256 edges**,
max `rel_id` 310.

### 2.1 Labels and endpoint kinds [observed]

| Label | Edges | Endpoint kinds (left → right) | Registry? |
|---|---|---|---|
| `covers` | 212 | `sprint_proposal → workitem` 185; `workitem → workitem` 27 (legacy bundles) | yes |
| `depends_on` | 23 | `workitem → workitem` 23 | yes |
| `related` | 7 | `workitem → workitem` 7 | **no** |
| `finding` | 5 | `report → workitem` 5 | yes |
| `related-to` | 5 | `workitem↔workitem` 3, `workitem↔card` 2 | yes |
| `part_of` | 3 | `workitem → workitem` 3 | **no** |
| `follows_from` | 1 | `workitem → workitem` 1 | **no** |

11 of 256 edges (4.3%) are off-registry — but the honest denominator is
the 66 edges written by hand through `relate()`/import (everything except
`covers`-from-proposals and `finding`): **one in six hand-written edges
uses a label korg doesn't understand.**

### 2.2 The off-registry labels, decoded [observed]

Each invented label was pulled and read with its endpoint titles:

- **`related` (7)** is the same idea as `related-to`, spelled the way the
  legacy kwi import spelled it — `korg-migrate/src/import.rs:233-248`
  bulk-inserted the source's own labels. Edges span the entire history
  (rel_ids 1–4, 129–130, 257), so the spelling has been *imitated by later
  writers*, not just inherited [inferred: rel_id 257 is far past the
  one-shot import]. Consolidation is safe: zero of the 7 pairs collide
  with an existing `related-to` edge in either orientation (live-checked).
- **`part_of` (3)** is one agent, one session, attaching WIs #400–402 to
  umbrella WI #277 ("WS1: chat surface"). All three have
  `parent_node_id IS NULL` (live-checked) — the agent reinvented the
  built-in parent mechanism as edges because nothing steered it otherwise.
  Two mechanisms now express "subtask of", and the corpus uses both.
- **`follows_from` (1)** links "Implement Edge tab" ← "Research: second
  tab…" (#474 → #472): implementation-follows-research lineage. A genuine
  one-off need, not a synonym — but one edge is not a proven vocabulary
  entry.

The pattern: **two of three inventions duplicated existing semantics; the
third is a candidate registration.** The free-form escape hatch generated
fragmentation, not extension.

### 2.3 Cardinality and lifecycle [observed]

- **Coverage is many-to-many, legitimately.** 27 WIs carry 2–3 `covers`
  edges; among *real* proposals (excluding legacy bundles) 5 WIs are
  covered twice. Decoded, every case is re-proposal history: a WI in a
  `declined` proposal re-covered by the one that shipped it (#24), or work
  spilling into a "round 2" sprint (#88, #103; klams #61, #62). Coverage
  is **accumulating history**, not current assignment.
- **"At most one non-terminal proposal covers a WI" holds today with zero
  violations** — enforced by nothing except the `refill-queue` skill's
  survey convention.
- **The proposal→WI status matrix is clean**: `done` proposals cover only
  terminal WIs (closed 117, resolved 41, done 7); no `done` proposal
  covers an `open` WI. Again: skill discipline, not schema.
- **Cross-project `covers` is almost entirely an artifact.** 33 edges pair
  a proposal and WI with different projects — but 32 of those are
  proposals with **no project at all**: 7 early proposals (all terminal)
  predate the convention of setting one. 6 of the 7 have a unanimous
  covered-WI project and are mechanically backfillable; #175 spans two.
  Exactly **one** real cross-project edge exists (kdeskdash proposal →
  k-homelab WI). `depends_on` is intra-project in all 23 cases.

### 2.4 What the schema doesn't know [observed]

`relationship` is four columns — `id, left_id, right_id, relationship`
(`crates/korg-core/migrations/0001_init.sql:134-142`) plus the 0006 pair
uniqueness and 0014 self-edge CHECK. No `created`, no author/origin, no
label index. Consequences demonstrated *by this review's own process*:

- It is impossible to tell which edges were written before vs after the
  sprint-014 registry existed.
- `max(rel_id)` = 310 against 256 rows: 54 ids are unaccounted for, and
  deletion (`unrelate`) is indistinguishable from `ON CONFLICT` sequence
  burn. No edge's disappearance is explainable after the fact.
- `sprint-ship` **deletes** `covers` edges for deferred items
  (`~/.claude/skills/sprint-ship/SKILL.md`) — "this item was planned and
  pushed out", a real historical fact, vanishes without trace.

---

## 3. Prioritized findings

Severity: `high` = wrong behavior or integrity risk for agent workflows
today; `medium` = generates future defects or blocks planned work; `low` =
latent/polish. Numbered **L-n** to avoid collision with the 2026-07
review's F-nn series.

---

**L-1 · high · confidence: high — The registry declares endpoint kinds and
direction; nothing enforces any of it at write time.**

- Evidence [observed]: `LabelSpec` carries `left_kind`/`right_kind`
  (crates/korg-core/src/relationships.rs:25-27), populated for `covers`
  (`sprint_proposal → workitem`) and `finding` (`report → workitem`)
  (relationships.rs:38-39,45-46). `relate()` never calls
  `relationships::spec` — it validates existence and self-edges only
  (crates/korg-core/src/repo.rs:779-808, verified line-by-line). A
  `relate(card, link, "covers")` writes successfully today; so does any
  label string. The UI's label select has a `custom…` free-text escape
  hatch (web/src/routes/work-items/+page.svelte:899-905). `docs/api.md`
  states "free-form labels are legal" as contract.
- Impact: the registry's guarantees are only as strong as the last
  well-behaved writer. Readers that filter `covers` by kind
  (`get_proposal`, the Planning page, `refill-queue`) would silently
  ignore a malformed `covers` edge rather than anyone learning of it.
  Cross-agent trust in an edge reduces to trust in every writer that ever
  ran.
- Recommendation: enforce in core (D-12) after the corpus is cleaned
  (LB-1): unregistered label → `invalid_input` naming the vocabulary and
  the near-miss ("did you mean `related-to`?" — the sprint-017 error
  pattern); kind-constrained labels check both endpoint kinds. See D-11
  for the vocabulary policy this presupposes.

---

**L-2 · high · confidence: high — The label space fragments under free
text, and the corpus proves the escape hatch produces synonyms, not
extensions.**

- Evidence [observed]: §2.1–2.2. `related` (7) duplicates `related-to`
  (5); `part_of` (3) duplicates `workitem.parent_node_id` (NULL on all
  three linked WIs); `follows_from` (1) is the only genuine extension in
  256 edges. One in six hand-written edges is off-registry. This is the
  same failure `wi_type` had before D-2 gave it a vocabulary, one layer
  down — and it happened *despite* sprint 014's registry, because the
  registry is advisory.
- Impact: filters miss edges (`label=related-to` misses 7 of 12 related
  pairs); `direction_is_meaningful("related")` returns `true`
  (relationships.rs:75-77), so noise is presented as signal; every future
  reader must know the synonym history.
- Recommendation: close the vocabulary (D-11), consolidate the corpus
  (D-14, D-15, D-16), remove the UI escape hatch. Extension stays cheap:
  a registry entry is one Rust edit and `just gen` propagates it to the
  MCP schema text, the UI select, and docs — the `has_handoff` label
  becomes the first exercise of exactly that path.

---

**L-3 · high · confidence: high — Edges carry no provenance and no time;
history questions are unanswerable and deletions are traceless.**

- Evidence [observed]: §2.4. Four-column schema; 54 unaccounted rel_ids;
  registry-era vs pre-registry edges indistinguishable; `sprint-ship`'s
  deferral `unrelate` leaves nothing behind. Contrast: every *node* has
  `created`/`updated` via the touch triggers.
- Impact: precisely the questions the handoff plan and multi-agent work
  will ask — "who attached this, when, is it stale, what changed since I
  was last here" — cannot be answered for the connective tissue between
  nodes. WI #466's eval-flagging problem is the same question ("which
  writer produced this row") asked of nodes; edges have it worse.
- Recommendation: D-17 — add nullable `created` (honest NULL for
  pre-provenance edges) and nullable `origin` (self-reported writer tag)
  in LB-1's migration; plumb `origin` through the writers in LB-2. A full
  audit sidecar (recording deletes) is deliberately **not** recommended
  yet — no consumer exists; revisit when handoffs make edge history
  load-bearing [preference].

---

**L-4 · medium · confidence: high — `covers` still carries two shapes; the
27 legacy bundle edges block hard enforcement and tax every reader.**

- Evidence [observed]: 185 `sprint_proposal → workitem` + 27
  `workitem → workitem` whose left is one of five archived pre-0008
  bundle WIs #108–112 ("Sprint: korg dogfood fixes", "Sprint: Reliability
  bug-bash — klams + kdeskdash", "Sprint: kapollo core UX",
  "Sprint: hv-simulator dashboard pass", "Sprint: Small-tools polish —
  klams + kpidash"). `docs/api.md` §"One legacy shape" documents the
  caveat; `LEGACY_SPRINT_TITLE_PREFIX` exists in code solely to serve it
  (relationships.rs:84).
- Impact: `covers` endpoint-kind enforcement (L-1) cannot be asserted as
  a corpus invariant while these exist; every kind-filtering reader
  carries a documented blind spot; the registry's own `left_kind:
  Some("sprint_proposal")` is false for 13% of covers edges.
- Recommendation: D-13 — convert the five bundles into real (archived,
  `done`) `sprint_proposal` nodes and re-point the 27 edges, retiring the
  legacy shape, the doc section, and the code constant. The cheaper
  fallback (relabel to a legacy-only label) and the status quo are also
  costed in D-13.

---

**L-5 · medium · confidence: high — Seven project-less proposals make 32
`covers` edges read as cross-project and hide from the Planning page's
project filter.**

- Evidence [observed]: §2.3. All 7 are terminal (6 done, 1 declined); 6
  have a unanimous covered-WI project; #175's covered WIs span two.
- Impact: any consumer grouping proposals or edges by project sees noise;
  the one *real* cross-project edge is camouflaged by 32 artifacts.
- Recommendation: D-18 — backfill the unanimous 6 in LB-1; Ken picks for
  #175 (kdeskdash, by its covered-WI majority and title) or leaves it.

---

**L-6 · medium · confidence: high — Focused reads hide edge context
entirely; the two-level contract stops at comments.**

- Evidence [observed]: `get_work_item` returns comments but no edge
  information of any kind — an agent reading WI #568 does not learn it is
  covered by proposal 569 or that anything depends on it. The web
  work-items page compensates with a second `neighbors` fetch
  (web/src/routes/work-items/+page.svelte); MCP consumers get nothing
  unless they independently think to call `neighbors`. The handoff plan
  names this exact failure as the thing a handoff feature must not
  reintroduce (sprints/planning/2026-07-21-handoff-node-plan.md:23-26) —
  and it is already the status quo for `depends_on` and `covers`.
- Impact: invisible context — the pre-sprint-012 comment failure, now for
  edges. Grows strictly worse with `has_handoff`.
- Recommendation: D-20 — extend the two-level contract to edges on the
  focused reads (`get_work_item` first), with the shape settled by payload
  measurement in LB-3 / the handoff sprint. `get_proposal.covered` is the
  proven template.

---

**L-7 · low · confidence: high — Unknown labels report `directed: true`,
presenting caller-noise as signal.**

- Evidence [observed]: `direction_is_meaningful` returns `true` for any
  unregistered label (relationships.rs:75-77, documented deliberately);
  sprint 014's own README flags `related`'s direction as "reported as
  meaningful when it almost certainly isn't".
- Impact: today it misleads for 11 live edges; under a closed vocabulary
  (D-11) the case becomes unreachable and this resolves itself. Listed
  separately so the fix is recognized as a *consequence* of D-11, not a
  code change to make now.

---

**L-8 · low · confidence: high — Real lifecycle invariants exist only as
skill convention.**

- Evidence [observed]: §2.3 — "≤1 live covering proposal per WI" and
  "done proposals cover only terminal WIs" both hold with zero violations,
  and nothing but `refill-queue`/`sprint-ship` discipline maintains them.
- Impact: latent; a misbehaving writer breaks planning-surface assumptions
  silently.
- Recommendation: D-19 — document both invariants in `docs/api.md` as
  conventions with their enforcement point named (the skills), and stop
  there. DB/app enforcement is deliberately not recommended: re-proposal
  flows legitimately pass through transient states, and the cost of a
  wrong hard rule exceeds the observed risk (zero violations to date)
  [preference].

---

**L-9 · low · confidence: high — The label column is unindexed (F-25
carry-over).**

- Evidence [observed]: `\d relationship` — btree on `left_id`, `right_id`,
  the pair-unique triple, nothing on `relationship` alone.
  `project_edges` filters on it across a project join
  (crates/korg-core/src/repo.rs:877-892).
- Impact: none at 256 rows. Fold `CREATE INDEX ... ON relationship
  (relationship)` into LB-1's migration since it is touching the table
  anyway; do not ship a migration for this alone.

---

**L-10 · low · confidence: medium — The pair-unique constraint is
orientation-sensitive, so undirected labels have a latent duplicate
channel.**

- Evidence [observed]: `UNIQUE (left_id, right_id, relationship)` admits
  both `(A,B,related-to)` and `(B,A,related-to)`. Zero reverse-duplicate
  pairs exist live (checked across all labels). D-1 chose reader-side
  symmetry deliberately; the write side was left open.
- Impact: latent dup channel for `related-to`; would double-count in
  neighbor lists.
- Recommendation: in LB-2, `relate()` on a registry-*undirected* label
  checks for the reverse edge and returns its `rel_id` as the dedup
  (mirroring the existing same-orientation `ON CONFLICT` no-op). No
  migration needed — the corpus is already clean [preference].

---

## 4. The questions, answered

WI #568 posed eight questions. Each answer states the recommendation and
the decision it hangs on; §5 carries the alternatives and costs.

### 4.1 Vocabulary: closed, open-with-fast-path, or namespaced?

**Closed** (D-11). The corpus is the argument: free-form invention
produced two synonyms and one candidate registration in 256 edges, and
zero durable value that registration wouldn't have captured better. The
counterargument for open vocabularies — "agents need expressiveness
mid-task" — is answered by the actual failure mode: agents didn't need
new semantics, they needed *steering toward existing ones*, which a
closed set with sprint-017-style errors ("unknown label `related`;
registered labels are …; did you mean `related-to`?") provides at the
moment of writing, self-documentingly. Namespacing (`korg:*` vs
`agent:*`) was considered and rejected: it preserves invention while
marking it, but the evidence says invention *is* the failure — namespacing
would merely organize the noise [preference]. Extension stays one Rust
edit away, and `has_handoff` will prove the path.

### 4.2 Enforcement: what, and where?

**App-side in korg-core, nothing in the database beyond what exists**
(D-12). `relate()` is the single choke point both transports share; it
gains label-membership and endpoint-kind validation from the registry the
module already declares. DB-level enforcement (label CHECK, kind
triggers) was weighed and rejected: the DB is written only through core
(operations.md documents direct SQL as read-only), a CHECK would need a
new migration per vocabulary change, and a kind-validating trigger
duplicates the registry outside Rust — recreating the drift class B4
eliminated [preference]. Migration story for nonconforming edges: none
needed at enforcement time, because LB-1's fix-ups run *first* and its
migration asserts corpus conformity as a postcondition; enforcement then
only ever faces new writes.

### 4.3 Provenance, time, ordering: which become load-bearing?

**Time and origin now; ordering never; audit not yet** (D-17). `created
timestamptz NULL` (NULL = predates provenance, honestly) and `origin text
NULL` (self-reported writer tag: `web`, `start-sprint`, `kmon`,
`propose_sprint`, …). Origin is attribution, not authentication — korg is
no-auth HTTP, so it rides the payload; that is the same conclusion WI
#466 reached for nodes, and the two should stay one mechanism (same
column name and semantics if #466 later adds it to `node`). Ordering:
every ordering need found (covered items, future handoffs) is served by
the *node* side (wi_number, handoff.updated) — an edge ordinal has no
consumer; skip it. An append-only edge-audit sidecar (which would make
`unrelate` — including sprint-ship's deferral deletes — reconstructable)
has real appeal and no current consumer; explicitly deferred until
handoffs or multi-agent flows demand edge history (L-3, D-17).

### 4.4 Cardinality: may two proposals cover one WI?

**Yes — it already happens legitimately and must stay legal** (D-19).
Multi-coverage is re-proposal history (declined → re-proposed, round-2
spillover); forbidding it would falsify real history. The invariants
worth stating are lifecycle ones ("≤1 *live* covering proposal", "done
proposals cover terminal WIs"), which hold today by skill convention and
should be documented as such, not enforced (L-8).

### 4.5 The legacy `covers` overload?

**Convert the five bundles into real proposals** (D-13, recommended
against two alternatives). The five `Sprint: …` WIs are, semantically,
sprint proposals that predate the `sprint_proposal` kind — the conversion
writes the history the way it would have been written. After it, `covers`
has exactly one shape, the registry's `left_kind` becomes a true corpus
invariant that LB-1 asserts, `docs/api.md` loses its caveat section, and
`LEGACY_SPRINT_TITLE_PREFIX` is deleted. Cost: one migration creating 5
archived `done` proposals (titles/timestamps carried from the WIs),
re-pointing 27 edges, leaving the WIs archived in place; fully
rehearsable with exact expected counts.

### 4.6 Reads: is `neighbors` the right primitive?

**Yes, as the generic floor — with typed context inlined where workflows
live** (D-20). `get_proposal` proved the pattern: the hot workflow gets a
typed read; `neighbors` remains for everything else. The gap is L-6:
focused reads must stop hiding edge context. Generalize the two-level
contract to edges — `get_work_item` inlines a compact related-nodes
summary (grouped by label, capped, truncation-flagged), shaped by payload
measurement per the handoff plan's own method. No traversal DSL, no
generic graph query surface [preference]. `NeighborPage`'s
cap-not-paginate shape stays as documented (WI #579's uniformity question
is unaffected either way).

### 4.7 Performance and scale?

Non-issue at 256 edges [observed]. Add the label index while LB-1 touches
the table (L-9); `neighbors` is already bounded and per-node-indexed.
Nothing else until edge volume changes regime.

### 4.8 What lets an agent trust an edge another agent wrote?

Three legs, two of which are this review's work: **the label means one
thing** (closed vocabulary + enforcement — an edge that exists is an edge
korg validated), **the edge says who and when** (origin + created, honest
about being self-reported), and **the read path can't hide it** (L-6's
inlined context). The fourth leg — payload-carrying links with authoring
conventions — is the handoff node itself, which this layer now cleanly
precedes.

---

## 5. Decisions — RESOLVED with Ken, 2026-07-23

Numbering continues the 2026-07 review's resolved D-1..D-10. All ten were
resolved on 2026-07-23 with **every recommended option accepted**; the
"recommended" markers below are therefore the outcomes. Filed as proposals
`korg:596` (LB-1), `korg:597` (LB-2), `korg:598` (LB-3).

- **D-11 — Vocabulary policy.** (a) **Closed registry — recommended**:
  unregistered labels are `invalid_input`; UI `custom…` removed; extension
  = registry edit (§4.1). (b) Namespaced (`agent:*` free tier). (c) Status
  quo, docs-only. Choosing (b)/(c) voids L-7's auto-resolution and halves
  the value of D-12.
- **D-12 — Enforcement point.** (a) **korg-core app-side — recommended**
  (§4.2). (b) Add DB trigger backstop too. (c) Docs only.
- **D-13 — Legacy `covers`.** (a) **Convert bundles #108–112 to archived
  done proposals — recommended** (§4.5). (b) Relabel the 27 edges to a
  registered legacy-only label (e.g. `bundled`) — cheaper, keeps a
  permanent one-off label. (c) Keep the documented dual shape — blocks
  the covers kind-invariant forever.
- **D-14 — `related` → `related-to`.** (a) **Consolidate the 7 edges —
  recommended**; zero collisions verified. (b) Leave as caller-defined.
- **D-15 — `part_of`.** (a) **Convert to `parent_node_id` (set on WIs
  #400–402, delete the 3 edges) — recommended**: one mechanism for
  subtask structure, and the relate error text steers to it. (b) Register
  `part_of` and accept two mechanisms.
- **D-16 — `follows_from`.** (a) **Relabel the 1 edge `related-to`;
  note `follows_from` as a candidate registration when a workflow needs
  lineage — recommended** (one edge ≠ proven need). (b) Register it now
  (directed, "successor follows from predecessor").
- **D-17 — Provenance.** (a) **Add nullable `created` + `origin` columns;
  writers stamp origin; NULL = pre-provenance — recommended** (§4.3).
  (b) Also add an append-only edge-audit table now. (c) Neither.
  Coordinate the `origin` semantics with WI #466 whichever way it goes.
- **D-18 — Proposal project backfill.** (a) **Backfill the 6 unanimous;
  Ken names #175's project (its covered-WI majority is kdeskdash) —
  recommended.** (b) Backfill 6, leave #175 null.
- **D-19 — Lifecycle invariants.** (a) **Document as convention in
  api.md, no enforcement — recommended** (§4.4). (b) Enforce "≤1 live
  covering proposal" in `propose_sprint`.
- **D-20 — Edge context on focused reads.** (a) **Adopt the contract
  principle now; shape it by payload measurement in LB-3/handoff sprint —
  recommended** (§4.6). (b) Leave edge discovery to `neighbors` and
  handle only `has_handoff` specially later.

---

## 6. Dependency-ordered bundles

Sized in this repo's convention (S ≈ half a day, M ≈ 1–2 days, L ≈ 3+ of
focused agent work). LB-1 → LB-2 are strictly ordered; LB-3 can trail or
fold into the handoff sprint.

---

**LB-1 — Corpus true-up: data fix-ups + provenance schema** · size M ·
prereqs: decisions D-13..D-18 resolved

One migration (plus its rehearsal) closing L-3(schema half), L-4, L-5,
L-9 and the data sides of L-2:

- `related` → `related-to` (7 edges), `follows_from` → `related-to`
  (1 edge) — per D-14/D-16.
- `part_of` → `parent_node_id` on #400–402, delete 3 edges — per D-15.
- Legacy bundle conversion: 5 archived `done` proposals created from WIs
  #108–112 (title sans `Sprint: ` prefix, node timestamps carried), 27
  `covers` edges re-pointed — per D-13.
- Project backfill on the 7 project-less proposals — per D-18.
- `ALTER TABLE relationship ADD COLUMN created timestamptz, ADD COLUMN
  origin text` (both NULL; no backfill lies) + label index — per D-17,
  L-9.
- **Postcondition, asserted in the migration**: every label ∈ registry;
  every `covers` edge is `sprint_proposal → workitem`; every `finding`
  edge is `report → workitem`; per-label counts match the rehearsed
  expectation; zero reverse-duplicate undirected pairs.
- `docs/api.md`: delete "One legacy shape"; delete
  `LEGACY_SPRINT_TITLE_PREFIX` and its 0014 references from prose.

Rehearsal is mandatory per the `docs/operations.md` recipe: restored
nightly dump → baseline counts → migrate → diff. Expected deltas are
exactly enumerable in advance (7+1 relabels, 3 edge deletes, 27
re-points, 5 node inserts, 6–7 project updates).

Blast radius: readers of `related`/`part_of`/`follows_from` labels — none
exist in code or skills [observed: grep + skills audit]; the Planning
page and `get_proposal` see 5 new archived done proposals (hidden by
default filters); `plan-status`/`refill-queue` untouched (`depends_on`
byte-identical).

---

**LB-2 — Enforcement + provenance write path** · size M · prereqs: LB-1
deployed (corpus conforms before writes are constrained)

Closes L-1, L-2 (code half), L-3 (write half), L-7, L-10:

- `relate()` consults the registry: unregistered label → `invalid_input`
  naming the vocabulary and near-miss; kind-constrained labels validate
  both endpoint kinds → `invalid_input` naming the expected kinds.
  (Sprint-017 error style; the error is the documentation.)
- Undirected-label reverse dedup: `relate(A,B,related-to)` with
  `(B,A,related-to)` extant returns the existing `rel_id` (L-10).
- `origin` accepted on `relate` / `propose_sprint` / `create_report`
  (optional, free text); internal writers default to their operation
  name; web client sends `web`; `created` stamped by default on insert
  (the `ON CONFLICT` no-op preserves the original).
- UI: `custom…` escape hatch removed; select is the registry (D-11).
  Optionally export `left_kind`/`right_kind` to the generated vocab so
  the UI can steer target pickers.
- Skills touched: any that `relate` (currently none write labels outside
  the registry [observed]); `sprint-ship`/`refill-queue` descriptions
  gain the origin convention.
- Tool descriptions + `docs/api.md` rewritten: vocabulary is closed;
  "free-form labels are legal" deleted; provenance documented as
  self-reported.
- Tests: contract tests for the new `invalid_input` cases on both
  transports; dispatch fixture updated; corpus-conformity assertion moves
  from migration to a standing test against the test DB.

Blast radius: any writer using an unregistered label breaks loudly — the
grep says none exists; the *risk* is an unknown external caller, which is
precisely what the error message is designed to redirect.

---

**LB-3 — Edge context in focused reads** · size M–L · prereqs: LB-2;
pairs naturally with (or folds into) the handoff sprint

Closes L-6, implements D-20:

- `get_work_item` (both transports, same shape) gains inlined related-node
  context: compact refs grouped by label, capped, truncation-flagged —
  exact shape driven by payload measurement at production scale, per the
  handoff plan's stated method. `get_proposal` extends the same way for
  labels beyond `covers`.
- Web work-items detail consumes it, deleting the second fetch.
- Skills: `start-sprint` step-7 briefing and `sprint-ship`'s deferral pass
  read the inlined context instead of calling `neighbors`.
- If run as part of the handoff sprint: `has_handoff` registers in the
  same change, and the handoff read contract lands on the generalized
  mechanism rather than as a special case — the plan's
  `handoff_count`/`handoffs`/`handoffs_truncated` become one instance of
  the general shape.

---

**Sequencing against the handoff plan.** LB-1 + LB-2 are the linking
layer's answer to the handoff plan's precondition and should land before
it; LB-3 can be the handoff sprint's first half. After LB-2, registering
`has_handoff` is a one-line registry entry whose direction, endpoint
kinds, and provenance are enforced from birth — the extension procedure
working as designed on its first real customer.

---

## 7. Migration mechanics, downtime, and rollback

- **No downtime is required.** All LB-1 changes ride one sqlx migration,
  transactional, touching ≤32 rows against a ~700-node database; it runs
  in the normal deploy stop-start window like 0014 did. The offered
  offline/rebuild appetite is appreciated and declined [preference].
- **Rehearsal**: the standing `docs/operations.md` recipe (restore
  nightly dump → baseline → migrate → diff), with every expected count
  enumerated in this document beforehand. Sprint 014 is the precedent —
  its rehearsal is what caught the 27-edge premise failure this review
  inherits.
- **Rollback**: nightly dump before deploy (the deploy skill's preflight
  already checks currency); LB-1's migration asserts its postcondition
  and refuses to apply rather than half-applying (0014's pattern).
  Rolling back across the migration boundary is a restore, as
  operations.md documents.
- **Ordering**: fix-ups (LB-1) strictly before enforcement (LB-2), so
  enforcement never faces a nonconforming corpus and needs no grandfather
  clauses.

---

## 8. Validation log

Environment: kai; production reads via
`ssh kubsdb bash -s <<'EOF' … docker exec postgresql psql -U korg -d korg
-tAc "…" EOF` (fish-shell caveat honored). All queries read-only;
production untouched. Executed 2026-07-23.

Queries and results (condensed; full outputs in the session transcript):

1. Label inventory: covers 212, depends_on 23, related 7, finding 5,
   related-to 5, part_of 3, follows_from 1. Total 256.
2. Endpoint-kind matrix: §2.1 table verbatim; 10 combinations.
3. `\d relationship`: 4 columns; indexes left/right/pair-unique only;
   `relationship_no_self_edge` CHECK; FKs `ON DELETE CASCADE`.
4. Multi-coverage: 27 WIs with >1 covers edge; among `sprint_proposal`
   writers only: WIs 24, 61, 62, 88, 103 (2 each); statuses decoded
   (declined+done; done+done round-2 pairs).
5. Cross-project covers: 33 edges; 32 from 7 project-less proposals
   (174, 175, 177, 178, 183, 244, 288 — all terminal; 6 unanimous
   backfill targets; 175 spans 2); 1 real (kdeskdash → k-homelab).
6. depends_on projects: homelab-ai 21, kmon 1, kvscf 1 — all
   intra-project.
7. Edges touching archived nodes: covers 27 (the legacy bundles),
   finding 2.
8. Proposal→WI status matrix: done→{closed 117, resolved 41, done 7};
   proposed→open 15; active→open 2; declined→closed 3. No done→open.
9. Live-proposal overlap: zero WIs covered by two non-terminal proposals.
10. `related`/`related-to`/`part_of`/`follows_from` edges dumped with
    endpoint titles (§2.2); part_of targets' `parent_node_id` all NULL;
    zero `related`↔`related-to` pair collisions in either orientation;
    zero reverse-duplicate pairs any label.
11. `max(rel_id)` 310 vs count 256; node kinds: workitem 386,
    sprint_proposal 57, card 27, report 23, link 4, daily_plan_item 4.
12. Legacy covers left nodes: exactly #108–112, titles confirmed.

Code verification: `crates/korg-core/src/relationships.rs` (full read) and
`repo.rs:779-808` (`relate`) verified line-by-line by the reviewer;
remaining code citations from a dedicated code-map pass over
`repo.rs`/`ops.rs`/`tools.rs`/migrations/web routes/skills, spot-checked.
Sprint-delta inventory (013–020) compiled from the sprint READMEs and
`git log`. klams was searched for prior cross-agent linking conventions:
nothing beyond what this repo already records.

Not done / limitations: no scratch-instance write probes (the 2026-07
review's method) — this review's write-behavior claims rest on code
reads and the existing contract tests rather than live probes; corpus
numbers are a point-in-time snapshot; `origin` self-reporting is a design
judgment, not something validated against a misbehaving writer.

---

*End of review. Decisions D-11..D-20 resolved with Ken 2026-07-23 — all
recommended options accepted (§5). The bundles are filed in korg as
proposals `korg:596` (LB-1, WIs #585–588), `korg:597` (LB-2, WIs
#589–592) and `korg:598` (LB-3, WIs #593–595), tag `linking-2026-07`,
ranked ahead of the existing queue in dependency order. Produced on
branch `021-linking-layer-deep-review` for proposal `korg:569` /
WI #568.*

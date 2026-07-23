# Sprint 022 — LB-1 corpus true-up: data fix-ups + provenance schema

Proposal `korg:596`, covering WIs #585–588. First of the three
`linking-2026-07` bundles from the
[021 linking-layer deep review](../planning/2026-07-23-linking-layer/SUMMARY.md)
(decisions D-13..D-18, resolved with Ken 2026-07-23).

One rehearsed, postcondition-asserting migration pass that makes the live
corpus conform to the label registry **before** LB-2 (`korg:597`) turns on
write-time enforcement. Enforcement must never face a nonconforming corpus,
so this bundle is **strictly before** LB-2.

Nothing here changes the write path — that is LB-2. This sprint is a single
new SQL migration (`0016`) plus the code/docs cleanup the conversion makes
possible, rehearsed against a restored nightly dump and deployed on the
normal window (≤32 rows touched, no downtime).

## Why a `.sql` migration, and why it is safe to auto-apply

korg runs `sqlx::migrate!("./migrations")` at startup
([`lib.rs`](../../crates/korg-core/src/lib.rs)); each file runs in its own
transaction. So the migration ends with a `DO $$ … RAISE EXCEPTION $$`
postcondition block: if the corpus does not reach the asserted end-state the
whole file rolls back and startup fails loudly — **it refuses to apply rather
than half-apply** (0014's pattern, promoted to a standing requirement by the
review).

Reads against production to design this were done read-only per
`docs/operations.md`; the migration itself is rehearsed against a restored
scratch dump before it goes near production (see [Rehearsal](#rehearsal)).

## The four slices → one migration

`crates/korg-core/migrations/0016_linking_corpus_true_up.sql`, sectioned like
0014. All figures are the **2026-07-23 production baseline** — production
drifts with normal use, so re-baseline at rehearsal time and recompute
(see the [covers note](#the-covers-count-a-worked-example-of-why-you-re-baseline)).

### §1 — Consolidate off-registry labels → `related-to` (#585, D-14/D-16)

Decoded, the off-registry edges are two synonyms of an existing mechanism, not
extensions:

- 7 `related` edges → `related-to`. Pairs (by node id): (2,3) (2,4) (3,4)
  (34,35) (229,234) (229,235) (460,278) — all stored `left<right`.
- 1 `follows_from` edge (rel_id 259, 474→472) → `related-to`. D-16: not worth
  registering `follows_from` now; revisit if a workflow needs lineage
  semantics. Left as stored (474→472) — `related-to` is undirected, readers
  ignore orientation, and there is **no** canonicalization (D-1).

Live-verified safe: **zero** of these 8 pairs collide with an existing
`related-to` edge in either orientation, so the relabel cannot trip
`relationship_pair_label_unique` (0006). If one did, the constraint would
error and the whole migration would roll back — the safety net working as
designed.

### §2 — `part_of` → `parent_node_id` (#585, D-15)

`parent_node_id` is korg's one subtask mechanism; `part_of` re-invented it.
The 3 `part_of` edges are (child→parent): 400→277, 401→277, 402→277, with the
child on the left. Set `workitem.parent_node_id` from the edge's right end,
then delete the edges. WIs #400–402 have `parent_node_id` NULL today, so this
is a clean set, not an overwrite.

### §3 — Convert the five legacy `Sprint:` bundles to real proposals (#586, D-13)

The pre-0008 bundles are, semantically, sprint proposals that predate the
`sprint_proposal` node kind — WIs #108–112 (`kind=workitem`, titled
`Sprint: …`, project **Agent-Plan**, already archived). Each is the **left**
end of its `covers` edges (0014 oriented them bundle→member):

| bundle WI | title (`Sprint: ` stripped) | covers edges |
|---|---|---|
| #108 | korg dogfood fixes | 2 |
| #109 | Reliability bug-bash — klams + kdeskdash | 9 |
| #110 | kapollo core UX | 5 |
| #111 | hv-simulator dashboard pass | 5 |
| #112 | Small-tools polish — klams + kpidash | 6 |

**27** edges total. For each bundle, in one plpgsql loop:

1. Insert a `sprint_proposal` node — `kind='sprint_proposal'`, `archived=true`,
   `project`/`created`/`updated`/`tags`/`category` **carried from the source
   WI's node** (the history written the way it would have been written).
2. Insert the `sprint_proposal` detail — `title` = WI title minus the
   `Sprint: ` prefix, `summary` = the WI's `content` (the bundle's own
   description, which already ends in a `Covers: …` line), `status='done'`,
   `pinned=false`, `rank` = the source `wi_number` (stable, irrelevant once
   archived+done).
3. Re-point that bundle's `covers` edges from bundle-node → new-proposal-node
   (`UPDATE relationship SET left_id = <new> WHERE left_id = <bundle> AND
   relationship='covers'`). No collision risk: the proposal is brand new.

The **source WIs are kept** — archived, in place, now with no `covers` edges.
Nothing is deleted.

After this, `covers` is exactly `sprint_proposal → workitem` corpus-wide,
retiring the "one legacy shape" caveat for good. New proposals carry project
**Agent-Plan**, so their cross-project `covers` edges (Agent-Plan proposal →
korg/klams/… WI) are expected and honest — the rehearsal diff will show ~27
new cross-project covers edges from the conversion; that is not a regression.

Code/docs cleanup rides in the same change (see [Code & docs](#code--docs-cleanup)).

### §4 — Provenance columns + label index (#587, D-17 schema half / F-25)

- `ALTER TABLE relationship ADD COLUMN created timestamptz NULL, ADD COLUMN
  origin text NULL`. NULL means "predates provenance" — **no backfill lies**.
  The write-path half (stamping `created`/`origin` on new edges) is LB-2; a
  migration comment notes that LB-2's `relate()` `ON CONFLICT` no-op must
  **preserve** the original `created`/`origin`.
- `CREATE INDEX relationship_label_idx ON relationship (relationship)` — the
  label column is named `relationship`; there was no index on it (F-25).

### §5 — Backfill project on the project-less proposals (#587, D-18)

Applies D-18's **rule**, not a snapshot's node-id list — see the
[rehearsal finding](#the-finding-that-earned-the-rehearsal). Each project-less
proposal takes the unanimous project of the WIs it covers; #175 is the lone
ambiguous case (kdeskdash 5 vs klams 4) and is Ken's call: kdeskdash. A
proposal that spans two projects or covers nothing is left for a human and
caught by the postcondition, not guessed at.

The touch-updated trigger is disabled across the backfill so these frozen
records keep their real `updated` — a project fix-up is not a content edit.
After this, **zero** project-less `sprint_proposal` nodes remain (the 5 new
ones from §3 already carry Agent-Plan). The one genuine cross-project edge —
proposal node **485** (kdeskdash) → WI #484 (k-homelab), already
project-assigned — is untouched and still reads cross-project afterward
(rehearsal-confirmed).

## Code & docs cleanup

Ships with the migration so `main` and the deployed image stay honest:

- Delete `korg_core::relationships::LEGACY_SPRINT_TITLE_PREFIX`
  ([relationships.rs:79-84](../../crates/korg-core/src/relationships.rs#L79-L84))
  — it has no remaining Rust callers; its only uses were 0014's comment and
  the api.md caveat, both retired here.
- Delete the "One legacy shape" section of `docs/api.md`
  ([api.md:253-261](../../docs/api.md#L253-L261)).
- Update the api.md "History" section to note the 0016 conversion (the legacy
  work-item bundles became real archived-done proposals; the dual shape is
  gone).

## Postconditions (asserted in-migration; rollback on failure)

The migration's final `DO` block raises — and rolls the file back — unless all
hold:

1. Every `relationship` label ∈ registry (`covers`, `finding`, `depends_on`,
   `related-to`) — no `related` / `follows_from` / `part_of` survive.
2. Every `covers` edge is `sprint_proposal → workitem` (left kind and right
   kind both checked; `left_not_proposal` must be 0).
3. Every `finding` edge is `report → workitem`.
4. Zero reverse-duplicate undirected pairs — no `(a,b)` and `(b,a)` both
   present under `related-to`.
5. Zero project-less `sprint_proposal` nodes.
6. WIs #400–402 have `parent_node_id` = node #277, and zero `part_of` edges
   remain.
7. Provenance columns exist and are NULL on every existing edge.

Per-label and per-kind **counts** are checked by the rehearsal, not hardcoded
in the `.sql` (the review's explicit instruction — production drifts).

## Rehearsal — done, twice

Per `docs/operations.md` §"Rehearsing a data migration", against **scratch**
DBs, never production. Rehearsed on 2026-07-23 against two substrates, both
applied in a single transaction (`psql --single-transaction -v
ON_ERROR_STOP=1`, matching sqlx's per-migration transaction):

1. **Last night's dump** (`korg-20260723-032356.sql.gz`, 03:23) restored into
   `korg_scratch` — 11 project-less proposals, `covers`=212.
2. **A live-prod snapshot** (`pg_dump korg` → `korg_scratch2`) — 7 project-less
   proposals, `covers`=223, the exact shape the deploy will migrate.

Both **applied cleanly and committed** (postcondition passed); all structural
probes came back 0, the label index present, provenance columns NULL
everywhere, and the five converted bundles kept the source WIs' 2026-07
timestamps rather than today's. An earlier apply as the `korg` login role hit
`permission denied for schema public` (a scratch-ownership artifact: `korg`
owns the schema in the real DB but not in a `postgres`-owned scratch copy) —
and the single transaction rolled back with **nothing** committed, which
doubled as a live demonstration of the refuse-rather-than-half-apply property.

### The finding that earned the rehearsal

The first backfill draft hardcoded the seven project-less proposal node ids
from a live query. The dump exposed the trap: it held **11** project-less
proposals where production held **7** — four non-terminal ones
(#176/#184/#185/#186, kapollo/hv-simulator) had been triaged a project in the
hours between the 03:23 dump and the query. A node-id list is coupled to the
instant it was written; the deploy would have run against a *third* state.
D-18's underlying rule — unanimous covered-WI project, #175 by name — is
snapshot-independent, and all eleven were cleanly unanimous but #175. §5 was
rewritten to that rule; it then converged **both** substrates (11 and 7
project-less) to zero. Same class of surprise `docs/operations.md` promises:
the interesting number is the one you did not predict.

Enumerated deltas (dump baseline shown; prod baseline in parens where it
differs — both rehearsed):

| delta | before → after (dump / prod) |
|---|---|
| `related` edges | 7 → 0 |
| `follows_from` edges | 1 → 0 |
| `part_of` edges | 3 → 0 |
| `related-to` edges | 5 → **13** (5 + 7 + 1) |
| `covers` edges | 212 → 212 (dump) · 223 → 223 (prod) — re-pointed, not added |
| `depends_on` / `finding` | 23 / 5 → unchanged |
| `sprint_proposal` nodes | 57 → 62 (dump) · 60 → **65** (prod), +5 |
| `workitem` nodes | unchanged (bundles kept) |
| `covers` left-not-proposal | 27 → 0 |
| WIs #400–402 `parent_node_id` | NULL → #277 |
| project-less proposals | 11 → 0 (dump) · 7 → 0 (prod) |
| `relationship` columns | +`created`, +`origin` (NULL everywhere) |
| index | +`relationship_label_idx` |

### The `covers` count: a worked example of why you re-baseline

The 021 review predicted final `covers` = **212**; production shows **223**.
The +11 is not an error — it is the covers edges the three LB proposals
(`korg:596/597/598`) created when the review filed them (4 + 4 + 3), which the
03:23 dump (also 212) predates. The migration only re-points `covers`, so its
count is invariant on any substrate: whatever the rehearsal baseline reads is
the expected after-value, confirmed 212→212 on the dump and 223→223 on the
prod snapshot. This is exactly the drift #588 warns against hardcoding around.

## Deploy

Standard `deploy-kubsdb` window — the skill's preflight already does the
backup-currency check that makes an auto-applied migration survivable.
Migration runs at container startup; verify after with the postconditions
above (the migration self-asserts, but confirm the counts on the live DB) and
`scripts/post-deploy-check.sh --compare`.

Then LB-2 (`korg:597`) can constrain writes against a corpus that already
conforms.

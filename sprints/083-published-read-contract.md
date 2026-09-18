# 083 — A published read-shape contract, so a consumer's CI meets a shape change first

Proposal korg:2815, slice 3 of program korg:2816 ("Clear the korg backlog").
Run as an overseen karc leg (`korg-328c86`) on kai.

Covers WI 2041.

## Goal

Two consumers copied the same korg sink and diverged. When korg's list reads
moved to `{items, total, limit, offset}` envelopes, kmon's copy crashed daily
for 11 days; kyac's copy silently wrote nothing for two months. Both are now
loud when they drift — kmon through its hardened `korg_api.py` with a named
`KorgShapeError` (WI 899), kyac by reading back what it claims to have written
(WI 2040). What neither can fix alone is that **nothing propagates a korg-side
shape change to them.** Each learns about it from its own failure.

Ken's decision (2026-09-17, recorded on the proposal and on 2041): **option 2,
the published contract.** korg publishes a fixture set of its read shapes;
consumers assert against it in their own suites. The shared `korg-client`
distribution is not being built.

## Premise check

**2041 — premise holds.** Measured against the tree on 2026-09-17:

- **The envelope forms are as the item describes.** `repo::Page<T>` is
  `{items, total, limit, offset}`; `WorkItemListLean` is that plus `omitted`;
  the four filtered reads return `{items, omitted}`; six reads are still bare
  arrays. `docs/api.md`'s shape table agrees with the code.
- **The lean-vs-full projection difference holds, and is the silent half.**
  `WorkItemSummary` carries no `tags`, no `content`, no `details`, no
  `sprint`. `WorkItemRow` carries all of them. A dedup built on a lean row's
  `tags` reads `None` rather than raising, every run, forever.
- **korg publishes nothing a consumer can pull.** This is the part worth
  stating precisely, because korg's *internal* fences are strong and it would
  be easy to mistake them for the artifact: `dispatch.rs`'s
  `collection_reads_return_the_shape_the_instructions_promise` asserts the
  documented class against the real wire shape; `docs_drift` holds
  `docs/api.md`, the MCP instructions and the code in agreement;
  `tools_schema.json` snapshots the tool surface. Every one of them fires
  **inside this repo's CI**. `tools_schema.json` publishes *input* schemas, not
  response shapes. `web/src/lib/generated/korg.ts` publishes response types —
  in TypeScript, inside `web/`, to a consumer that clones korg. kmon and kyac
  are Python and clone nothing. So a korg shape change is invisible to them
  until production, which is exactly what the item says.

## Guiding plan

korg routes to `korg+/` in the cross-project planning index.

- **GP-14 — a sampled enum is not an enum**, and its "pin the specimen" half is
  the design constraint on this whole sprint. A published contract *is* a claim
  about korg's value domain, so it must not be read off whatever rows a seed
  happened to contain. Two consequences, both load-bearing:
  - the seed deliberately carries the variant instances (an archived project, a
    non-`active` project, a `closed` work item, an untagged row beside a tagged
    one), so a key that is only sometimes present is *observed* rather than
    assumed absent;
  - key presence is measured across rows, not read off one — `optional_keys`
    is derived from rows that disagree, which is the only mechanism serde has
    for a sometimes-absent key (`skip_serializing_if`, live today on
    `ProjectSummary::status`).
- **GP-14's mirror image** — a hypothetical drawn from korg's plausible future
  expires silently. The exhaustiveness gate here asserts against the **live
  advertised tool list**, never a hand-copied list of reads, so it cannot go on
  passing after korg grows a read.
- **GP-1 — data korg cannot hold is a korg work item first; consumers never
  grow side stores.** The published contract is the read-side counterpart:
  a consumer that needs to know korg's shapes gets them *from korg*, rather
  than keeping a hand-copy that drifts. That is the same argument in the same
  direction, and it is why this artifact belongs in korg rather than in a
  shared client both consumers vendor.
- **GP-13's consumer half** — where korg says nothing, the consumer renders
  nothing. The contract states `full_read: null` explicitly for a collection
  with no fuller counterpart, rather than omitting the field: "there is no more
  than this" is an answer, and a missing key is not.

This sprint **refines GP-14** rather than contradicting it, and the plan is
amended in this ship: GP-14 tells a consumer to "ask korg's schema for the
domain", and until now there was nothing to ask for response shapes. See the
amendment note at the end of this record.

## Design decisions

Comment 2510 left three things for this sprint to settle. Settled:

1. **Serialised as JSON**, one document, `contract/read-shapes.json` at the
   repo root. Language-neutral is the whole requirement: the consumers are
   Python, korg's own web app is TypeScript, and JSON is what all three read
   with no dependency. Repo root rather than beside a test snapshot because
   this one is *published* — it is meant to be found.
2. **Pulled over HTTP from korg itself**: `GET /api/contract/read-shapes`.
   A committed file alone is not pullable — kmon and kyac do not clone korg —
   and this is the decision that makes the artifact actually reach them. The
   endpoint serves the **embedded committed document** (`include_str!`), not a
   live regeneration: a consumer needs to know what *the korg it is talking to*
   promises, the drift gate guarantees the file matches the code that built the
   binary, and regenerating live would need a seeded database that production
   does not have. The chain is consumer ← endpoint ← embedded file ← drift gate
   ← real dispatched wire shapes.
3. **Which reads are covered**: every advertised collection read, exhaustively
   and by assertion — that is the envelope register, and the exhaustiveness is
   what makes it a contract rather than a sample. Plus the full-read
   counterpart for each collection that has one, which is what expresses the
   lean-vs-full projection difference the silent damage came from. Singleton
   node shapes beyond those counterparts are *not* covered: that is the whole
   node-shape surface and a different job.

The shapes are **generated from real dispatched responses**, never hand-written
— the same choice `tools_schema.json` made, for the same reason. A hand-written
contract is a second source of truth, and this item exists because a hand-copy
drifted.

## What shipped

**The document** — `contract/read-shapes.json`. All 15 collection reads, each
with its shape class, envelope keys, `omitted` keys, the keys every row carries,
the keys **some** rows carry, the fuller read where korg has one, and the
difference in both directions. Generated, never written.

**The generator and its gates** — `crates/korg-mcp/tests/read_shapes.rs`, six
tests. One needs a database and does the measuring; the other five judge the
**committed file**, which is the artifact a consumer actually receives, and need
no Postgres:

- `the_published_contract_matches_the_wire` — regenerate and compare. The drift
  gate.
- `every_advertised_collection_read_is_published` — the covered set equals the
  live advertised tool list. A new collection read fails the build until it is
  published; a hand-copied list of reads is the thing this item is about.
- `every_documented_shape_class_is_one_a_read_returns` — both legends, both
  directions. GP-14's mirror image: a legend entry for a shape korg does not
  emit expires silently.
- `the_conditional_project_status_key_stays_pinned` — the pinned specimen.
- `the_lean_work_item_row_is_published_as_missing_tags` — the silent half,
  named.
- `every_published_entry_is_complete` — no entry may omit a key, because a
  consumer reading an absent `only_on_full_read` concludes the row is complete.

**The pull** — `GET /api/contract/read-shapes`, embedded with `include_str!` and
served byte-for-byte, plus `crates/korg-api/tests/sprint083.rs`. Byte-identity
is deliberate: hashing what you fetched against what you vendored is the whole
staleness check, and it only works if the endpoint does not re-serialise.

**The drift gate** — `contract/read-shapes.json` joins `just gen` and
`gen-check`'s fingerprint, and the CI workflow mirrors it. This is the first
generated artefact that needs a database; testcontainers supplies it in both
places.

**Docs** — a `### The published read-shape contract` section in `docs/api.md`
(what it states, the four enforced properties, and separate "for a consumer" /
"for korg" halves), the REST row in `docs/usage.md`, and a README pointer aimed
at somebody about to write a korg client.

## The gates were perturbed, because a test that cannot fail is not a gate

Each was made to fail before it was trusted:

| Perturbation | Result |
|---|---|
| Drop `list_work_items` from the document | 3 fail: the snapshot, the exhaustiveness gate, the lean-row gate |
| Claim `list_projects.status` is always present | 2 fail: the snapshot and the pinned-specimen guard |
| Weaken the seed (archived project → active), then regenerate | **the snapshot passes** — it honestly matches the weakened seed — and the pinned-specimen guard is the only thing that fails |

The third is the one worth keeping. It is GP-14's failure exactly: the document
stayed *internally consistent* while quietly ceasing to describe a key korg
really emits, and only a test that knows what the corpus is *for* catches it.

## Acceptance: fired, not deferred

The criterion on 2041 is "the fixtures exist and a consumer can assert against
them", and the second half is the one that needed firing rather than asserting.
So a consumer-shaped check (`.scratch/consumer_check.py`, read-only, changed
nothing in kmon) ran kmon's own documented assumptions against the published
document. Everything kmon branches on held — its envelope-vs-bare-array split
is correct for all five reads it names.

It also found drift on its first use, which is the artifact doing its job before
it was even adopted. See "Findings for the consumer slices" below.

## Findings for the consumer slices, not repaired here

Both are in kmon, which this slice deliberately does not touch — consumer
adoption is out of scope by Ken's decision and becomes one work item per repo
when this merges. They are recorded so the adoption item starts from evidence
rather than rediscovering it:

- **kmon's `korg_api.py` docstring is stale about the lean work-item row.** It
  enumerates eight fields and says the row carries "NOT `tags`, `updated` or
  `sprint`". `updated` **is** on the lean row (added WI #1411), and the
  docstring also omits `has_details`, `has_handoff` and `proposal_node_id`. No
  bug — nothing reads those — but it is the exact drift class the contract
  exists to end, sitting in the file whose whole purpose is knowing korg's
  shapes.
- **kmon groups `list_projects` with the paginated reads.** korg publishes it as
  `filtered` — `{items, omitted}`, no `total`/`limit`/`offset`. kmon's `unwrap`
  copes because both are envelopes, so this is not a live fault; a consumer that
  tried to *page* it would find nothing to advance.

## Repaired in passing

Nothing. `just check` was green on arrival and no gate reported a pre-existing
defect.

## Plan amendment owed at ship (korg+ GP-14)

Drafted here, to land in the ship's documentation-freshness phase where the
amend rule puts it, rather than now — the overseer reviews this work before the
ship and the plan should not assert something review might change.

GP-14 tells a consumer to "ask korg's schema for the domain, or state in the
type that it is a sample". For **response shapes** there was nothing to ask, so
every consumer sampled, and the register GP-14 is written about had no mechanism
behind its first horn. There is one now, and the amendment should say three
things:

1. The mechanism: `contract/read-shapes.json`, served at
   `GET /api/contract/read-shapes`. A consumer's claim about korg's read shapes
   should cite it rather than a window of observed output.
2. The fifth register is **shape** — envelope form, key presence, projection —
   alongside numeric, state, type and threshold. It is the register with the
   most measured damage behind it: 11 days and two months.
3. "Pin the specimen" generalises from a value domain to a **corpus**: a
   generated contract read off a convenient seed is a sample wearing a
   contract's clothes, and the guard has to know what the corpus is for. The
   third perturbation above is the evidence, and it is worth carrying into the
   plan because it is the case where consistency and honesty came apart.


## Deployed

**2026-09-18, to kubsdb** (`:5674`), by the `deploy-kubsdb` skill declared in
`.sprint-deploy`. Image `1404555d02b5`
(`sha256:b5dbf797daf3d0fb47564a794b6a284949ac311fa1a84c8a6aee48af0d915e80`),
built from merged `main` — which is commit `1404555`, sprint **084**, not 083's
own `fc04947`. That is the whole story of this deploy and it is in the next
section.

Rollback target: `00f2c2cd92e3` (sprint 082), confirmed present in the registry
before building.

### The first attempt failed, and 083 caused it

The image build failed at compile time: `couldn't read …
contract/read-shapes.json`. The Dockerfile's rust stage copies `Cargo.*`,
`rust-toolchain.toml` and `crates/`, and `contract/` is a new top-level
directory — so the file this sprint asks `korg-api` to embed was not there.

Nothing in the gate could have caught it. `just check` and CI compile in the
full tree; only the image's rust stage sees a restricted copy set. Fixed in
sprint 084 along with a gate for the whole class, merged as PR #89, and this
deploy carries both sprints.

### Verified live

| Check | Result |
|---|---|
| Revision assertion (in-deploy) | running `1404555d02b5…` = the commit built. Catches a `pull` that silently did nothing |
| `post-deploy-check.sh --compare` | **OK**, exit 0. Every row count unchanged (work_items 1638, proposals 493, cards 30, links 21, projects 59, reports 81); migrations 35 → 35, as expected with no migration this sprint |
| `GET /api/contract/read-shapes` | **200, `application/json`, 12249 bytes, byte-for-byte identical to the committed file** (same sha256). All 15 reads present |
| The two damaging facts, over the wire | `list_work_items` is `paginated`; `tags` is in `only_on_full_read`; `list_projects` reports `status` optional |
| Consumer-position check | the read-only consumer script re-run against the **live endpoint** rather than the local file: every assertion held |

**Before/after is not 404 → 200, and that is worth knowing.** Against the
previous release the path returned **`200 text/html`** — the SvelteKit fallback
served the web app. So a consumer pointed at a korg too old to have this
endpoint does not get a clean 404; it gets HTML with a success status, and
`resp.json()` raises a parse error rather than anything diagnostic. **A
consumer's staleness check must look at the content type or the body, not the
status code.** Recorded here and carried into the consumer adoption items,
because it is exactly the kind of trap this artifact exists to remove and the
fetch code is written on the consumer's side.

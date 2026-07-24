# Sprint 025 — Handoff H-1: node, core model, API

Proposal `korg:614`, covering WIs #606–609. First of two bundles implementing
the [handoff node plan](../planning/2026-07-21-handoff-node-plan.md); H-2
(`korg:615`, WIs #610–613) builds the surfaces (web viewer, skill, docs) on top.
Tag `handoff-2026-07`.

Backend-only and independently deployable: a first-class `handoff` node with
durable, cross-machine storage and generalized `has_handoff` links to the work
it describes — reachable and legible from the read path on day one. **No UI, no
skill** (that's H-2).

## What the linking-2026-07 cleanup already did for us

This sprint reconciles the plan against the linking arc deployed 2026-07-24 (see
the plan's [Reconciliation section](../planning/2026-07-21-handoff-node-plan.md)).
Three things are already built — we consume, not rebuild:

- **LB-3 gave us the read path, generically.** `get_work_item` and
  `get_proposal` already inline a capped, truncation-flagged `related` block of
  `RelatedRef`s ([repo.rs:967](../../crates/korg-core/src/repo.rs)). A
  `has_handoff` edge surfaces there automatically. **We add no
  handoff-specific read fields** — the plan's `handoffs` / `handoffs_truncated`
  are dropped; the body is fetched with `get_handoff`. Cap is
  `RELATED_CONTEXT_CAP = 25`.
- **`get_proposal` exists** (Sprint 015). Inherited.
- **LB-2 closed and enforces the label vocabulary.** Adding `has_handoff` is one
  `LabelSpec` + `just gen`; enforcement, provenance (`origin`), and TS bindings
  come free. `has_handoff` is the registry's first customer (review D-11/D-12).

## The shapes, pinned before code

### 1 — Migration `0017_handoff.sql` (#606)

Mirrors [`0010_report.sql`](../../crates/korg-core/migrations/0010_report.sql):
widen the kind check, add one detail table.

```sql
ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check
    CHECK (kind IN ('workitem','card','link','slot','sprint_proposal','report','handoff'));

CREATE TABLE handoff (
    node_id BIGINT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    title   TEXT   NOT NULL,
    summary TEXT   NOT NULL,
    body    TEXT   NOT NULL,   -- Markdown
    CONSTRAINT handoff_title_nonempty   CHECK (btrim(title)   <> ''),
    CONSTRAINT handoff_summary_nonempty CHECK (btrim(summary) <> '')
);
```

No index in v1 — discovery is relationship-driven (plan: a bounded list op only
on a demonstrated need). `body` may legitimately be empty-ish; `title`/`summary`
carry the non-empty backstops (vocab is validated app-side, these are the DB
backstop).

### 2 — Registry entry (#606)

One line in `REGISTRY`
([relationships.rs:34](../../crates/korg-core/src/relationships.rs#L34)):

```rust
LabelSpec {
    label: "has_handoff",
    directed: true,               // subject -> handoff; not symmetric
    left_kind: None,              // any node may own a handoff
    right_kind: Some("handoff"),
    reads: "node has handoff",
},
```

Subject on the **left**, handoff on the **right**. `just gen` propagates it into
`web/src/lib/generated/vocab.ts` `RELATIONSHIP_LABELS`. LB-2's `relate()`
enforcement then rejects a `has_handoff` edge whose right endpoint isn't a
handoff, with an `invalid_input` naming the registry.

### 3 — `create_handoff` (#607) — atomic, mirrors `upsert_report`'s tx

```text
create_handoff(title, summary, body,
               related_node_ids: [i64],   // reject empty unless allow_standalone
               allow_standalone: bool = false,
               project?, category?, tags?) -> HandoffRef { node_id }
```

One transaction, following
[`upsert_report`](../../crates/korg-core/src/repo.rs) (INSERT node → INSERT
detail → INSERT edges → commit):

1. Reject empty `related_node_ids` unless `allow_standalone` — an orphan handoff
   must be a deliberate opt-in, never a forgotten link step (plan Write
   contract).
2. **Reject** any `related_node_ids` that doesn't resolve — `not_found`, whole tx
   rolls back, no partial insert. (This is the one place we diverge from
   `upsert_report`, which *silently drops* unresolved findings; a handoff that
   loses an owner silently is exactly the failure the plan exists to prevent.)
3. `INSERT INTO node (kind, project_id, category, tags) VALUES ('handoff', …)`.
4. `INSERT INTO handoff (node_id, title, summary, body) …`.
5. Per owner, `INSERT INTO relationship (left_id, right_id, relationship,
   created, origin) VALUES (owner, handoff_node, 'has_handoff', now(),
   'create_handoff') ON CONFLICT … DO UPDATE SET left_id = relationship.left_id`
   — **owner on the left, handoff on the right**, provenance stamped, ON CONFLICT
   preserving original `created`/`origin` exactly as `upsert_report` does.

Orientation is correct by construction, so we insert directly (as
`upsert_report` writes `finding`) rather than routing through `relate()`.

### 4 — Reads: `get_handoff` + title resolution (#607)

`get_handoff(node_id) -> HandoffFull` mirrors
[`get_report`](../../crates/korg-core/src/repo.rs): node metadata + title +
summary + Markdown `body` + related nodes (reuse `related_context(pool, node_id,
None)` for the related block, so a handoff shows what it's attached to, both
ways).

**Critical:** `related_context`'s title `COALESCE`
([repo.rs:1022](../../crates/korg-core/src/repo.rs#L1022)) does **not** join
`handoff` yet — a handoff neighbor currently resolves to `"handoff #431"`. Add
`LEFT JOIN handoff hd ON hd.node_id = other.id` and slot `hd.title` into the
`COALESCE` (before the `kind || ' #' || id` fallback) so `has_handoff` edges
render with the real title in every focused read. This is the one edit that
makes the LB-3 block actually useful for handoffs.

### 5 — `update_handoff` + archive (#607)

`update_handoff(node_id, title?, summary?, body?)` — partial, only passed fields
change, touches `node.updated`. Archive via the existing node-archive path
(set `node.archived`), no new mechanism. Relationship changes go through the
generalized `relate`/`unrelate` — no focused edge op in v1 (plan Write
contract).

## Slices

### §1 — Migration + registry (#606) · S
`0017_handoff.sql` + the `has_handoff` `LabelSpec` + `just gen`. Acceptance:
migration applies clean against a restored nightly dump; `node_kind_check`
accepts `'handoff'`; `relate()` accepts `has_handoff` → handoff and rejects a
non-handoff right endpoint (`invalid_input` naming the registry); `just gen`
diff fence green. **Done first.**

### §2 — Core model (#607) · M
`create_handoff` (atomic, reject-empty, reject-missing), `get_handoff`,
`update_handoff`, archive; the `related_context` `handoff` join. Acceptance:
create + several `has_handoff` edges atomically; reject missing related → no
partial insert; update/archive; retrieve from **both** sides of the edge
regardless of stored orientation; a handoff neighbor renders with its title.

### §3 — MCP + REST (#608) · M
MCP `create_handoff` / `get_handoff` / `update_handoff`
([korg-mcp/tools.rs](../../crates/korg-mcp/src/tools.rs)); REST `POST
/api/handoffs`, `GET|PATCH /api/handoffs/:node_id`
([korg-api](../../crates/korg-api/src)). Same domain semantics both transports,
differences documented in [docs/api.md](../../docs/api.md). Tool descriptions
say handoffs are **required context**, not optional related reading.

### §4 — Contract + atomicity tests (#609) · S
Both transports: create/get/update round-trips; reject-missing leaves node +
detail + edges all absent; `has_handoff` surfaces in `get_work_item.related` and
`get_proposal.related` (proposals still exclude `covers`) carrying the handoff
title; response-size test — several large handoffs on one node stay within
tool-output limits past the cap, `related_truncated` exact. `docs_drift` +
`just gen` fences green.

## Test plan

- **Core**: atomic create + edges; reject-empty (`allow_standalone` opt-in
  works); reject-missing rolls back fully; update/archive; both-sides retrieval;
  title resolves for a handoff neighbor.
- **Contract** (both transports): `create`/`get`/`update` shape parity;
  `related` block carries `has_handoff`; errors/null/not-found match reviewed API
  conventions.
- **Fences**: `docs_drift`, `just gen` (`vocab.ts` gains `has_handoff`).

## Deploy

`deploy-kubsdb` after merge (has a migration — `0017`). Rehearse `0017` against a
restored nightly dump per docs/operations.md before the deploy window. Verify
live: create a handoff attached to a WI, confirm it appears in that WI's
`get_work_item.related` with its title and `related_truncated` false;
`post-deploy-check.sh --compare` clean. **Deploy H-1 before starting H-2** — the
viewer and skill consume this live surface (LB-1-before-LB-2 discipline).

## Deferred to implementation / H-2

- Inline-cap **ordering** when many handoffs attach (plan open Q) — `(label,
  node_id)` from LB-3 already groups them; revisit only if a real ordering need
  appears.
- Mutability vs. successor edges for revisions; standalone-handoff workflows;
  archival policy — all plan open questions, none blocking H-1.
- Collection-level `handoff_count` on survey/list — intentionally skipped
  (2026-07-24 decision); revisit on a demonstrated miss.

## Deployed 2026-07-24

- **Image**: `korg:24495eb83ef8` — revision
  `24495eb83ef84cce63745a408256656303e3a44a` (the squash-merge of PR #26).
- **Rollback target**: `korg:e26030b14d08` (LB-3, image
  `sha256:720e2c76c681`). Rollback is image-only; **0017 does not auto-revert**
  on a re-tag — crossing it backward needs a dump restore (docs/operations.md),
  though the migration is purely additive.
- **Migration rehearsed first** against a restore of the 2026-07-24 nightly dump
  in a local scratch container: version 17 applied, node/edge counts
  byte-identical, a functional `create_handoff` on real data surfaced in the
  owner's `related` block. The corrected kind list (0012's, +`handoff`) matched
  production's live `node_kind_check` exactly.
- **CI**: green on PR #26 (rust + web) before merge.
- **Verified live** against the deployed API:
  - `_sqlx_migrations` head = **17 (handoff)**; `handoff` table present;
    `node_kind_check` includes `handoff`.
  - `GET /api/handoffs/9999999` → `404 {"code":"not_found"}` — a clean 404 (not a
    500) proves the route queries the now-existing table.
  - MCP `tools/list` advertises `create_handoff` / `get_handoff` /
    `update_handoff`.
  - `post-deploy-check.sh --compare`: **OK** — every row count stable
    (additive migration touched no data).

Clears the ground for **H-2** (`korg:615`): web viewer, entry points, skill.

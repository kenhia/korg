# korg Deep Review — Spring Cleaning

Date: 2026-07-22 (review executed against `main` @ `2626128`)
Reviewer: Fable
Prompt: `sprints/review/2026-07-21-review-prompt.md`

Evidence notation: file:line citations refer to the current working tree.
Claims are tagged **[observed]** (verified against code or a live check),
**[inferred]** (evidence-backed inference), or **[preference]** (design
recommendation). Live checks ran against a scratch instance (see §10
Validation Log); nothing here touched `kubsdb`.

---

## 1. Executive Assessment

korg is in fundamentally good shape. The typed-node + generalized-edges model
has held up across seven node kinds without schema contortions; migrations are
disciplined (the 0009 identity rewrite is genuinely well-engineered); the
daily-planning module is a model of what the rest of the core should look like
(typed errors, server-enforced lifecycle, transactional reorder with row
locks); the full test suite and all 27 Playwright e2e specs pass; and the
dogfooded agent workflows (`start-sprint` / `sprint-ship` / `refill-queue`)
have **zero response-shape drift** against the live tool surface — every field
those skills read actually exists.

The accumulated cost of twelve sprints is concentrated in four systemic
problems, and almost every individual defect below is a symptom of one of them:

1. **No single source of truth for the domain contract.** The concept
   inventory (statuses, kinds, labels, field shapes) is hand-maintained in at
   least five places: Rust consts/DB constraints, hand-written MCP JSON
   schemas, REST request structs, hand-mirrored TypeScript types, and prose
   docs. They have already drifted (survey's advertised `archived` default is
   false, actual behavior is "both" — live-verified; the docs' REST table is
   missing 4 endpoints; three different tool-category lists exist).
2. **Three error regimes in one core crate.** `RepoError` (mapped to 4xx),
   `PlanningError` (mapped precisely), and bare `anyhow::bail!` (always 500).
   The WI #289 taxonomy was applied to two modules and stopped. Live-verified
   consequences: invalid dates, unknown reports, invalid t-shirt sizes and
   card statuses all return **500**.
3. **Mutations don't acknowledge.** Most updates return `()`/`{"ok":true}`
   without checking existence or kind. Live-verified: `PATCH
   /api/work-items/9999` returns `{"ok":true}`; `PATCH /api/cards/4` where
   node 4 is a *work item* silently archived the work item. For a system
   whose primary consumers are agents, "success" responses that report
   nothing and can lie are a first-class product defect.
4. **Relationship direction is half-adopted.** Sprint 008 made `relate()`
   directed, but `create_proposal` and `upsert_report` still write
   id-canonicalized edges, so `covers` and `finding` direction is
   unrecoverable from storage, and `neighbors.direction` is only meaningful
   for post-008 `relate`-created edges. The handoff plan explicitly depends on
   resolving this.

The corrective direction: make korg-core the single authority for vocabulary,
errors, and per-operation contracts; make every mutation validate, acknowledge,
and return the entity; settle relationship-label semantics with a registry and
a backfill; then generate the derived surfaces (MCP schemas, TS types, doc
inventories) from the core instead of hand-mirroring them. The UI and docs
cleanups fall out of those.

Nothing found is data-destroying in normal operation. The two sharpest edges
are the cross-kind `update_card`/`update_proposal` hazard (F-04) and the
`--reset` blast radius understatement (F-21). There is no CI. The production
database is backed up nightly (k-homelab's `korg-backup` recipe, restore-
verified) — but that fact is recorded only in another repo and a closed WI's
comment, invisible from korg itself, which is the corrected F-24 finding.

---

## 2. Target Architecture

### 2.1 Current de facto architecture [observed]

```
crates/korg-core     schema (12 sqlx migrations), repo.rs (WIs, cards, links,
                     relationships, previews, proposals, reports, projects,
                     areas, comments), topics.rs, daily_plan.rs, config.rs
crates/korg-api      axum REST handlers; per-endpoint request structs; ApiError
                     (anyhow downcast → status); serves SPA + mounts /mcp
crates/korg-mcp      tools() — 42 hand-written JSON schemas; per-tool arg
                     structs; a 500-line match dispatch; all calling korg-core
crates/korg-migrate  one-shot kwi+kcard import (done; historical)
web/src/lib/api.ts   hand-mirrored TS types + fetch wrappers (497 lines)
web/src/routes       Svelte 5 pages; domain rules re-derived per page
docs/, README.md     hand-maintained inventories of all of the above
~/.claude/skills     agent workflows consuming MCP + REST
```

Where each concept is defined vs re-derived:

| Concept | Authority | Re-derived in |
|---|---|---|
| WI statuses | `repo.rs:34` `WI_STATUSES` | MCP enums (tools.rs:54,90), TS `WI_STATUSES` (api.ts), usage.md prose, tool descriptions |
| Card statuses | DB enum `card_status` (0001) | MCP enums ×2, TS `CARD_STATUSES`, UI literals (`"Cut"` ×3 files) |
| Terminal-status rule | nowhere | 6 call sites in web (`"closed"` literals), `plan` page `TERMINAL` set |
| Node kinds | `node_kind_check` (migration 0012) | preview dispatch match (repo.rs:393), daily-plan plannable set, TS `DailyPlanSourceKind`, UI kind labels ×2 |
| Relationship labels | nowhere | `'covers'` (repo.rs:1129), `'finding'` (repo.rs:1575), `'depends_on'` (lib.rs:907, plan page), `"related-to"` (2 web files), free-text input (work-items page) |
| Request shapes | duplicated | korg-api structs + korg-mcp structs + TS param types |
| Error taxonomy | `RepoError` (partial) | `PlanningError`, bare anyhow, MCP `to_err`, TS thrown strings |

### 2.2 Proposed target [preference, opinionated]

**korg-core owns the domain, transports present it, derived surfaces are
generated.** Concretely:

1. **Vocabulary in core.** One `vocab` module: `WiStatus`, `WiTshirt`,
   `CardStatus`, `LinkDisposition`, `ProposalStatus`, `ReportStatus`,
   `ProjectStatus`, `NodeKind` as real enums (serde + sqlx), plus a
   **relationship-label registry**: each known label with its direction
   semantics (`covers`: proposal→WI, directed; `finding`: report→WI,
   directed; `depends_on`: dependent→dependency, directed; `related-to`:
   undirected). Free-form labels stay allowed but undeclared labels are
   documented as "direction is caller-defined".
2. **One error type.** Extend `RepoError` (or a renamed `CoreError`) with the
   variants `PlanningError` proved out: `InvalidInput`, `NotFound`,
   `Conflict` (frozen-past / reorder mismatch), everything else internal.
   `topics.rs` and `daily_plan.rs` converge on it; `daily_plan`'s variants map
   into it mechanically. `ApiError` keeps its downcast but now covers
   everything; MCP `to_err` gains a `code` field from the same variants.
3. **Mutations validate, acknowledge, and return the entity.** Every
   `create_*`/`update_*` returns the post-write row (`WorkItemRow`,
   `CardRow`, …). Every update checks existence (and node kind for node-level
   fields) and returns `NotFound` otherwise. Deletes report whether they
   deleted. This is a small mechanical change to repo.rs (topics.rs already
   does the kind-guard half).
4. **Shared operation structs.** The patch/args structs (`WorkItemPatch`,
   `CardPatch`, …) already live in core; move the serde-facing request shapes
   there too, so korg-api and korg-mcp deserialize the *same* struct (the
   `double_option` idiom already exists in both — keep one copy). REST and
   MCP remain thin presenters: same operation names where surfaces overlap,
   documented intentional asymmetries (e.g. `create_report` is MCP-only;
   `get_node` preview is currently REST-only and should gain an MCP tool or a
   documented "UI-only" note).
5. **Generated MCP schemas.** Derive tool input schemas from the shared
   structs with `schemars` instead of 250 lines of `json!` literals. If the
   rmcp `Tool` type resists, the fallback is a parity test: deserialize each
   schema's `properties` and assert field-name/enum agreement with the struct.
   Either way, drift becomes impossible or loud.
6. **Generated TypeScript.** Derive TS types from the core structs with
   `ts-rs` into `web/src/lib/generated/`. `api.ts` shrinks to fetch wrappers;
   the const-tuples (`WI_STATUSES` etc.) come from the same generation. The
   alternative (OpenAPI + openapi-typescript) buys standard tooling but costs
   an axum annotation layer (utoipa) — not worth it for a single-consumer
   API. **[preference]**
7. **Generated doc inventories.** REST route table, MCP tool table, env-var
   table emitted by a small `just docs` generator (or asserted by drift
   tests). Prose stays handwritten (see §5).
8. **What the UI may assume.** The UI consumes generated types and a single
   `web/src/lib/domain.ts` for presentation rules (terminal statuses, kind
   labels, chip colors). It may assume: node ids are globally unique; WI
   `wi_number == node_id`; server-shaped detail reads include their context
   (comments/covered refs) up to documented caps. It may not: hardcode
   vocabulary, join collections client-side when a detail read exists, or
   invent relationship labels outside the registry defaults.

**What stays as-is [preference]:** the repo-function style (no service traits
— at this scale a service layer is ceremony); REST and MCP as separate thin
layers over core (a shared "handler layer" would couple transport error
models for little gain); sqlx string queries (compile-checked macros would
require DATABASE_URL at build; not worth it); Svelte 5 + hand-rolled fetch
(no data-fetching framework needed).

### 2.3 Load-bearing gaps (ranked by how many findings they cause)

1. **Mutation contract (returns + existence/kind checks)** → F-03, F-04,
   F-06 and half the agent-experience complaints.
2. **Error unification** → F-02, most of the wrong-status matrix in §4.
3. **Vocabulary/label registry** → F-01, F-05, F-11, F-15.
4. **Hand-mirrored derived surfaces** → F-11, F-12, F-22, F-23.
5. **Two-level read contract generalization** → F-09, F-10, planning-page
   N+1, and it is the stated precondition for the handoff node plan.

### 2.4 Decisions

All decisions were resolved with Ken on 2026-07-22 — see §9. The
architecture-shaping outcomes: undirected labels stay as stored with
orientation-blind readers (D-1); `wi_type` gains a vocabulary including
`brainstorm` (D-2); envelopes + `archived=false` land as one lock-step
breaking change (D-3); MCP schemas and TS types are generated via
schemars/ts-rs (D-4); REST errors gain a `code` field (D-5).

---

## 3. Prioritized Findings

Severity: `high` = causes agent/user-visible wrong behavior or integrity risk
today; `medium` = drift/coupling that keeps generating defects; `low` =
polish/latent. No `critical` findings (nothing loses data in normal
operation on the trusted network).

---

**F-01 · high · confidence: high — Relationship direction is unrecorded for
`covers`/`finding`, while the API claims edges are directed.**

- Evidence [observed]: `relate()` writes caller order and the tool description
  says "DIRECTED … reverse orientation is a distinct edge"
  (crates/korg-core/src/repo.rs:251-270, crates/korg-mcp/src/tools.rs:160).
  But `create_proposal` and `upsert_report` canonicalize `(lo,hi) = (min,max)`
  before insert (repo.rs:1121-1136, repo.rs:1567-1582), so stored orientation
  of `covers`/`finding` is a function of node-id ordering, not semantics.
  Migration 0006 canonicalized all pre-008 edges the same way. Sprint 008's
  own README records hand-flipping 10 `depends_on` edges post-deploy
  (sprints/008-directed-relationships-plan-view/README.md:14-16).
- Impact: `neighbors.direction` is semantically meaningless for every
  `covers`/`finding` edge and every pre-008 edge; an agent following the
  `relate` description will draw wrong conclusions. All current readers
  compensate by reading both ends (repo.rs:1656-1662), which works but means
  the "directed" contract is a fiction for these labels. The handoff plan's
  `has_handoff` label needs exactly this settled
  (sprints/planning/2026-07-21-handoff-node-plan.md:89-93).
- Recommendation: adopt the label registry (§2.2.1). Fix `create_proposal` /
  `upsert_report` to write semantic orientation (proposal→WI, report→WI). Add
  a backfill migration re-orienting existing `covers`/`finding` edges using
  endpoint kinds (unambiguous: exactly one endpoint is a proposal/report).
  Decide policy for undirected labels (D-1): either keep both orientations
  distinct (current constraint) and have readers treat them as one, or
  canonicalize undirected labels on write. Document per-label semantics in
  the tool description and docs/api.md.
- Uncertainty: none on the defect; the undirected-label dedup policy is a
  genuine choice.
- Validation: code read; migration 0006 + sprint 008 README; live `neighbors`
  output during seeding showed `covers` edges with id-order direction.

---

**F-02 · high · confidence: high — Error taxonomy is applied to two modules
and abandoned elsewhere; user errors surface as 500s.**

- Evidence [observed, live-verified]: `ApiError::status` maps only
  `PlanningError` and `RepoError` (crates/korg-api/src/error.rs:22-45).
  Everything else is a 500, including:
  - invalid date strings — `parse_date` wraps plain anyhow
    (crates/korg-api/src/lib.rs:167-170) → live: `GET
    /api/daily-plan?from=notadate…` → **500**;
  - missing report — `get_report` handler raises plain anyhow (lib.rs:928) →
    live: **500** `{"error":"no report with node_id 9999"}`;
  - invalid `wi_tshirt` (DB CHECK) and invalid card status (enum cast) →
    live: **500** with raw DB error text;
  - `topics.rs` uses `bail!` for not-found and empty-name
    (crates/korg-core/src/topics.rs:44,126,152) → 500;
  - `update_comment` missing id uses `anyhow::anyhow!` (repo.rs:1033) → 500;
  - history preset/`from`/`to` validation errors (lib.rs:849-861) → 500;
  - `relate`/`add_comment` to nonexistent nodes → FK violation → live: MCP
    `isError` with raw `violates foreign key constraint` text.
- Impact: agents cannot distinguish their own bad input from server faults;
  raw DB errors leak as the primary "API documentation" for these paths.
- Recommendation: one error enum in core (§2.2.2); pre-validate vocabulary
  app-side (card status, tshirt, disposition) so DB constraints are a
  backstop, not the UX; existence-check `relate`/`add_comment` targets →
  `NotFound`; convert `topics.rs` and the api-layer parse/validation errors
  to typed variants. Add REST tests asserting the status matrix in §4.
- Validation: live probe matrix in §10.

---

**F-03 · high · confidence: high — Not-found and no-op semantics are
inconsistent; updates against missing rows report success.**

- Evidence [observed, live-verified]:
  - `update_work_item` returns `Ok(())` when the wi_number doesn't exist
    (repo.rs:1308-1311) → live REST `PATCH /api/work-items/9999` → `200
    {"ok":true}`; live MCP `update_work_item(9999)` → `{"ok":true}`.
  - `update_card`, `update_proposal` never check existence (repo.rs:923-983,
    1192-1247); `set_link_disposition`, `mark_link_read`, `set_node_tags`
    ignore `rows_affected` (repo.rs:210-236); `delete_comment`/`unrelate`
    are silent no-ops on missing ids (repo.rs:1036-1042, 311-317).
  - Contrast: `update_project` 404s (repo.rs:794-796), `update_topic`/
    `archive_topic` error (topics.rs:125-127,151-153), daily-plan ops 404
    precisely (daily_plan.rs).
  - Reads: `GET /api/work-items/9999` → `200 null` [live]; `get_topic`,
    `get_node` → `200 null`; `get_report` → 500 (F-02); MCP `get_work_item`
    missing → success `null` while MCP `get_report` missing → `isError`.
- Impact: an agent that typos a wi_number gets confirmation instead of an
  error; sprint-ship's close-out loop could "resolve" nothing and report
  success. This is the highest-frequency agent hazard in the review.
- Recommendation: PATCH/updates on missing → 404/`NotFound` everywhere;
  single-item GETs → 404 (REST) and MCP `isError` not-found (D-6 records the
  null-vs-404 choice); deletes return `{"deleted": bool}`. All mutations
  return the entity (§2.2.3).
- Validation: live probes; code read of every repo mutation.

---

**F-04 · high · confidence: high — `update_card`/`update_proposal` apply
node-level fields to nodes of any kind.**

- Evidence [observed, live-verified]: the node-table branches bind only `id`
  (repo.rs:953-980 for card; 1231-1244 for proposal) — no `kind` guard,
  unlike `update_topic`/`archive_topic` (topics.rs:132,146). Live: `PATCH
  /api/cards/4 {"archived":true,"title":"hijack"}` where node 4 is a work
  item → `200 {"ok":true}`; node 4 became `archived=t` (title update
  no-oped against the card table). Since 0009 made `wi_number == node_id`,
  confusing a wi_number for a card node_id is exactly the sort of slip an
  agent will eventually make.
- Impact: silent cross-kind mutation of archived/tags/category/project on
  arbitrary nodes, reported as success.
- Recommendation: existence + kind check at the top of both functions →
  `NotFound("no card with node_id N")`; regression tests. (Falls out of the
  F-03 mutation-contract work.)
- Validation: live probe, reverted afterwards (§10).

---

**F-05 · medium · confidence: high — Validation gaps at create time and for
non-status vocabulary.**

- Evidence [observed]: `wi_type` is entirely free-text (no constraint
  anywhere; `WI_TYPES` exists only in web/src/lib/api.ts as select options);
  `wi_tshirt` is enforced only by DB CHECK (→ 500); card status only by
  enum cast (→ 500); `create_work_item` does not validate that `area_id`
  belongs to `project_id` — live-verified: created WI #20 with korg project +
  homelab-ai area — while `update_work_item` enforces exactly that
  (repo.rs:1388-1402). `create_card`/`create_link` accept any
  project_id (FK 500 on nonsense).
- Impact: typo'd `wi_type` fragments the corpus (survey filters, refill-queue
  bundling); cross-project areas undermine the WI #291 invariant the update
  path defends.
- Recommendation: validate tshirt/card-status/disposition app-side from the
  vocab module; enforce area∈project at create; D-2 decides whether wi_type
  gets a vocabulary or stays free-form (if free-form, document it and expose
  distinct values via survey for cleanup).
- Validation: live probes (§10).

---

**F-06 · medium · confidence: high — Silent reference-resolution drops.**

- Evidence [observed, live-verified]: `update_work_item` parent: `Some(num)`
  that fails to resolve becomes `Some(None)` — i.e. *clears the parent*
  silently (repo.rs:1313-1317). Live: `PATCH /work-items/2 {"parent": 1}`
  (no WI 1 exists) → `{"ok":true}`, parent cleared. `create_proposal` and
  `upsert_report` drop unresolvable wi_numbers from `covers`/`findings`
  (repo.rs:1090-1095, 1507-1512) — these at least echo the resolved set in
  the response (`covered`, `findings_linked`), which the skills check.
- Impact: parent-clearing is invisible data corruption from the caller's view.
- Recommendation: unresolved `parent` → `InvalidInput`. Keep drop-and-report
  for covers/findings but say so in the tool descriptions ("numbers that do
  not resolve are omitted from `covered` — compare against your request").
- Validation: live probe during seeding (§10).

---

**F-07 · medium · confidence: high — Report re-runs accumulate stale
`finding` edges.**

- Evidence [observed]: `upsert_report` adds finding edges idempotently but
  never removes edges absent from the new run (repo.rs:1567-1582); the
  migration comment says a same-day re-run "REPLACES content"
  (migrations/0010_report.sql:7-9) — content yes, findings no.
- Impact: kmon re-runs with a corrected finding list leave the old findings
  attached; `get_report.findings` over-reports.
- Recommendation: within the upsert transaction, delete this report's
  `finding` edges not in the resolved set (or all, then re-add). Note the
  behavior change in the tool description.
- Uncertainty: whether accumulate-across-re-runs was intended. The word
  "REPLACES" suggests not. Flagged in §9 (D-7).

---

**F-08 · low · confidence: high — Fresh installs can never have node/WI #1.**

- Evidence [observed, live-verified]: migration 0009's final
  `setval(…, GREATEST((SELECT MAX(id) FROM node), 1))`
  (migrations/0009_identity.sql:79-80) on an *empty* database sets the
  sequence to 1 with `is_called = true`, so the first node gets id 2. Live:
  scratch DB's first work item was `{"node_id":2,"wi_number":2}`; `SELECT
  min(id) FROM node` → 2. (`import.rs:163-169` has the same idiom but always
  runs with data present.)
- Impact: cosmetic on production (data exists); confusing on fresh installs
  and in tests; my own seed script tripped over it.
- Recommendation: new migration: if `node` is empty, `setval(seq, 1, false)`.
  Do not edit 0009 (sqlx checksums).

---

**F-09 · high · confidence: high — Collection reads are unbounded and the
Sprint-012 two-level contract stops at work items; REST and MCP detail reads
disagree.**

- Evidence [observed]:
  - `list_work_items` returns every item with full `content` + `details`
    (repo.rs:563-580); the survey tool exists *because* this exceeds output
    limits at instance scale (tools.rs:68) — yet `list_work_items` remains
    unbounded, and its MCP description doesn't mention that archived items
    are included (no filter exists).
  - `list_cards`, `list_links`, `list_topics`, `list_comments`, `neighbors`:
    no pagination, no limits, no filters (beyond topics `q`).
  - Two-level contract: `comment_count` exists on `WorkItemRow`/`WorkItemSummary`
    only. Cards, proposals, reports, topics, links are all commentable
    (comments are node-scoped, repo.rs:998) but no collection view signals
    it, and no focused read inlines them — except MCP `get_work_item`.
  - REST `GET /api/work-items/:wi` returns `get_work_item` (no comments,
    lib.rs:237-239) while MCP `get_work_item` returns `get_work_item_detail`
    (inlined capped comments, tools.rs:791-797). Same operation name, two
    shapes; the web UI compensates with a second fetch (`Comments`
    component), which is fine for a browser but means the REST surface never
    got the Sprint-012 fix.
  - REST `PATCH /api/work-items` cannot set `category` (hardcoded `None`,
    lib.rs:362); MCP can (tools.rs:98). Unintentional parity gap [inferred].
- Impact: MCP `list_work_items` with no project is already a context bomb at
  instance scale; agents fetching a card/proposal/report detail can silently
  miss discussion (the exact failure Sprint 012 fixed for WIs, and the
  handoff plan's stated fear).
- Recommendation (exact contracts in §4.3): adopt the
  `{items, total, limit, offset}` envelope for the four unbounded lists;
  generalize `comment_count` to `CardRow`/`ProposalRow`/`ReportRow`/`Topic`;
  make REST WI GET return the detail shape (or add `/api/work-items/:wi/detail`
  — recommend the former, the UI already fetches comments separately and can
  stop); add REST `category` patch support.
- Validation: code read; payload behavior observed during browser checks.

---

**F-10 · medium · confidence: high — No proposal detail read; the Planning
page does an N+1 and a client-side join to compensate.**

- Evidence [observed]: no `get_proposal` anywhere (repo, REST, MCP). The
  Planning page loads `proposals()` + **all** `workItems()`, then calls
  `neighbors()` once per proposal and joins covered node_ids against the
  work-item list client-side (web/src/routes/planning/+page.svelte:44-61).
  `start-sprint` does the same dance via MCP (SKILL steps 2.1-2.3).
- Impact: N+1 per page load; the primary agent entry point (`start-sprint`)
  needs three tools and a join to answer "what does this proposal cover";
  the handoff plan requires `get_proposal` as the authoritative read.
- Recommendation: `get_proposal(node_id)` returning proposal fields +
  covered WI references `{wi_number, title, wi_status, wi_tshirt}` +
  `comment_count` (§4.3); `list_proposals` gains `covered_count`. Update
  Planning page and `start-sprint` to consume it.

---

**F-11 · medium · confidence: high — MCP schema/description drift against
actual behavior.**

- Evidence [observed, live-verified]:
  - `survey_work_items` schema advertises `"archived": {"default": false}`
    (tools.rs:73) but the server default is `None` = both — live: archived
    WI #7 appears in a no-args survey.
  - `relate` description overstates direction (F-01).
  - `list_work_items` description omits that archived items are included.
  - `propose_sprint`/`create_card` `rank` default 0 → all default-ranked
    proposals tie at 0 and sort by insertion accident (no tie-breaker,
    repo.rs:1170; F-19).
  - Transport asymmetry: MCP `update_card` moves projects by `project_id`;
    REST `update_card` takes a project *name* and auto-creates it
    (lib.rs:462-469). Neither documents the other.
  - README's "42 tools" is currently true but hand-counted (three prose
    copies of the category list, each different — see F-12).
- Recommendation: fix the `archived` default (schema or behavior — recommend
  schema says "omit for both", default stays None); rewrite `relate`
  description per label registry; document archived inclusion; add ordering
  tie-breakers; document the project_id/name asymmetry or align on id.
- Validation: live survey probe; code read.

---

**F-12 · medium · confidence: high — Documentation drift (11 concrete
instances).**

- Evidence [observed] (full audit in §10 notes; line cites verified):
  1. README status block still says kwi/kcard are the live tools and korg is
     "Milestone 1" (README.md:6-9) — contradicted by production deploy +
     five consuming skills. [inferred: stale]
  2. README "Crates" list omits `korg-api` (README.md:33-38).
  3. Tool-category prose exists in three inconsistent copies: README.md:51
     (omits proposals + reports), docs/usage.md:88 (omits reports), MCP
     server instructions (tools.rs:1251 — omits proposals + reports).
  4. usage.md REST table is missing 4 implemented endpoints:
     `GET /api/projects/:name/plan`, `PATCH /api/projects/:name`,
     `GET /api/reports`, `GET /api/reports/:node_id` (lib.rs:85-89).
  5. The reports feature (3 tools, 2 endpoints) has no prose documentation.
  6. `--reset` documented as clearing "work items/cards/projects/areas"
     in README.md:107, docs/migration.md:57, justfile:23 and the code's own
     comment — actual SQL is `TRUNCATE node, project, area … CASCADE`
     (korg-migrate/src/main.rs:100), which also wipes links, topics, daily
     plans, proposals, reports (F-21).
  7. `KORG_CORS_ORIGINS` (lib.rs:143) missing from setup.md env table.
  8. deploy-kubsdb SKILL's env row omits required `KORG_TIMEZONE` — a
     *first* deploy on a new host would crash at startup (config.rs:19).
  9. README curl example uses MCP protocol 2025-06-18;
     scripts/mcp-roundtrip-check.sh uses 2025-03-26.
  10. sprint-ship expects a `just check` recipe; korg's justfile has none.
  11. `update_project` also accepts description/gh_repo/cn_path (cn_path is
      load-bearing for start-sprint) — usage.md lists only 4 fields.
- Recommendation: one docs sweep sprint plus drift *checks* so it stays
  fixed (§5).

---

**F-13 · medium · confidence: high — No CI; `cargo test --workspace` fails on
a clean checkout; no unified check entry point.**

- Evidence [observed]: no `.github/`, no hooks. korg-migrate's three test
  suites hard-require `snapshots/*.dump` which are gitignored and
  machine-local (crates/korg-migrate/tests/common/mod.rs:65) — the suite
  passed here only because this machine has June-24 dumps. No `just check`.
- Impact: every quality gate is voluntary; a fresh clone can't even run the
  advertised test command cleanly.
- Recommendation: GitHub Actions running `cargo test --workspace
  --exclude korg-migrate` (or gate snapshot tests behind an env var so the
  workspace run skips them gracefully), `pnpm check`, `pnpm lint`, `cargo
  clippy`; add `just check` running the same set locally (sprint-ship and
  kproject-init already assume it). Playwright smoke in CI is optional
  (needs Docker + build; start with the Rust/svelte gates). [preference on
  scope]

---

**F-14 · medium · confidence: high — Test gaps concentrated exactly where the
findings are; 10 copies of the container-bootstrap helper.**

- Evidence [observed]: untested surface includes: 14 of 42 MCP dispatch arms
  (all report tools, topic reads/updates, `mark_link_read`, `update_project`,
  `update_comment`, `delete/reorder/history` daily-plan); REST
  `GET /api/reports*`, `GET /api/projects/:name/plan` (sole caller of
  `project_edges`, which has zero coverage anywhere), `PUT …/order`,
  `DELETE /api/daily-plan/:id`, `PATCH /api/comments/:id`, links `read`;
  `get_node_preview` for 5 of 7 kinds; every DB-CHECK error path.
  `fresh_korg()` is copy-pasted across 8 korg-core test files + variants in
  korg-mcp/korg-api; `NewWorkItem` builders redefined per file.
- Recommendation: a `korg-test-support` dev crate (container bootstrap +
  builders); add the missing dispatch/REST tests as part of whichever bundle
  touches each surface (acceptance criteria in §8) rather than a separate
  test sprint; status-matrix contract tests from §4 are the highest value.

---

**F-15 · medium · confidence: high — UI re-derives domain rules in scattered
literals.**

- Evidence [observed]: terminal-status rule via `"closed"` literals in 6
  places (+ a different `TERMINAL` set on /plan including done/resolved);
  `"Cut"` in 3; `kindLabel()` copy-pasted twice; `midRank` fractional-rank
  duplicated in cards + planning; proposal status literals hardcoded despite
  an exported `PROPOSAL_STATUSES` nobody imports; relationship label default
  `"related-to"` in 2 files plus a free-text label input; chip color
  conventions differ per page (project/tag/category chips have 2-3 variants
  each). Full map in the web inventory (§10 notes).
- Impact: every vocabulary change is an N-file hunt; the /plan page's
  different terminal set is a live inconsistency (an item `resolved` is
  "done" on /plan but active elsewhere).
- Recommendation: `web/src/lib/domain.ts` (fed by generated types in B4):
  `isTerminal(status)`, `kindLabel`, `midRank`, chip-class maps; replace the
  free-text relationship label input with registry defaults + "custom…"
  escape hatch.

---

**F-16 · medium · confidence: high — UI error handling: many mutations fail
silently; some pages swallow load errors entirely.**

- Evidence [observed]: no try/catch on mutations in reading-list, Comments,
  cards add/save/archive, most work-items mutations (unhandled rejections,
  no user feedback); `.catch(() => [])` turns load failures into empty
  pages on link-up (all four lists) and swallows areas/neighbors/covers
  errors elsewhere; error containers differ per page (5 variants,
  `role="alert"` on 4 of 10 pages); loading/empty states inconsistent
  (link-up has neither).
- Recommendation: one error/toast primitive + a fetch wrapper that routes
  failures into it; make `.catch(()=>[])` distinguishable ("failed to load"
  state vs genuinely empty).

---

**F-17 · medium · confidence: high — Accessibility and destructive-action
gaps.**

- Evidence [observed, markup + browser-verified]: NodePreview has no focus
  trap, no focus restore, no Escape handler (components/NodePreview.svelte);
  cards modal has Escape but no trap/restore; clickable table rows are
  `<tr tabindex=0>` without `role` (cards, work-items); board tile responds
  to Enter but not Space; drag-only reorder/move flows (planner, cards)
  have no keyboard alternative; unlabeled placeholder-only inputs across
  reading-list/cards/WorkItemForm/work-items toolbar (contrast: TopicPicker
  and topics page are exemplary); comment delete, relationship remove, topic
  archive, WI/card archive all fire immediately with no confirm/undo;
  reading-list title is an `<a>` whose Enter/click *edits* instead of
  navigating. Mobile (390px, live): top nav overflows with `overflow-x:auto`
  and no affordance (last item at x=797 of 390 viewport); cards board's Cut
  strip clips off-screen; work-items table clips mid-column inside its
  scroll container.
- Recommendation: shared modal/slide-over primitive with focus management
  (NodePreview + cards modal + WI detail); row-as-link pattern (real `<a>`
  or `role="button"` + Space); confirm-or-undo on the four destructive
  actions; label the inputs (sr-only is fine); mobile nav: wrap or add
  scroll affordance. Keyboard alternative for reorder can be "move up/down"
  buttons at small cost. [preference on mechanism]
- Validation: markup audit + live browser checks at 1440×900 and 390×844
  (§10).

---

**F-18 · low · confidence: high — `project.updated` never advances.**

- Evidence [observed]: touch trigger exists only for `node` and `comment`
  (migrations/0001_init.sql:145-160); `update_project` never sets `updated`
  (repo.rs:785-848). Latent — `ProjectRow` doesn't expose timestamps — but
  it's a booby trap for anything that starts sorting projects by recency.
- Recommendation: add the trigger in the next schema-touching migration.

---

**F-19 · low · confidence: high — Missing ordering tie-breakers.**

- Evidence [observed]: `list_proposals` orders `pinned DESC, rank ASC`
  (repo.rs:1170) — default rank 0 means agent-created proposals tie and the
  order is unstable across queries; `list_cards` `status, rank` likewise
  (repo.rs:749); `neighbors` orders by neighbor `n.id` only — two edges to
  the same node have unstable relative order (repo.rs:283).
- Recommendation: append `node_id`/`rel_id` tie-breakers; document ordering
  per operation in §4 contracts.

---

**F-20 · low · confidence: high — Non-atomic compound operations.**

- Evidence [observed]: REST `update_link` runs up to three independent repo
  calls (disposition, read, tags — lib.rs:611-626): a mid-sequence failure
  leaves a partial write and returns an error that doesn't say which parts
  landed. Link-up's clique-linking loops `relate()` per pair sequentially
  with one failure aborting the rest silently
  (web/src/routes/link-up/+page.svelte:64-68).
- Recommendation: fold link updates into one repo function/transaction
  (mirrors `update_card`); link-up: `Promise.allSettled` + report failures
  (or a bulk relate endpoint if this grows).

---

**F-21 · medium · confidence: high — `--reset` wipes far more than
documented.**

- Evidence [observed]: `TRUNCATE node, project, area RESTART IDENTITY
  CASCADE` (korg-migrate/src/main.rs:100) destroys **all** node kinds —
  topics, daily plans, proposals, reports, links — while the flag's help
  text, README, migration.md and justfile all say "work items / cards /
  projects / areas".
- Impact: an operator re-running the import "to refresh legacy data" on the
  production DB would silently destroy every post-import entity. The import
  is done and one-shot; the risk is precisely that it's only ever run again
  by mistake.
- Recommendation: fix the wording everywhere; add a confirmation gate
  (refuse unless `KORG_RESET_CONFIRM=yes`) or make korg-migrate refuse to
  run against a DB containing post-import kinds. [preference on mechanism]

---

**F-22 · high · confidence: high — Hand-written parallel contract surfaces
(the structural cause).**

- Evidence [observed]: 42 hand-written `json!` schemas + 30 arg structs in
  korg-mcp duplicating 25 request structs in korg-api, duplicating patch
  structs in korg-core, mirrored by ~500 lines of hand-written TS types in
  web/src/lib/api.ts, mirrored again in docs tables. Drift already observed
  at every seam (F-11, F-12, TS `WorkItem.wi_status: string` vs enums,
  create/update shape asymmetries in the client).
- Impact: this is the generator of the drift class; each new entity (e.g.
  handoff) adds five more hand-synced copies.
- Recommendation: B4 — shared serde structs core-side, schemars-derived (or
  parity-tested) MCP schemas, ts-rs-generated TS. See D-4.

---

**F-23 · low · confidence: high — Dead/unused client surface.**

- Evidence [observed]: `api.reorderDailyPlan`, `api.createProposal`,
  `PROPOSAL_STATUSES`, `Neighbor.direction`, `prettyDuration()` are unused
  in the web app; `survey` has no client wrapper; unused response fields
  (`WorkItem.comment_count`, `Link.read`, `Link.category`) are carried in
  types. Not harmful; symptomatic of hand-mirroring. Fold cleanup into B4
  (generation makes "unused" visible and free).

---

**F-24 · low-medium (docs/ops) · confidence: high — The production database
IS backed up, but that fact is invisible from the korg repo.**

- **CORRECTED during Phase 2 reconciliation (2026-07-22).** As originally
  written, this finding claimed no backup existed — that was wrong.
  Evidence [observed]: korg WI #234 (closed 2026-07-08) records that
  k-homelab's `korg-backup` recipe delivers a nightly pg_dump of the korg DB
  to `/gratch/backups/korg/` (03:17 timer, `Persistent=true`, 14-day
  retention, empty-dump refusal), restore-verified into a scratch DB; the
  restore command lives in k-homelab `recipes/korg-backup/README.md`.
- The residual finding stands, and this review is its own proof: nothing in
  the korg repo (docs, README, deploy SKILL) mentions the backup, so a
  reviewer/agent working from the repo concludes none exists — exactly what
  this review's Phase 1 did. Recovery knowledge lives in another repo's
  README and a closed WI's comment.
- Recommendation: docs/operations.md documents the backup (pointer to the
  k-homelab recipe, dump location, retention, restore command) and the
  deploy skill preflight gains a "backups current?" one-liner. Optionally a
  periodic restore-drill note. Size XS-S.

---

**F-25 · low · confidence: medium — Minor performance notes (fine today,
worth shaping while touching contracts).**

- Evidence [observed]: `comment_count` as correlated subquery per row in
  both WI selects (repo.rs:567, 684) — fine at current scale, becomes a JOIN
  LATERAL if lists grow; UI re-fetches all four collections after every
  planner mutation (routes/+page.svelte `refresh()`), full project re-list
  after every WI edit; `relationship.relationship` (label) unindexed while
  `project_edges` and covers/finding lookups filter on it. No action needed
  now beyond the pagination work in B3; note the index if edge volume grows.

---

## 4. MCP/REST API-Response Matrix

### 4.1 Operation inventory and parity (as-built) [observed]

Legend: pagination `—` = unbounded; NF = not-found behavior; ✓/— under REST/
MCP = surface exists. MCP errors are `isError` tool results; statuses are
REST.

| Operation | REST | MCP | Filters / pagination | Ordering (tie-break?) | NF behavior | Known contract issues |
|---|---|---|---|---|---|---|
| create_work_item | ✓ POST /api/work-items | ✓ | — | — | n/a | returns ref only, not row; area/project unvalidated (F-05); tshirt 500 (F-02) |
| list_work_items | ✓ GET /api/work-items | ✓ | project only; **no pagination** | wi_number ✓ | empty list | full content+details per row; archived included, undocumented (F-09/F-11) |
| survey_work_items | ✓ GET /api/work-items/survey | ✓ | project, wi_status, archived; limit≤500+offset ✓ | wi_number ✓ | empty | advertised archived default false ≠ actual both (F-11); no client wrapper |
| get_work_item | ✓ GET /api/work-items/:wi | ✓ | — | n/a | REST 200 null; MCP null success | **REST row-only vs MCP detail+comments** (F-09) |
| update_work_item | ✓ PATCH | ✓ | — | n/a | **200 ok on missing** (F-03) | returns `{ok}`; REST can't patch category; parent silently clears (F-06) |
| create_card | ✓ POST /api/cards | ✓ | — | — | n/a | invalid status → 500; returns id only |
| list_cards | ✓ | ✓ | none; unbounded | status, rank (no tie) | empty | no comment_count (F-09); archived included |
| update_card | ✓ PATCH /api/cards/:id | ✓ | — | n/a | **ok on missing** | **kind-unguarded node fields** (F-04); REST moves project by *name* (auto-create), MCP by id (F-11) |
| get card (single) | — | — | | | | no single-card read on either surface |
| list_comments | ✓ GET /api/nodes/:id/comments | ✓ | none | created ✓ | empty (even for missing node) | no way to distinguish "no comments" from "no node" |
| add_comment | ✓ POST | ✓ | — | — | FK **500** on missing node | should be 404 (F-02) |
| update_comment | ✓ PATCH /api/comments/:id | ✓ | — | — | **500** "no comment" (F-02) | |
| delete_comment | ✓ DELETE | ✓ | — | — | silent ok | `{deleted:bool}` wanted (F-03) |
| create_link | ✓ POST /api/links | ✓ | — | — | n/a | |
| list_links | ✓ | ✓ | none; unbounded | node_id ✓ | empty | no disposition filter; whole reading-list history forever (F-09) |
| update link | ✓ PATCH /api/links/:id (disposition/read/tags) | mark_link_read only | — | — | silent ok on missing | **MCP cannot set disposition** though 0004 says dispositions are recorded over MCP (parity gap, likely accidental [inferred]); REST 3-call non-atomic (F-20) |
| relate | ✓ POST /api/relationships | ✓ | — | — | FK **500** | direction claim wrong for covers/finding (F-01); no self-edge guard |
| unrelate | ✓ DELETE /api/relationships/:id | ✓ | — | — | silent ok | |
| neighbors | ✓ GET /api/nodes/:id/neighbors | ✓ | none | n.id (unstable among multi-edges) | empty (missing node = empty) | direction unreliable (F-01); no kind/label filter; unbounded |
| get_node preview | ✓ GET /api/nodes/:id | — | — | n/a | 200 null | MCP has no preview tool (documented asymmetry needed) |
| project_plan | ✓ GET /api/projects/:name/plan | — | — | edges by rel id | empty items for unknown project | undocumented in usage.md; skill consumes REST directly |
| topics CRUD | ✓ (5 routes) | ✓ (6 tools) | q search; unbounded list | lower(name), node_id ✓ | get: 200 null; update/archive: **500** not-found (F-02) | |
| daily-plan ops | ✓ (6 routes) | ✓ (7 tools) | date-range | plan_date, position ✓ | 404/400/409 typed ✓ | **the model citizen** — copy its semantics |
| create_report | — | ✓ | — | — | n/a | MCP-only (intentional: kmon writes) — document; stale finding edges (F-07) |
| list_reports | ✓ GET /api/reports | ✓ | source, limit (default 30) ✓ | date DESC, source ✓ | empty | undocumented endpoint (F-12) |
| get_report | ✓ GET /api/reports/:id | ✓ | — | findings by wi_number ✓ | REST **500**; MCP isError | should be 404 (F-02) |
| propose_sprint / create_proposal | ✓ POST /api/proposals | ✓ | — | — | n/a | silent covers drop but echoed (ok); rank default 0 ties (F-19) |
| list_proposals | ✓ | ✓ | status; unbounded | pinned, rank (**no tie**) | empty | no covered info → N+1 (F-10); no comment_count |
| get_proposal | — | — | | | | **missing**; required by handoff plan (F-10) |
| update_proposal | ✓ PATCH | ✓ | — | — | **ok on missing** | kind-unguarded node fields (F-04) |
| projects list/create/update | ✓ | ✓ | none | name ✓ | update: 404 ✓ (the good one) | update_project extra fields undocumented (F-12) |
| recent_project | ✓ GET /api/projects/recent | — | — | — | null | UI-only; fine, document |
| areas list/create | ✓ | ✓ | project required | name ✓ | create: 500 on unknown project (fetch_one) | should be 404 |

Intentional asymmetries worth keeping (but documenting): `create_report`
MCP-only; `get_node` preview + `recent_project` + `project_plan` REST-only
(UI feeds); `propose_sprint` (MCP name) vs `POST /api/proposals` (same op).
Accidental asymmetries to fix: link disposition missing from MCP; REST WI
category patch; REST vs MCP `get_work_item` shape; card project move
id-vs-name.

### 4.2 Error-status matrix (live-verified, scratch instance)

| Probe | Today | Target |
|---|---|---|
| PATCH /api/work-items/9999 | 200 `{"ok":true}` | 404 |
| GET /api/work-items/9999 | 200 `null` | 404 (D-6) |
| GET /api/reports/9999 | 500 | 404 |
| GET /api/daily-plan?from=notadate | 500 | 400 |
| POST /api/work-items (tshirt "GIGANTIC") | 500 | 400 |
| POST /api/cards (status "Bogus") | 500 | 400 |
| POST /api/relationships (missing node) | 500 raw FK text | 404 |
| PATCH /api/cards/4 (node 4 is a WI) | 200, node archived | 404 "no card 4" |
| POST /api/work-items (area of other project) | 201 created | 400 |
| MCP update_work_item(9999) | `{"ok":true}` | isError not_found |
| MCP survey (archived omitted) | includes archived | match schema (D: keep both + fix schema text) |

### 4.3 Proposed contracts for the material changes

**Error body (REST)** — keep the string, add a stable code
(non-breaking for the current client, which reads `error` only):

```json
{ "error": "no card with node_id 4", "code": "not_found" }
```
codes: `invalid_input` (400), `not_found` (404), `conflict` (409),
`internal` (500). MCP error content: `{"message": "...", "code": "..."}`.

**Mutation acknowledgement** — every create/update returns the full row the
read path would return, e.g. `PATCH /api/work-items/:wi` →
`WorkItemRow` (or 404). Deletes → `{"deleted": true|false}` with 200.

**List envelope** — for `list_work_items`, `list_cards`, `list_links`,
`list_topics` (matching survey's existing shape):

```json
{ "items": [...], "total": 123, "limit": 200, "offset": 0 }
```
defaults `limit=200` (clamped ≤500), `offset=0`; each gains
`archived` (default **false** — a deliberate default change, D-3) and
per-entity filters (cards: `status`, `project`; links: `disposition`,
`read`; topics: existing `q`). Deterministic ordering with id tie-breakers,
stated in the tool description. UI updated in the same change.

**Collection signal / focused read (two-level contract generalized):**
`CardRow`, `ProposalRow`, `ReportRow`, `Topic` gain `comment_count: i64`.
Focused reads inline capped context with truncation flags, uniformly:

```json
// GET /api/work-items/:wi  == MCP get_work_item   (REST aligns to MCP)
{ ...WorkItemRow, "comments": [...≤10], "comments_truncated": false }

// NEW get_proposal (REST GET /api/proposals/:node_id, MCP get_proposal)
{
  "node_id": 17, "title": "...", "summary": "...", "status": "proposed",
  "rank": "2", "pinned": true, "project": "korg", "tags": [...],
  "archived": false, "created": "...", "updated": "...",
  "covered": [ { "wi_number": 2, "title": "...", "wi_status": "open",
                  "wi_tshirt": "M", "comment_count": 3 } ],
  "comments": [...], "comments_truncated": false, "comment_count": 1
}
```
`covered` ordered by wi_number; cap 50 with `covered_truncated` if ever
needed (proposals cover ≤ ~10 today). `list_proposals` rows gain
`covered_count`. This is the read `start-sprint` switches to, and the shape
the handoff plan extends (`handoff_count`/`handoffs`/`handoffs_truncated`).

**neighbors** — add optional `label` and `kind` filters and a `limit`
(default 100, flag `truncated`); direction guaranteed meaningful once F-01
lands; ordering `rel_id`.

Compatibility: REST consumers are the web UI (updated in lock-step) and
`plan-status` (reads `/api/projects/:name/plan`, unchanged). MCP consumers
are the skills — `{ok:true}` is never *read* by any skill (verified in the
skills audit), so richer returns are safe; the list envelope changes
`list_work_items`/`list_cards`/`list_links`/`list_topics` shapes, which
skills don't consume in enveloped form except `survey` (already enveloped).
kmon writes reports (create_report unchanged shape, plus documented finding
replacement).

---

## 5. Target Documentation Map and Sources of Truth

| Document | Audience | Content | Canonical source | Generated? | Drift check |
|---|---|---|---|---|---|
| README.md | humans, first contact | what korg is, current status (updated!), quickstart, pointers | prose | counts/inventories removed or generated | test asserting tool count matches `tools().len()` if the number stays |
| docs/setup.md | operator (Ken) | install, run, env vars, Docker | env table generated from one `EnvVar` inventory in code, or drift-tested | partially | test enumerating `env::var` call sites vs table |
| docs/usage.md | humans | UI tour + API overview, links to api.md | prose + generated REST/MCP tables | tables yes | route-inventory test |
| **docs/api.md (new)** | agents + humans | response contracts (§4.3), error codes, pagination, ordering, truncation flags, **relationship label registry**, intentional REST/MCP asymmetries | handwritten, normative | no | contract tests reference its tables |
| **docs/operations.md (new)** | operator | deploy (kubsdb), backup/restore + drill, health, logs (`KORG_LOG`), `--reset` warning, rollback | handwritten | no | deploy skill links here instead of duplicating |
| docs/migration.md | historical | legacy import; mark completed + point at --reset warning | handwritten | no | — |
| .claude/skills/* + user skills | agents | *workflow* guidance only; shapes/lifecycles by reference to docs/api.md | handwritten | no | skills audit item in sprint-ship checklist |
| sprints/* | historical | unchanged; explicitly non-normative | — | — | — |

Principles: exactly one normative home per fact (contracts → api.md; env →
setup.md; ops → operations.md); inventories are generated or drift-tested,
never hand-counted; tool descriptions in tools.rs are part of the contract
surface and get reviewed in the same PR as behavior changes.

---

## 6. UI/UX Assessment

Grounded in live browser checks (routes, viewports and evidence in §10) plus
markup audit. Overall: the UI is dense, fast, coherent in visual language,
and genuinely good at its two core loops (weekly planning; work-item triage).
The gaps are robustness (silent failures), accessibility, and a few places
where the UI is paying for missing API shapes.

**Information architecture.** Ten top-level routes is at the edge of what the
flat nav supports — at 390px the nav overflows with no affordance
[observed]. "Today / History / Topics" are one planning domain; "Planning"
(proposals) vs "Plan" (dependencies) is a naming collision that requires
learning. Suggest: group nav (Plan-family dropdown or section dividers) and
rename "Planning" → "Queue" or "Plan" → "Dependencies" [preference].
"Sprint Plan ID: 17" on Planning cards exposes internal node ids with a
third name (screenshot evidence); align on `korg:<node_id>` notation used by
the skills.

**Workflows.** Daily planner: strong (drag + Add fallback, frozen-past
copy-forward semantics surfaced clearly, TopicPicker is the a11y model).
Work items: the rail/table/detail flow works; quick-edit is efficient;
find-by-ID + NodePreview is a good cross-kind affordance. Weak spots: no
route per work item (detail is page-internal state; deep-linking relies on
find-by-ID); relationship editing is expert-only (free-text label, wi_number
target, no direction display — blocked on F-01 anyway); planner labels
truncate aggressively at desktop widths ("Unify error …" in a column with
room to spare, screenshot evidence).

**API/UI coupling.** Planning page N+1 (F-10); planner `refresh()` re-fetches
4 collections per checkbox toggle (F-25); client-side joins for related
labels and covers chips; `Neighbor.direction` ignored by the UI — reasonable
today given F-01, wrong after the fix.

**States.** Loading/empty/error handling inconsistent; link-up can render an
entirely empty page on API failure with no message (F-16). Success feedback
is instant-mutation + reload, which works but gives no confirmation for
destructive actions (F-17).

**Responsive.** Desktop 1440px: all 10 routes render correctly, no horizontal
overflow, no console errors [observed]. Mobile 390px: layouts stack
correctly (planner becomes a vertical week; cards board 2-across); no page
overflows; but nav clips (scrollable, undiscoverable), the cards Cut strip
clips, and the work-items table depends on inner horizontal scrolling
[observed]. Usable, not polished.

**Accessibility.** Detailed in F-17. Positive: global `:focus-visible`
outline, TopicPicker's full combobox pattern, sr-only labels on several
search inputs, keyboard row-activation (Enter+Space) on tables. Negative:
modal focus management absent, drag-only flows, unlabeled inputs, `<tr>`
interactivity without roles, no confirmations.

**Recommended UI investments, in order:** (1) shared error/toast + fetch
wrapper (F-16); (2) modal/slide-over primitive with focus management (F-17);
(3) domain.ts consolidation (F-15); (4) consume `get_proposal` and the WI
detail read, deleting the joins (after B3); (5) mobile nav + Cut-strip fixes;
(6) confirmations/undo on destructive actions.

---

## 7. Test and Operations Gaps

Prioritized; inventory detail in F-13/F-14 and the §10 notes.

1. **CI** (F-13): GH Actions — `cargo test --workspace` (with snapshot tests
   env-gated), `cargo clippy -D warnings`, `pnpm check`, `pnpm lint`. Add
   `just check` mirroring it locally.
2. **Backup visibility** (F-24, corrected): backups already run nightly via
   k-homelab's korg-backup recipe; document them in docs/operations.md +
   deploy-skill preflight check so the repo itself carries the recovery
   story.
3. **Contract/status tests** (F-02/F-03/F-04): REST tests asserting the §4.2
   target matrix; MCP tests for not-found + isError shapes. These lock in
   B1 and prevent regression.
4. **Missing-surface tests** (F-14): 14 MCP dispatch arms; REST reports/plan/
   reorder/delete-plan/update-comment/link-read; `get_node_preview` ×5 kinds;
   `project_edges`.
5. **Test-support consolidation** (F-14): `korg-test-support` crate
   (container bootstrap, builders); removes ~120 duplicated lines and makes
   new tests cheap.
6. **Snapshot-test gating** (F-13): `KORG_SNAPSHOT_TESTS=1` guard so a clean
   checkout passes `cargo test --workspace`; document in setup.md.
7. **Ops docs** (F-12/F-24): docs/operations.md; fix deploy skill env row
   (KORG_TIMEZONE); decide deploy-from-working-tree policy (D-9).
8. **Observability** (low): JSON log formatter flag for production
   (`tracing-subscriber` json feature is already compiled in); no metrics
   needed at this scale [preference].
9. **E2E**: suite is healthy (27/27 in 3.5s against a live server). Add one
   spec per new contract surface (proposal detail view) as B3/B6 land;
   rename `slot-schedule.spec.ts` (slots are gone).

---

## 8. Dependency-Ordered Cleanup Bundles

Sized as sprints in this repo's convention (S ≈ half a day, M ≈ 1-2 days,
L ≈ 3+ days of focused agent work).

---

**B1 — Core integrity: one error type, honest mutations** · size M-L ·
prereqs: none

Closes: F-02, F-03, F-04, F-05, F-06, F-07, F-18, F-21 (+ status-matrix
tests from §7.3).

Scope: unify on extended `RepoError` (Invalid/NotFound/Conflict) across
repo.rs/topics.rs (daily_plan maps in); every mutation existence+kind
checked, returns the entity; vocabulary validated app-side (tshirt, card
status, disposition; area∈project at create; parent must resolve); report
upsert replaces finding edges; REST/MCP error mapping + `code` field;
`--reset` wording + guard; `project` touch trigger.

Blast radius: every write path in core + both transports; the web UI reads
no mutation bodies except comments (`Comment` returns unchanged) so UI risk
is low; skills never read `{ok:true}` bodies (verified) so MCP risk is low.

Acceptance: §4.2 matrix all green in REST tests; MCP not-found probes return
`isError` with `code`; cross-kind card patch returns 404 and mutates
nothing; `cargo test --workspace` green.

Out of scope: response envelopes, pagination, direction, generation.

---

**B2 — Relationship semantics: label registry + direction backfill** ·
size M · prereqs: B1 (error shapes for relate validation)

Closes: F-01 (+ relate existence checks from F-02's list, self-edge guard).

Scope: label registry in core (covers/finding/depends_on/related-to +
semantics); `create_proposal`/`upsert_report` write semantic orientation;
migration backfilling covers/finding orientation from endpoint kinds;
`relate` validates endpoints exist (404) and rejects self-edges; `neighbors`
gains label/kind filters + limit; tool descriptions rewritten to state
per-label direction; docs/api.md section (can land as a standalone file
early).

Blast radius: relationship table (backfill is deterministic and reversible —
orientation only); neighbors consumers (skills verified tolerant: they
filter by label+kind, not direction, except refill-queue's depends_on use,
which is already direction-correct).

Acceptance: for every covers edge, left is the proposal; for every finding
edge, left is the report (SQL assertion in migration + test); direction
asserted from both ends in tests; `plan-status`/`refill-queue` depends_on
semantics unchanged.

Out of scope: handoff label itself (that's the handoff sprint).

---

**B3 — Response contracts: envelopes, two-level reads, get_proposal** ·
size L · prereqs: B1 (returns), B2 (covered refs read edges)

Closes: F-09, F-10, F-11, F-19, F-20 (+ REST/MCP asymmetry fixes: WI
category patch, MCP link disposition, REST WI detail alignment).

Scope: §4.3 contracts implemented — list envelopes + filters + archived
default false (D-3), comment_count generalization, REST get_work_item →
detail shape, `get_proposal` + `covered_count`, neighbors limit, ordering
tie-breakers, atomic link update, survey schema-default truthfulness,
tool-description sweep (archived inclusion, drop-and-report semantics). Web
UI updated in lock-step: planning page consumes get_proposal (deletes N+1),
work-items uses detail read, list callers pass envelope.

Blast radius: every list consumer (web pages, refill-queue's survey usage is
already enveloped; `start-sprint` switches to get_proposal — update the
skill in the same change).

Acceptance: no unbounded collection read remains on either surface; e2e
suite green after UI updates; `start-sprint` dry-run resolves a proposal
with one tool call; payload of MCP list_work_items(project) with 200-item
project stays under tool-output limits.

Out of scope: type generation (B4) — but B3 freezes the shapes B4 encodes.

---

**B4 — One contract source: shared structs, generated schemas + TS** ·
size M-L · prereqs: B3 (shapes final)

Closes: F-22, F-15 (types half), F-23.

Scope: request/patch structs shared korg-core ↔ korg-api ↔ korg-mcp (one
`double_option`); schemars-derived tool input schemas or a schema↔struct
parity test; ts-rs generation into web/src/lib/generated/; api.ts reduced
to wrappers; `web/src/lib/domain.ts` for presentation rules (terminal
statuses, kindLabel, midRank, chip classes) replacing the scattered
literals; delete dead client surface.

Blast radius: build tooling (a `just gen` step + CI check that generated
files are current); no behavior change intended — lock with e2e suite.

Acceptance: `git diff --exit-code` after `just gen` in CI; grep finds no
`"closed"`/`"Cut"` literals outside domain.ts; svelte-check green.

Out of scope: OpenAPI, runtime validation frameworks.

---

**B5 — Truth in docs and ops** · size M · prereqs: none to start; final
inventory pass after B3/B4

Closes: F-12, F-13, F-24 (+ §7 items 1, 2, 6, 7).

Scope: CI workflow + `just check` + snapshot-test gating; document the
existing korg-backup recipe (dump location, retention, restore command) in
docs/operations.md + deploy preflight check; docs/api.md skeleton (normative
error/contract doc — starts in B1/B2, completed here); README status/crates
fixes; usage.md regenerated tables; --reset wording (with B1's guard);
deploy skill KORG_TIMEZONE + protocol-version consistency; drift tests for
tool count/routes/env vars.

Blast radius: none in product code.

Acceptance: clean-checkout `just check` passes without snapshots or Docker
prod access; a documented restore actually performed once against a scratch
DB; docs audit items 1-11 (F-12) all closed.

---

**B6 — UI robustness and access** · size M-L · prereqs: B3 (for the
join-deletion parts); the F-16/F-17 halves can start any time

Closes: F-16, F-17 (+ §6 recommendations 1-2, 5-6; F-15's UI half if B4
hasn't landed yet).

Scope: shared fetch/error/toast; modal primitive with focus trap/restore/
Escape (NodePreview + cards modal); labeled inputs; row-activation pattern;
confirmations (or undo) for comment delete / relationship remove / archive
actions; mobile nav affordance + Cut strip fix; planner truncation +
relationship-label select; keyboard move-up/down for reorder.

Acceptance: axe-core pass on the 10 routes with no serious violations
(spot-check level, not a compliance program); e2e additions for confirm
flows; mobile screenshots re-taken at 390px show nav affordance and intact
Cut column.

---

**B7 — Test consolidation** · size S-M · prereqs: none (best interleaved,
listed for tracking)

Closes: F-14, F-08 (tiny migration), plus §7 items 4-5.

Scope: korg-test-support crate; the 14 missing MCP arm tests + REST gaps +
preview kinds + project_edges; fresh-install sequence migration + test;
rename slot-schedule spec.

---

**Post-filing additions (2026-07-22, Ken).** Two items worked in after the
bundles were filed:

- **Proposal project display/filter (WI 565, added to B3 / korg:555).**
  Proposals already carry a project (`ProposalRow.project`) but
  `list_proposals` filters only by status and the Planning page neither
  shows nor filters by project. B3 adds the filter (REST + MCP) and the
  Planning-page chip + filter control.
- **Eval-created work items (WI 466, tagged into the cohort as a design
  input, mechanism undecided).** Agents under evaluation can create real
  WIs (the loglens project). korg is no-auth HTTP, so flagging must ride
  the payload or convention. Candidates (comment on #466): eval tag
  convention + B3 tag-exclusion filters; designated inactive eval projects;
  a scratch korg instance for evals (near-free after B5's CI bootstrap); a
  first-class `origin` field (cheap after B4's generation). B3's filter
  design should anticipate tag-based exclusion either way.

**Sequencing and the handoff plan.** B1 → B2 → B3 are strictly ordered and
are the review's answer to the handoff plan's precondition ("response, API,
relationship, documentation cleanup" — handoff-node-plan.md:278-280). **The
handoff sprint can start after B3**, with B4 strongly recommended first so
handoff types are born generated rather than hand-mirrored. B5 can run in
parallel at any point (backup + CI should land *first*, honestly — they're
independent and cheap); B6/B7 fill gaps opportunistically.

---

## 9. Decisions — RESOLVED with Ken, 2026-07-22

All decisions were reviewed interactively; outcomes below are normative for
the cleanup bundles.

- **D-1 (B2) — RESOLVED: readers orientation-blind.** Undirected labels
  (e.g. `related-to`) keep current storage; readers treat orientation as
  meaningless for labels the registry marks undirected. No canonicalization
  migration. Reverse-duplicate edges for *directed* labels remain legal and
  meaningful (A depends_on B and B depends_on A is a cycle, not a dup).
- **D-2 (B1) — RESOLVED: constrain `wi_type` to a vocabulary including
  `brainstorm`:** `[task, bug, chore, feature, brainstorm]`, validated like
  `WI_STATUSES`, enum in the MCP schema. Before enforcement, B1 must query
  the live corpus for other values in use and surface them rather than
  break existing rows.
- **D-3 (B3) — RESOLVED: clean break, lock-step.** List envelopes + filters
  + `archived=false` default land in one sprint with the UI, skills, and
  tool descriptions updated together. No dual shapes.
- **D-4 (B4) — RESOLVED: full generation.** `schemars`-derived MCP input
  schemas and `ts-rs`-generated TypeScript, with a `just gen` step and CI
  freshness check.
- **D-5 (B1) — RESOLVED: add `code` field.** REST errors become
  `{"error": string, "code": "invalid_input"|"not_found"|"conflict"|"internal"}`
  (non-breaking); MCP error content mirrors `{message, code}`.
- **D-6 (B1) — RESOLVED: 404 / isError.** Missing single-item reads return
  404 on REST and `isError` not-found on MCP, uniformly across all kinds;
  the find-by-ID UI switches on response status instead of null.
- **D-7 (B1) — RESOLVED: replace findings.** Same-(source,date) report
  re-runs replace the finding edge set transactionally (delete stale, add
  new); tool description updated.
- **D-8 (B5) — RESOLVED: korg is the live tool.** kwi/kcard are no longer
  the systems of record. README drops the Milestone-1 framing and states
  korg is the system of record.
- **D-9 (B5) — RESOLVED: clean tree + SHA for kubsdb.** deploy-kubsdb
  refuses a dirty working tree and stamps the image with the git SHA.
  Web-UI iteration moves to a dev loop on kai (vite dev server against a
  local or prod API per README's existing hot-reload flow) — kubsdb only
  ever receives clean-tree builds.
- **D-10 (B3) — RESOLVED: keep `list_work_items`, enveloped.** Full rows
  for one project remain a single call; envelope + limit clamp +
  `archived=false` remove the footguns; description steers cross-project
  callers to `survey_work_items`.
- **Flagged items — RESOLVED:** add an atomic `update_link` MCP tool
  (disposition + read + tags in one transaction, restoring 0004's intended
  agent workflow; `mark_link_read` deprecated into it) in B3; reject
  self-edges in `relate` (`InvalidInput`) plus a `CHECK (left_id <>
  right_id)` after verifying none exist in prod, in B2. `neighbors`
  pagination stays at the B3 limit+flag level.

---

## 10. Validation Log

Environment: kai (Linux 6.8), Docker 29.6.1, rustc via rust-toolchain
(stable), pnpm 10.33.2, Node 24.15. Scratch Postgres 17 container
(`korg-review-pg`, 127.0.0.1:5433, created for this review, destroyed
after). korg-api run locally at 127.0.0.1:8091 with
`KORG_TIMEZONE=America/Denver`, `KORG_WEB_DIR=web/build`. Production kubsdb
untouched.

Commands and outcomes:

- `cargo build --workspace` — ok.
- `cargo test --workspace` — **36 passed, 0 failed** across 15 suites
  (includes korg-migrate fidelity/import/read_sources, which passed because
  this machine has local `snapshots/*.dump` from Jun 24; these fail on a
  clean checkout — F-13).
- `pnpm install`, `pnpm check` — 0 errors, 0 warnings (319 files).
- `pnpm lint` — clean. `pnpm build` — ok (adapter-static).
- Seeded scratch instance over REST (2 projects, 3 areas, 6 work items incl.
  archived + parent + 12-comment thread, 3 cards, 2 links, 2 topics, 5
  daily-plan items, 2 proposals, 1 MCP-created report with findings).
  Seeding itself surfaced F-08 (first node id = 2) and F-06 (parent
  silently cleared) — the failures are recorded verbatim in the review
  transcript.
- REST probe matrix — §4.2 "Today" column, all live-observed.
- MCP probes via `POST /mcp` (stateless JSON): `update_work_item(9999)` →
  ok:true; `get_work_item(9999)` → null success; `survey_work_items`
  (archived omitted) → archived item included; `create_report` with one
  bad finding number → created, `findings_linked:[5]`; `relate` to missing
  node → isError with raw FK text.
- Cross-kind hazard: `PATCH /api/cards/4` (node 4 = work item) with
  `archived:true,title:"hijack"` → ok:true; DB showed node 4 archived, WI
  title untouched; reverted immediately.
- Playwright e2e: `KORG_E2E_URL=http://127.0.0.1:8091 npx playwright test`
  — **27 passed** (3.5s), Chromium.
- Browser sweep (custom Playwright script): routes `/`, `/history`,
  `/topics`, `/plan`, `/cards`, `/work-items`, `/planning`,
  `/daily-reports`, `/reading-list`, `/link-up` at **1440×900** and
  **390×844**, full-page screenshots + horizontal-overflow measurement +
  console/pageerror capture. Results: no page-level horizontal overflow on
  any route at either size; zero console errors; nav at 390px is
  `overflow-x:auto` with content to x≈797 (measured); cards Cut strip and
  work-items table clip at 390px (screenshots). Screenshots retained in the
  session scratchpad (not committed); representative observations recorded
  in §6.
- Workflows exercised by the e2e suite against this instance: card CRUD +
  DnD + cut-autohide + launch links; WI create/edit/relate/archive/comments/
  markdown; project sticky; area add; planner topic pick/create, complete/
  reorder/move/delete, frozen-past copy (mocked); history stats (mocked);
  link-up filter/link/show-all; reading-list add.

Checks not completed / limitations:

- No performance testing at production data volume (scratch DB is small);
  F-25 items are code-shape observations, not measurements.
- Fidelity suite ran against the local Jun-24 snapshots, not fresh dumps.
- deploy-kubsdb, backup, and Tailscale-HTTPS claims not exercised (out of
  scope per constraints); D6/D7 in the deploy-skill audit are
  runtime-unverifiable from the repo.
- Screen-reader behavior not tested with an actual AT; a11y findings are
  markup/focus-behavior level.
- Sub-investigations (web inventory, docs/skills audit, test/ops inventory)
  were executed by three read-only survey agents; every finding cited in
  §3 that originated there was re-verified against the code or live
  instance before inclusion. Their full inventories informed §4-§7.
- Phase 2 reconciliation (live korg instance, 2026-07-22): `list_work_items`
  (project korg) + `list_proposals` reviewed for overlap before filing.
  One open korg WI exists (#466, eval-flagging — unrelated). Closed WI #234
  corrected F-24 (backup exists via k-homelab korg-backup, restore-verified);
  the live `wi_type` corpus (task, bug, feature, research, tweak, brainstorm
  — no `chore`) informs the D-2 vocabulary work item.

---

## 11. Ranked Top Ten (benefit ÷ implementation risk)

1. **Kind-guard + existence-check mutations (F-03/F-04).** Tiny mechanical
   change; kills the two worst agent hazards (success-lies, cross-kind
   writes). Risk ≈ zero. (B1)
2. **Document the existing backup story (F-24, corrected).** One doc + one
   preflight line; makes recovery knowledge discoverable from the repo. No
   code risk. (B5)
3. **Error unification + status matrix tests (F-02).** Mechanical, high
   agent-facing payoff, locked by tests. (B1)
4. **CI + `just check` + snapshot gating (F-13).** Makes every later bundle
   safer; zero product risk. (B5)
5. **`get_proposal` + planning/start-sprint consumption (F-10).** Small
   additive endpoint; deletes an N+1 and a skill workaround; direct handoff
   prerequisite. (B3)
6. **Relationship direction fix + backfill (F-01).** Medium risk (data
   migration) but deterministic and reversible; unblocks handoff semantics.
   (B2)
7. **Docs truth sweep (F-12).** Pure editing; large trust payoff for agents
   and future-Ken; drift tests keep it done. (B5)
8. **Two-level read generalization + envelopes (F-09).** Highest total
   benefit of the API items; medium risk (UI lock-step) — hence sixth not
   third. (B3)
9. **web domain.ts + shared error surface (F-15/F-16).** Contained to web;
   removes the silent-failure class and the literal sprawl. (B6/B4)
10. **Report finding replacement + silent-drop documentation (F-06/F-07).**
    Small, closes real kmon-facing correctness gaps. (B1)

---

*End of review. Decisions resolved 2026-07-22 (§9); Phase 2 filed the
cleanup in korg (project 11, tag `cleanup-2026-07`).*

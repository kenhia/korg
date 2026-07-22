# korg Deep Review — Spring Cleaning

Date: 2026-07-21
Reviewer: Fable
Repository: `korg` (this workspace)

## Purpose

korg has grown through twelve sprints and now carries the accumulated cost of
that growth: drift between layers, documentation that no longer matches code,
duplicated inventories, and response contracts that were shaped by whatever the
calling surface needed at the time. The goal of this review is not an audit for
its own sake. It is to produce a concrete, dependency-ordered path to getting
korg into a **clean, maintainable, well-architected** state, and to file that
path as executable work in korg itself.

This review is the prerequisite for the cleanup implementation that follows it,
and for `sprints/planning/2026-07-21-handoff-node-plan.md`, which is explicitly
blocked on this cleanup landing.

## Phases

The review runs in two phases with a checkpoint between them.

**Phase 1 — Review (do this now).** Analyze and write `sprints/review/REVIEW.md`.
Do not implement fixes, modify product code, refactor, or prepare commits.

**Phase 2 — File the work in korg (only after Ken says go).** Turn the cleanup
bundles from Phase 1 into korg work items and sprint proposals. Mechanics are in
[Phase 2: Filing the Cleanup in korg](#phase-2-filing-the-cleanup-in-korg).

Stop at the end of Phase 1 and present the executive assessment plus the proposed
bundles. Ken decides what gets filed, and whether the shape is right, before
anything is written to korg.

## Operating Constraints

- Write only inside `sprints/review/`. Create `REVIEW.md` and, only when they
  materially improve auditability, optional supporting files alongside it.
- Read the repository broadly enough to verify conclusions against the owning
  code and tests.
- Run existing tests, lint checks, builds, type checks, and local browser checks
  freely. You are working in the live workspace — use it. `just build`,
  `just test`, the scripts in `scripts/`, and the web dev server are all fair
  game, and browser checks against a locally running instance are expected
  rather than optional.
- Do not deploy, alter production data on `kubsdb`, install system software, or
  change machine configuration. If a check needs a database, use a local or
  scratch one.
- Avoid modifying generated files, caches, snapshots, lockfiles, test artifacts,
  or anything outside `sprints/review/`.
- Treat sprint documents in `sprints/` as historical evidence: they explain
  intent and evolution, but they are not proof of current behavior.
- Widen scope at your discretion when evidence points to adjacent risks,
  inconsistencies, missing abstractions, or undocumented behavior.

## Review Method

Begin with the database schema and migrations, the core domain and repository
abstractions, and the transport registration/dispatch surfaces. Build an
inventory before drawing broad conclusions. Then trace representative operations
end to end through:

1. database schema, constraints, and migrations (`crates/korg-migrate`);
2. Rust core/domain/repository code (`crates/korg-core`);
3. REST routes, request parsing, validation, and response shaping (`crates/korg-api`);
4. MCP tool schemas, registration, dispatch, descriptions, and responses (`crates/korg-mcp`);
5. TypeScript client code and types (`web/src`);
6. UI state, joins, views, and workflows (`web/src`);
7. agent-facing skills, prompts, and operational documentation (`.claude/`, `docs/`, `README.md`).

Verify each material finding against the code or test that owns the behavior.
Use version history selectively when it resolves intent, regression risk, or
migration rationale; history is supporting evidence, not authority.

Clearly distinguish **observed facts**, **evidence-backed inferences**, and
**recommendations or design preferences**. Cite concrete files and symbols.
Record uncertainty, conflicting evidence, checks performed, and checks that could
not be completed. Prefer exact examples and operation-level analysis over generic
advice.

Two standing biases for this review, given its purpose:

- **Prefer the actionable finding.** A finding that cannot be turned into a
  work item with acceptance criteria is usually not worth writing down. If
  something is wrong but you cannot say what "fixed" looks like, say so
  explicitly and mark it as needing a decision from Ken.
- **Name the target state, not just the defect.** For each structural problem,
  describe the architecture you would end at. The cleanup needs a destination,
  not just a list of things that are wrong.

## Minimum Scope

### Target Architecture

This section is the spine of the review. Everything else feeds it.

- Describe korg's current de facto architecture — layers, ownership boundaries,
  and where each concept is actually defined versus where it is re-derived.
- Propose the target architecture: what each crate owns, where the source of
  truth for the domain model lives, how REST and MCP should relate to the core
  (shared handler layer? separate presenters over a common service?), how types
  reach TypeScript, and what the UI is allowed to assume.
- Identify the specific gaps between current and target, and which are load-bearing
  — that is, which ones keep causing the other findings.
- Be opinionated. Where two defensible designs exist, pick one, say why, and note
  what the other buys.

### Documentation and Architecture

- Check documentation claims against current code, schemas, commands,
  configuration, and behavior. `README.md`, `docs/migration.md`, `docs/setup.md`,
  and `docs/usage.md` are all in scope, as is agent-facing guidance under `.claude/`.
- Propose a coherent documentation map: intended audiences, sources of truth,
  generated versus handwritten boundaries, and drift checks.
- Cover the data model, migrations, core domain concepts, MCP API, REST API,
  TypeScript client, UI, agent skills, deployment, configuration, operations,
  backup/recovery, and contributor documentation.
- Identify stale, duplicated, missing, misplaced, or contradictory documentation,
  including terminology drift and undocumented invariants.

### DRYness, Structure, and Ownership

- Assess meaningful duplication and structural boundaries across Rust core, REST
  handlers, MCP schemas and dispatch, TypeScript client/types, UI joins, tests,
  and skills.
- Focus on duplication that creates behavioral drift, inconsistent validation,
  response mismatch, fragile joins, or maintenance cost. Do not recommend
  abstraction merely to reduce line count.
- Identify unclear ownership, leaky layers, repeated inventories, parallel type
  definitions, hand-maintained mappings, and abstractions that are either missing
  or unjustifiably complex.

### Data Model and Integrity

- Review entities, identifiers, lifecycle states, timestamps, nullability,
  defaults, uniqueness, foreign keys, delete behavior, indexes, and
  database-enforced versus application-enforced invariants.
- Inspect migration ordering, forward compatibility, assumptions about existing
  data, idempotence where expected, rollback/recovery implications, and schema
  drift risk.
- Examine generalized relationships, directionality, labels, duplicate
  prevention, referential integrity, comments, daily planning, reports,
  proposals, topics, links, projects, areas, cards, and work items.
- Look for race conditions, partial writes, ambiguous identity, orphaning,
  ordering instability, and compound operations lacking transactional guarantees.

### MCP and REST

- Produce a complete MCP-to-REST inventory and parity assessment. Explain
  intentional asymmetry as well as accidental gaps.
- Review registration and discoverability, names, descriptions, parameter
  schemas, defaults, required/optional/null semantics, validation, filtering,
  sorting, pagination, limits, truncation, error taxonomy, status mapping, and
  consistency.
- Assess whether tool descriptions give agents enough information to choose tools
  safely, understand lifecycle rules, avoid expensive broad calls, and recover
  from errors.
- Review compound operations for atomicity, partial-success semantics,
  idempotence, retry safety, and response clarity.
- Trace representative successes, empty results, invalid inputs, not-found cases,
  conflicts, partial failures, and large-result behavior.

### Operation-by-Operation Response Design

For every externally meaningful MCP and REST operation, assess the actual
response contract rather than only endpoint presence. Determine whether responses
return too much, too little, or the wrong shape for likely consumers.

Sprint 012 established a two-level retrieval contract worth treating as
precedent: collection views signal that more context exists (`comment_count`),
and focused reads inline that context up to a cap with an explicit truncation
flag (`comments_truncated`). Assess where that pattern is honored, where it is
violated, and where it should be generalized.

Specifically examine:

- hidden comments, relationships/neighbors, metadata, or state that force
  follow-up calls;
- client-side joins and repeated fetches that should be server-shaped, versus
  over-expanded payloads that create output or coupling costs;
- bulk/list versus detail representations;
- total counts, page information, cursors/offsets, truncation indicators, and
  output limits;
- deterministic ordering and tie-breakers;
- stable identifiers, serial numbers, references, relationship IDs, and link
  direction;
- mutation acknowledgements, created/updated records, no-op reporting, partial
  success, and error details;
- consistency between Rust models, JSON, MCP schemas, TypeScript types, and UI
  assumptions.

For each material response-design issue, propose an exact contract: fields,
nesting, optionality/null behavior, ordering, pagination metadata,
limit/truncation behavior, and error or partial-success shape. Note compatibility
and migration implications.

### UI and UX

- Perform a UI/UX review grounded in the implemented product rather than presumed
  intent.
- Assess information architecture, navigation, find/preview/detail flows, editing
  and destructive actions, daily planning, status transitions, relationships,
  comments, reports, proposals, topics, projects, links, and cross-object
  workflows.
- Review responsive behavior, accessibility semantics, contrast, focus
  management, keyboard operation, screen-reader naming, loading/empty/error/
  success/disabled states, confirmation and undo behavior, discoverability,
  density, consistency, and feedback latency.
- Identify where UI behavior depends on fragile client joins, stale assumptions,
  missing API data, or inconsistent domain rules.
- Run browser checks at representative desktop and mobile sizes against a local
  instance. Record routes, viewport sizes, and workflows exercised. Do not claim
  visual validation without performing it.

### Agent Workflows and Skills

- Review agent-facing skills, prompts, tool guidance, examples, and workflow
  documentation against live MCP and REST behavior. The `start-sprint`,
  `sprint-ship`, and `refill-queue` trio is the primary workflow; korg is also
  consumed by `plan-status` and by ad-hoc agent sessions.
- Look for omissions, stale tool inventories, misleading lifecycle guidance,
  unsafe defaults, over-broad retrieval patterns, missing pagination guidance,
  response-shape drift, and instructions that encourage partial or non-atomic
  workflows.
- Assess whether common agent tasks are discoverable, efficient, auditable, and
  recoverable, and whether compound tools are warranted.
- korg is dogfooded heavily. Where the agent experience is bad, say so plainly —
  that is a first-class product defect here, not a documentation nit.

### Tests, Operations, Performance, Security, and Reliability

- Assess coverage by layer and behavior: schema/migrations, repository/domain
  logic, REST, MCP transport and schemas, TypeScript client, UI components,
  end-to-end workflows, skills, and documentation examples.
- Identify high-value missing tests, weak assertions, duplicated fixtures,
  unrealistic test paths, untested failures, and gaps between transport parity
  tests and semantic behavior tests.
- Review CI, build/release/deploy instructions, configuration, secrets handling,
  logs, metrics, tracing, health/readiness signals, failure diagnostics,
  backup/restore, and recovery validation.
- Account for the trusted-network operating model (tailnet, `kubsdb` deploy)
  without treating it as immunity. Report performance, security, integrity, and
  abuse risks in proportion to realistic exposure and impact.
- Consider query growth, N+1 behavior, unbounded output, memory use, database
  contention, blocking work, transaction scope, and operability as the dataset
  grows.

## Required Deliverables

Write `sprints/review/REVIEW.md` with all of the following sections:

1. **Executive Assessment**
   - A concise overall judgment, major strengths, systemic risks, and the most
     important corrective direction.
2. **Target Architecture**
   - Current de facto architecture, proposed target, the load-bearing gaps
     between them, and the decisions Ken needs to make.
3. **Prioritized Findings**
   - Give every finding a stable ID (e.g. `F-01`).
   - Assign severity as `critical`, `high`, `medium`, or `low`, and state
     confidence.
   - Include evidence, impact, recommendation, uncertainty, and relevant
     validation for each finding.
4. **MCP/REST API-Response Matrix**
   - Inventory operations and compare availability, inputs, validation, errors,
     filtering, pagination, ordering, response shape, limits, consumers, and
     parity.
   - Include exact proposed contracts for material response changes.
5. **Target Documentation Map and Sources of Truth**
   - Propose the document set, audience, ownership, canonical source, generated
     versus handwritten boundaries, and drift checks.
6. **UI/UX Assessment**
   - Cover workflows, information architecture, responsive behavior,
     accessibility, keyboard use, states, browser evidence, and API/UI coupling.
7. **Test and Operations Gaps**
   - Prioritize missing tests and CI/deploy/config/observability/backup/recovery
     improvements.
8. **Dependency-Ordered Cleanup Bundles**
   - Group remediation into coherent, dependency-ordered bundles. Each bundle
     should be a plausible sprint.
   - For each bundle: rationale, the finding IDs it closes, prerequisites, blast
     radius, acceptance criteria, rough size, and explicit out-of-scope work.
   - Do not force the work into one sprint; size and sequence it according to
     dependencies and risk.
   - Note explicitly which bundle(s) must land before the handoff node plan in
     `sprints/planning/2026-07-21-handoff-node-plan.md` can start.
9. **Questions and Decisions Needed**
   - Unresolved questions, ambiguous intent, tradeoffs, and places where you
     disagree with the apparent current direction. Flag anything that blocks
     Phase 2 filing.
10. **Validation Log**
    - Commands and browser checks run, outcomes, environment limitations,
      skipped checks, and evidence-validation notes.
11. **Ranked Top Ten by Benefit Relative to Implementation Risk**
    - Rank the ten best improvements by expected benefit relative to
      implementation risk, with brief reasoning and dependencies.

Optional support files may hold large inventories, matrices, query notes, or
browser observations when keeping them in `REVIEW.md` would hurt readability.
`REVIEW.md` must remain self-contained enough to understand every finding and
recommendation without them.

## Phase 2: Filing the Cleanup in korg

Do not start this until Ken has read the Phase 1 output and told you to proceed.
He may accept the bundles as-is, reshape them, or file only some of them.

When given the go-ahead, use the korg MCP tools to turn the accepted bundles into
executable work. korg's own project is **`korg`, `project_id` 11**.

**Work items.** For each discrete piece of remediation, `create_work_item` with:

- `project_id: 11`;
- a title that names the change, not the symptom;
- `content` stating the problem and the desired end state in a few sentences;
- `details` carrying the evidence — file and symbol citations, the proposed
  contract or schema change, and acceptance criteria;
- `wi_type` matching the work (`task`, `bug`, `chore`, or whatever the existing
  corpus uses — check `list_work_items` for the conventions in play rather than
  inventing new ones);
- `wi_tshirt` sized honestly; use `Unknown` rather than guessing wildly;
- a shared tag so this cohort is retrievable as a set — use `cleanup-2026-07`,
  plus a per-area tag where it helps.

Cross-reference each work item back to its finding ID from `REVIEW.md` in the
`details`, so the review stays the authority for evidence.

**Sprint proposals.** For each cleanup bundle Ken accepts, `propose_sprint` with:

- `project_id: 11`;
- the bundle's title and a `summary` that carries rationale, prerequisites, and
  acceptance criteria — enough that `start-sprint` can run from it without
  re-reading `REVIEW.md`;
- `work_item_numbers` listing the work items that bundle covers;
- `rank` set so the dependency order is the queue order, lowest first;
- the `cleanup-2026-07` tag.

Check the response from `propose_sprint` for which `wi_numbers` actually resolved
to covered items, and report any that did not.

**Before writing anything**, run `list_work_items` scoped to `korg` and
`list_proposals`, and reconcile against what already exists. A good deal of this
cleanup may already be filed from earlier sprints. Do not create duplicates —
prefer updating an existing item, and report what you found rather than silently
merging.

**After writing**, report back: every work item created with its `wi_number`,
every proposal with its `node_id`, anything you chose not to file and why, and
any existing items you think should be closed or superseded as a result of the
review.

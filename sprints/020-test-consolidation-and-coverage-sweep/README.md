# Sprint 020 — Test consolidation and coverage sweep

Proposal `korg:559` ("Cleanup B7"). Covers WI #550, #551, #552, #574.

The last of the 2026-07 review's seven bundles. B1–B6 changed korg's contracts:
typed errors, honest mutations, response envelopes, one contract source, docs
that match the code, a client that admits failure. Each of those was verified by
tests written for *that* change. This one is about everything the six sprints
walked past — the surfaces nothing has ever called, and the harness those tests
were written on top of.

The theme, stated once: **a test suite is only evidence for what it actually
executes.** korg had 6,800 lines of tests and 10 MCP tools that no test had ever
dispatched. Sprint 016 found one of them the hard way — it changed how
`create_report` parses its date and discovered nothing in the repo would have
caught a regression.

This document began as the plan agreed before implementation and has been
updated in place, so what follows is what was built — including the four places
where measuring something changed the plan.

## The inventory, re-measured

The work items were written on 2026-07-22 against the review's numbers. Six
sprints have landed since, so every claim below was re-counted on this branch
before planning against it. Two numbers moved.

| Claim (WI) | Measured now | Verdict |
|---|---|---|
| ~10 copy-pasted `fresh_korg` harnesses | **17** files constructing `Postgres::default()` | worse than stated |
| duplicated `NewWorkItem` literals | **30** across 10 files | confirmed |
| duplicated `count()` in migrate suites | **3** | confirmed |
| 14 never-dispatched MCP arms | **10** | four had since been covered |
| `get_node_preview` untested for 5 of 7 kinds | only `workitem` + `card` covered | confirmed |
| `project_edges` has zero coverage anywhere | one caller, no test | confirmed |
| fresh DB cannot mint node #1 | confirmed against 0009 and `GREATEST` semantics | confirmed |

The four arms that quietly got covered since the WI was written — `get_topic`,
`list_topics`, `search_topics`, `get_report` — are the proposal's sequencing
note working as intended: the contract bundles wrote tests against the surfaces
they touched, and this sprint swept the remainder. The remaining 10 were the
ones no bundle happened to touch:

```
archive_topic          delete_daily_plan_item   update_comment
create_report          reorder_daily_plan       update_project
list_reports           daily_plan_history       update_topic
mark_link_read
```

REST, measured the same way — routes in `korg-api/src/lib.rs` against paths any
suite requested:

```
GET    /api/areas
GET    /api/projects/:name/plan     ← sole caller of project_edges
GET    /api/reports
PUT    /api/daily-plan/:plan_date/order
DELETE /api/daily-plan/:node_id
```

The WI also listed `PATCH /api/comments/:id` and the links `read` field as
uncovered; both had since gained tests. Recorded so the delta is visible rather
than silently dropped.

## Front 1 — `korg-test-support` (#550)

`crates/korg-test-support`, a dev-dependency crate. Not published, not a runtime
dependency of anything, so it ships in no binary.

- `start_pg()` — the one place in the workspace that starts a container, pinned
  to `18-alpine`. `fresh_korg()` (migrated, via `korg_core::connect` — the path
  production takes) and `raw_postgres()` (unmigrated, for suites whose subject
  *is* the migrator) are the two bootstraps over it.
- `new::{work_item, card, link, proposal, report}` — every optional field
  defaulted to what the surfaces' serde defaults produce, so a test names only
  the fields it asserts on.
- `count(pool, table)` for the migrate suites.

**Net: 626 lines deleted, 89 added.** The 17 bootstraps became one. The 30
`NewWorkItem` literals, which spelled out 13 fields each to set two, became
struct-update expressions over the builders.

The copies had already drifted, which is the argument for doing this at all:
some connected via `korg_core::connect`, some hand-rolled a pool with a
different `max_connections`, and `schema.rs` alone ran against Postgres's
*default* tag rather than the pinned 18-alpine every other suite used. A test
could pass or fail depending on which harness its file happened to inherit.

**Deviation from the plan.** The plan put the MCP `server()`/`args()`/`body()`
scaffolding in the shared crate too. That would have made `korg-test-support`
depend on `korg-mcp` while being its dev-dependency — a cycle Cargo tolerates
but nobody should have to reason about. Surface-specific scaffolding went into
each crate's own `tests/common/` instead (`korg-mcp/tests/common/`,
`korg-api/tests/common/`), which is where it can see the crate under test. The
shared crate depends on `korg-core` and nothing else in the workspace.

A `#[cfg(test)]` fence in the crate asserts the builders' hard-coded defaults are
members of the vocabularies. Hard-coding is deliberate — a builder reading
`vocab::CARD_STATUSES[0]` would silently follow a reordering instead of failing
— but a value that leaves the vocabulary entirely must not go unnoticed.

## Front 2 — fresh-install node sequence (#552)

`0009_identity.sql:74` ended with

```sql
PERFORM setval(pg_get_serial_sequence('node', 'id'),
               GREATEST((SELECT MAX(id) FROM node), 1));
```

Right on a populated database, wrong on an empty one: `MAX(id)` is NULL,
`GREATEST` ignores NULLs and yields 1, and two-argument `setval` sets
`is_called = true`. The first `nextval` therefore returned 2, so on a fresh
install node #1 — and work item #1 — could never exist. Invisible in production,
where data was always present.

`0015_node_sequence_fresh_install.sql` resets the sequence with
`is_called = false` **only when `node` is empty**. 0009 is untouched (sqlx
checksums it). `korg-migrate/src/import.rs:163` carried the same idiom; it was
harmless there because the import always inserts first, but it is now the
three-argument form too — leaving a copy of a wrong idiom around invites the bug
back.

Both halves are tested in `identity.rs`: a fresh database mints node id 1, and
re-applying 0015 against a populated database does not rewind the sequence. The
second is the half that would actually hurt.

## Front 3 — the dispatch-completeness fence (#551)

`crates/korg-mcp/tests/dispatch.rs`. A table maps all 44 advertised tools to a
valid argument object against one seeded database, and the test asserts the
fixture set **equals** `tools()`'s name set, then dispatches every one.

A new tool with no fixture fails the first assertion. That is the entire point:
a tool cannot ship without something calling it.

Destructive tools (`delete_comment`, `unrelate`, `delete_daily_plan_item`) each
get their own entity, and each daily-plan tool its own date, so the order tools
happen to be dispatched in cannot matter. A fixture table whose correctness
depends on iteration order is a trap for whoever adds the 45th tool.

Sprint 016's `every_advertised_tool_has_a_handler` is **deleted**, not left
alongside. It grepped `tools.rs` for each tool's string literal, which proved an
arm existed — it would have passed against `todo!()`, and it did pass throughout
the period when ten tools had never been called by anything. Two tests for one
property, one of which cannot fail when the other passes, is a slower way to
learn the same thing.

The fence is a floor, not a substitute for behaviour: it proves each arm runs.
Fronts 4 and 5 prove the interesting ones are right.

## Front 4 — the reports trio (#574)

`crates/korg-mcp/tests/reports.rs`, five tests:

- a same-`(source, report_date)` re-run **keeps** the `node_id` (so comments and
  relationships survive) and **replaces** the finding edge set rather than
  accumulating it — asserted against the edge set via `get_report`, not the
  response echo, and against the row count, so a re-run that created a second
  report would fail too;
- `findings_linked` echoes only the `wi_number`s that resolved, checked with a
  mixed real/bogus request;
- a malformed `report_date` (`11/07/2026`, `2026-7-11`, `yesterday`, empty) is a
  caller error, not a 500 — the dispatch-side half of sprint 016's serde unit
  fence;
- `list_reports` source filter, newest-first ordering, and `limit`;
- `get_report` returns the markdown body and the linked findings, and a missing
  node id is `not_found` rather than an empty success.

## Front 5 — the rest of the sweep (#551)

`crates/korg-mcp/tests/sweep.rs` (13 tests) and `crates/korg-api/tests/sweep.rs`
(7 tests), each undispatched arm getting a happy path and an error path.

The plan view got the most attention it was owed: `GET /api/projects/:name/plan`
is the only caller of `project_edges`, and it feeds both the `/plan` UI and the
`plan-status` skill. The assertion that matters is scoping — an edge belongs
only if *both* endpoints are in the project — because an edge leaking in from
another project draws a dependency arrow between nodes the view cannot render,
and the failure mode is a wrong answer to "where are we on the plan", not a
crash.

`get_node_preview` now covers all seven kinds. Each of the five new ones asserts
a field only its own branch can produce, because an uncovered kind degrades to
the fallback title `"{kind} #{id}"` — a preview that renders, looks deliberate,
and says nothing.

### Three things the sweep found

**1. DB-CHECK violations were reported as `internal`. Fixed.**

An empty comment body or link URL was rejected — the 0001/0002 `CHECK` constraints
worked, nothing blank was ever stored — but the failure arrived as an
`sqlx::Error`, which classifies as `internal`. The caller was told:

```
{"code":"internal","message":"error returned from database: new row for relation
 \"comment\" violates check constraint \"comment_body_nonempty\""}
```

`internal` means korg's problem. Since sprint 019 the web client renders it as
an apology and a retry suggestion — precisely the wrong advice for input that
will never be accepted. `repo::require_non_empty` is now the polite front door
on `add_comment`, `update_comment` and `create_link`, returning `invalid_input`.
The CHECK constraints stay: this is a front door, not a replacement for the
guarantee.

This was in scope — WI #551's acceptance asks for these paths asserted, and
asserting the old behaviour would have enshrined a bug.

**2. A partial reorder is a `conflict`, not `invalid_input`.**

The test was written expecting `invalid_input` and was wrong: `PlanningError::
InvalidReorder` maps to `Conflict` deliberately, because the request is
well-formed and disagrees with stored state (sprint 013's taxonomy). The test
now asserts `conflict` and says why.

**3. `list_reports` returns a bare array, not the response envelope. Recorded,
not changed.**

`list_work_items`, `list_cards`, `list_links` and `list_topics` return
`{items, total, limit, offset}`. `list_reports` returns a bare array — as do
`list_proposals`, `list_projects`, `list_areas` and `list_comments`. That is
consistent across MCP, `GET /api/reports`, and `api.ts`'s `ReportRow[]`, so the
tests assert it; but the MCP server instructions tell agents that *collection
reads* return the envelope, full stop, which is not true for five of them.

The *code* is unchanged: enveloping those reads is a contract change with a UI
blast radius, and picking it silently under a coverage sweep's banner would be
the opposite of what B6 and B7 are for.

The *docs* are fixed, at ship time. Leaving a verified falsehood in the
agent-facing text while the rest of the sprint was about untested claims made no
sense. `docs/api.md` now carries a table of the four shapes actually in use
(Page envelope, `NeighborPage`, `History`, bare array) in place of "Every list
returns the same envelope" — a line that already half-admitted the problem with
a trailing "`list_proposals` is not enveloped". The MCP `server_instructions`
string names which reads are which.

**WI #579** stays open on the narrower question it is now actually about:
whether the six unpaginated reads should be enveloped for uniformity anyway.

## Front 6 — housekeeping (#550)

`web/tests/e2e/slot-schedule.spec.ts` → `card-plan-drag.spec.ts`. Slots were
removed in migration 0012; the file had tested card→daily-plan dragging for two
sprints under a dead name. The stale "seeded slot template" docstring at
`korg-mcp/tests/server.rs:5` is gone too. Historical migrations keep their slot
references — they are the record of what happened.

**The drag assertion: diagnosed and fixed, not quarantined.** The proposal
recorded it as failing on `main`. Run against a live app it *passed* — once.
Repeated serially it failed 4 runs out of 5, which is what made the cause
visible: the e2e suite runs against a persistent database, so the Backlog column
accumulates cards. `locator.dragTo` moves a real mouse and needs source and
target on screen together; once the new card sits far below the plan grid,
`scrollIntoViewIfNeeded` on the source pushes the target out of view and the
drag lands nowhere. It looked like a product bug and was not one — it was a test
that only worked on an empty board.

The spec now drives the HTML5 contract directly, sharing one `DataTransfer`
between `dragstart` and `drop`, which is exactly what `dragCard` and `dropCard`
depend on. 6 runs out of 6 on a populated board, and 340 ms instead of a 10 s
timeout.

Full suite afterwards: **46 passed, 2 flaky** (both passed on retry).
`cards-dnd.spec.ts`'s "drag a card from Backlog to Active" is the same
accumulated-board fragility, but it goes through `svelte-dnd-action` rather than
native DnD so the fix differs. Left alone and filed as **WI #580** rather than
expanded into here.

## Acceptance

| Criterion | Status |
|---|---|
| Every MCP dispatch arm exercised by a test that runs it | ✅ 44/44, fenced against the 45th |
| Fresh DB mints node id 1; existing DBs unaffected | ✅ both halves tested |
| `project_edges` and all 7 `get_node_preview` kinds covered | ✅ |
| One bootstrap implementation; the 17 copies gone | ✅ 626 lines deleted |
| `just check` green | ✅ |

142 Rust tests passing, up from 112 on `main`. e2e: 46 passing, 2 flaky, 0 failing.

## Follow-ups filed

- **WI #579** — docs corrected at ship time; what remains is whether the six
  unpaginated reads should be enveloped for uniformity.
- **WI #580** — `cards-dnd.spec.ts` is flaky on a populated board, same root
  cause as the drag test this sprint fixed.

---

## Deployed 2026-07-23

Image `korg:fb09e12cea33` (revision `fb09e12cea3386570c68f912d7b33dd88c749a49`),
built from merged `main` and deployed to kubsdb — web + REST + MCP on `:5674`.
Rollback target: `korg:ae202ca…` / image `sha256:ecd620de1ac1` (sprint 019).

Preflight: clean tree, `just check` green on merged `main`, backups current
(`korg-20260723-032356.sql.gz`, 283 KB — larger than the previous night's, timer
active).

### The migration no-opped, as designed

0015 is the first migration since 0009 to touch the node id sequence, and its
whole contract on a populated database is to do nothing. Baselined before and
diffed after:

| | Before | After |
|---|---|---|
| `schema_version` | 14 | **15** |
| `node` rows | 500 | 500 |
| `max(node.id)` | 583 | 583 |
| `min(node.id)` | 1 | 1 |
| `node_id_seq` | `last_value=583 is_called=true` | `last_value=583 is_called=true` |

The sequence is untouched, which is the assertion that mattered — a migration
that rewound it onto ids already in use would have been invisible until the next
write collided. `min(node.id) = 1` also confirms production was never affected by
the bug 0015 fixes: the import populated node 1 directly, so the unmintable-#1
case only ever existed on fresh installs.

`scripts/post-deploy-check.sh --compare`: all reads, both transports, the error
contract and the idempotent write green; **every row count identical** (work
items 385, cards 27, links 4, topics 0, proposals 57, reports 23, projects 29).

### What this sprint changed, verified live

- **`invalid_input` instead of `internal`.** `POST /api/nodes/1/comments` with a
  blank body now returns `400 {"code":"invalid_input","error":"comment body must
  not be empty"}`; `POST /api/links` with a blank url returns the matching
  `link url must not be empty`. Both previously returned `500 internal` carrying
  raw Postgres constraint text. Chosen as the smoke test because a rejected
  write exercises the fix and cannot add rows.
- **The corrected MCP server instructions** are live in the `initialize`
  response, naming which reads are paginated and which return a bare array.
- **`project_edges`** — the path with zero coverage before this sprint — agrees
  with the database exactly: `korg` 0 edges (all 23 `depends_on` edges belong to
  other projects), `homelab-ai` 21, `kmon` 1, `kvscf` 1.
- `GET /plan` returns 200.

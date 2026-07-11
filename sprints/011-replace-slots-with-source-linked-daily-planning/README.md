# Sprint 011 — source-linked daily planning

Proposal korg:361. Implements the design resolved in WI #89 through WIs #357,
#358, #359, and #360.

## Intent

Replace generated duration-based timeboxes with an ordered list of concrete
things planned for each local calendar day. Every daily item points to a work
item, card, or reusable topic and stores a display snapshot so history remains
truthful across later source renames.

Existing slot data is disposable. Every other korg entity and relationship must
remain intact.

## Model

### Topic

A `topic` is a first-class node with a required name, optional description, and
standard node metadata/archive behavior. Topics are reusable identities for
explorations that are not naturally represented by a card or work item.

### Daily plan item

A `daily_plan_item` is a node with:

- local `plan_date`
- stable order (`position`) within that date
- required source node (work item, card, or topic)
- source display snapshot captured when planned
- nullable completion timestamp
- creation timestamp

Duplicate occurrences are intentional and count independently.

## Lifecycle rules

`KORG_TIMEZONE` is a required DST-aware IANA timezone. The backend derives the
current local date and enforces all boundaries.

- Today/future: add, reorder, move, delete, check, and uncheck.
- Moving between open days transfers the item and leaves no source history.
- Past days: structure is frozen; only completion can be corrected.
- Moving a past item copies it, leaving the original unchanged.
- Late completion counts for the original `plan_date`; its timestamp records
  when the correction was entered.

## Interfaces

- Core repository operations and migrations.
- REST and MCP parity for topics, planning, completion, movement, and history.
- Today becomes the weekly daily planner.
- Cards and WIs can be dragged into days.
- Typed planning uses a topic combobox: select an existing topic or explicitly
  create one. Unlinked free text is not supported.
- Topics receive a searchable create/edit/archive management surface.
- History presets: week, month, 90 days, year. Each ends yesterday in local
  time and shows completion percentage plus the complete chronological list.
  Today is never in the denominator. No built-in analysis is added.

## Delivery order and gates

1. Migration, topic repository, and daily-plan lifecycle tests.
2. REST endpoints and API integration tests.
3. MCP tools and schema/round-trip tests.
4. Typed web client, planner/topics/history UI, and browser tests.
5. Remove obsolete slot APIs/UI/module, update docs and deployment config.
6. Final gates: Rust format/clippy/workspace tests; web lint/check/build/E2E;
   production deployment verification.

## Progress

- Backend/core complete: migration 0012, timezone-aware lifecycle context,
  topics, daily planning/history, REST, MCP, and deterministic Rust tests.
- Frontend complete: typed client, weekly planner with frozen-history copy
  semantics, card/work-item source tray, Cards-page planning drops, topic
  management, history stats/filtering, responsive navigation, and replacement
  Playwright coverage.
- Database-backed browser tests require `DATABASE_URL`; Playwright starts the
  local API with `KORG_TIMEZONE=Etc/UTC` when that environment is available.

## Deployment

Deployed to kubsdb on 2026-07-11 with image
`sha256:44676f5955463a587bc647a538482beb764ccc1025512b3bd9cbef7da7a0cfa1`.
The container runs with `KORG_TIMEZONE=America/Los_Angeles`.

Live verification passed:

- health and the `/`, `/topics`, and `/history` routes
- topic, daily-plan, and history REST reads
- MCP initialize, tool discovery, and a real `list_projects` call
- preservation of existing production projects, work items, and cards

## Explicitly out of scope

- Preserving existing slot rows.
- Topic merge.
- Multiple users or per-user timezones.
- Trends, recommendations, scoring, or automated historical analysis.

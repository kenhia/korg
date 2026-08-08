# Usage

korg unifies work items and kanban cards into one typed-node + generalized-edge
data model, served by a single `korg-api` process. Once it is running (see
[setup.md](setup.md)) you can drive it three ways: the web UI, the REST API, and
the MCP endpoint.

## Web UI

Browse to the address `korg-api` is serving (e.g. `http://<host>:8090`). The UI
covers:

- **Today** — the planning overview and the page to start from: "which sprint
  am I on" is the question it answers at a glance (the daily-plan week board it
  used to carry was removed with the slots feature in sprint 050, WI #965).
  Carries the latest daily report's status pill, the active proposals list
  (open by default) and the cards tray (collapsed by default — Ken,
  2026-07-29). At the top sits the **Awaiting you**
  lane (#969): everything an agent has marked as moving only when Ken acts,
  oldest ask first. It renders **nothing** when the lane is empty — a panel that
  shows an empty state every day is one you learn to skip, and this one has to
  be read on the days it has rows.

  Each row answers in one gesture (#981). `reply` opens an inline box; sending
  posts the text as a comment **on the node** and clears the marker, in that
  order — a failed comment leaves the ask standing rather than losing the
  answer. The comment lands where agents read it (`get_work_item`,
  `list_comments`); the lane itself stays a flag and never stores anything.
  `clear` remains for asks that need no answer, and a status-shaped decision
  needs neither: closing or finishing the item clears its marker on its own
  (D-7). Every kind the lane renders is clickable — kinds with a page of their
  own go there, the rest to the list they live on.
- **Work Items** — create, edit, archive, set parent/area, and manage
  relationships and comments. Project selection is sticky across navigation.
  Filters include tags (AND-combined, collapsed by default) and `Only Prop`,
  which narrows the list to the work items a sprint proposal *covers* — the
  `sprint` field is free text and usually empty, so the `covers` edges are the
  only honest answer to "what is in this sprint". List rows carry *tickers* —
  💬 with a count for comments, 📝 for a details section — so "there is more here
  than the title" is visible without opening the row. The footer's count is the
  whole collection, not a first page (WI #762): the list fetches every page, and
  on the rare occasion it stops short it says so in amber — `N of M loaded` —
  with a **Load all** beside it. A count with no such clause is complete.
- **Review completed** (`/work-items/review`) — the close-out page, reached by
  the `Review` button beside `Only Prop` rather than from the top nav. Lists
  **only** `done` and `resolved` items (a status-filtered server read, so an
  open item cannot appear however the filters are set), closes each with a
  single undoable button rather than a status dropdown, and shows the selected
  row's content, details *and* comments together. The details pane sits below
  the list, moving beside it once the window is wide enough.
- **Cards** — kanban cards with status, rank, tags, comments, and clickable
  launch links for URL fields.
- **Link Up** — relate any node to any other across kinds via the generalized
  `relationship` edge. **Not on the top nav** as of sprint 029 — linking is
  an agent request in practice — but `/link-up` still serves.
- **Planning** — the agent-planning queue: `sprint_proposal` nodes (a title +
  summary bundled with the work items they cover), drag-orderable by rank,
  with pin-to-top. Start/Decline/Done buttons drive the status lifecycle; a
  copy icon copies a `/start-sprint korg:<node_id>` prompt.
- **Programs** (`/programs`, `/programs/:node_id`) — the layer above Planning
  (#968). A program `includes` sprint proposals, ordered, and is where
  cross-project work is legal; a proposal itself stays single-project. The list
  shows each program's derived `span` — the repos its slices touch — since a
  program has no project of its own. The detail page renders slices in program
  order with each one's work-item rollup, from a **single** call: that read is
  what replaces walking program → proposals → work items. Deliberately minimal
  — kfdc's Operations panel is the real consumer.

  Each slice's rollup reads `work-complete / verified-by-you / total` (#980),
  where work-complete is `resolved + done + closed` — everything agents consider
  finished — and verified is `closed`, the status reserved for Ken. The two are
  separate figures because collapsing them misleads in the state that matters
  most: `N / <N / N` means all the work is done and it is waiting on Ken, which
  the old single count rendered as `0/N` and read as "not started". The bar
  encodes the same three states from the same computation, so the two cannot
  disagree. The header carries an active/holding/done control, so a program is
  finishable from the browser rather than only over REST.
- **Handoff pages** (`/handoffs/:node_id`) — the first per-node detail route
  (WI #621), and until sprint 044 the only one. A handoff's `has_handoff` ref opens the slide-over preview
  anywhere it appears; "Open full page ↗" on that preview navigates here (it is
  offered for any kind with a page of its own, so programs get it too), where
  the Markdown body gets a real reading width and a comment thread. The body is
  **read-only** — handoffs are authored through the API and the `handoff` skill
  — so a reviewer responds with a comment rather than editing the document.
  Not on the top nav; it is reached from the work the handoff belongs to, and
  links back to it.

### Behaviour common to every page

Four rules hold across the UI (sprint 019, plus one from 049):

**Nothing fails silently.** Every mutation reports failure in a toast, and a
failed *load* renders a distinguishable "couldn't load" state with a retry —
never an empty list. The distinction matters: "there is nothing here" and "korg
is unreachable" used to look identical. Error toasts stay until dismissed;
successes fade.

**Destructive actions are split by whether they can be undone.** Deleting a
comment or removing a relationship cannot be undone from the UI, so each takes
two presses — the button arms, then commits, and disarms if you click away.
Archiving a work item or card *is* reversible, so it happens immediately and
offers an Undo in the toast instead.

**Dialogs behave like dialogs.** The node preview and the card editor trap
focus, close on Escape, and return focus to whatever opened them.

**Every id an agent cites is on screen, and every id resolves** (#980, #982).
Agents name things by id — `korg:979`, `#846` — and Ken reads agent output
constantly, so matching a number against a screen is a scanning task. Rows and
detail headers across programs, cards, the reading list, daily reports,
the awaiting lane, Today's trays, and both Link Up pickers carry
their id in one shared mono-and-muted style. The reverse direction holds too:
find-by-ID on Work Items resolves **every** node kind to its real title (a work
item navigates; anything else previews), and `crates/korg-core/tests/sprint049.rs`
fails if a kind is ever added without a `get_node_preview` arm to resolve it.

## REST API

All endpoints are unauthenticated (single-user, trusted-network posture) and
rooted at `/api`. Responses are JSON.

Path parameter names below match the router exactly, because
`crates/korg-mcp/tests/docs_drift.rs` compares this table against the route
registrations in `korg-api` and fails on either a route missing here or a row
describing a route that no longer exists.

| Method(s) | Path | Description |
| --- | --- | --- |
| `GET` | `/api/health` | Liveness check. |
| `GET`, `POST` | `/api/projects` | List or create projects. |
| `GET` | `/api/projects/recent` | Most recently used project. |
| `PATCH` | `/api/projects/:name` | Update project metadata (everything but the name). |
| `GET` | `/api/projects/:name/plan` | A project's work items plus their `depends_on` edges, for plan/frontier views. |
| `GET`, `POST` | `/api/work-items` | List (`{items,total,limit,offset}`; filters `project`, `archived`, `limit`, `offset`) or create. |
| `GET` | `/api/work-items/survey` | Slim, paginated work-item projection (no content/details); filters `project`, `wi_status` (+ `all`), `archived` (tri-state since #851, excluding archived by default). Since #861 it shares the MCP `list_work_items` read, so omitting `wi_status` means everything **not terminal** rather than every status, and the response carries `omitted: {closed, archived}`. |
| `GET`, `PATCH` | `/api/work-items/:wi_number` | Fetch with inlined comments (same shape as the MCP tool), or update. |
| `GET`, `POST`, `PATCH`, `DELETE` | `/api/areas` | List, create, rename/re-describe, or delete areas (delete refuses while work items are filed under one). Selected by `{project, name}` in the body. |
| `GET`, `POST` | `/api/cards` | List cards (enveloped; filters `status`, `project`, `archived`) or create. |
| `PATCH` | `/api/cards/:node_id` | Update a card. |
| `GET`, `POST` | `/api/nodes/:node_id/comments` | List or add comments on a node of any kind. |
| `PATCH`, `DELETE` | `/api/comments/:id` | Edit or delete a comment. |
| `GET`, `POST` | `/api/links` | List links (enveloped; filters `disposition`, `read`, `archived`) or create. |
| `PATCH`, `DELETE` | `/api/links/:node_id` | Update a link: disposition, read flag, tags and archived in one transaction. `DELETE` hard-deletes an unreferenced capture and refuses (`409`) otherwise — see [disposal semantics](api.md#disposal-semantics-wi-855). |
| `POST` | `/api/relationships` | Create a generalized relationship. |
| `DELETE` | `/api/relationships/:id` | Delete a relationship. |
| `GET` | `/api/nodes/:id` | Kind-agnostic preview of any node by id (powers find-by-ID + the preview panel); 404 if none. |
| `GET` | `/api/nodes/:id/neighbors` | A node's edges: `{items,total,limit,truncated}`, optional `label`/`kind`/`limit` (see [api.md](api.md#relationships)). |
| `GET`, `POST` | `/api/proposals` | List sprint proposals (filters `status`, `project`), or propose one: project + title + summary + covered `work_item_numbers` in a single call. The project is required, and a `work_item_numbers` entry from another project is `invalid_input`. |
| `GET` | `/api/proposals/rollup` | Per-project planning weather for the Planning rail: `proposals` (live), `wi_in_proposal` and `wi_total` (both live + unarchived), plus the project's `status`. Every project, including one with three zeroes. |
| `GET`, `PATCH` | `/api/proposals/:node_id` | Proposal detail (covered work items + comments), or update status/rank/pinned/archived. |
| `GET`, `POST` | `/api/programs` | List programs (`{items, omitted}`, live by default), or create one. A program takes **no** project — it is the cross-project layer, and `span` is derived from its slices. |
| `GET`, `PATCH` | `/api/programs/:node_id` | Program detail — ordered slices with per-slice work-item rollups — or update title/aim/notes/status/rank/pinned/archived. |
| `GET`, `POST` | `/api/schedules` | List schedules (`{items, omitted}`, live by default, soonest-due first; filters `status`, `project`, `due_only`), or create one. |
| `GET`, `PATCH` | `/api/schedules/:node_id` | Schedule detail — due-ness, `preview_title`, and every work item it has materialised — or update template/cadence/anchor/status. |
| `POST` | `/api/schedules/:node_id/materialize` | Turn a due schedule into a work item. `?force=true` brings a not-yet-due one forward; it never lifts the outstanding-item refusal. |
| `GET` | `/api/report-sources` | Every reporting source with its `freshness` and what it `asserts`. Anything not `fresh` asserts `unknown`, never a last known status. |
| `PATCH` | `/api/report-sources/:source` | Declare a source's cadence, grace window, retirement or note. Every field is an override; `null` returns it to derivation. |
| `GET` | `/api/awaiting` | Everything waiting on Ken, oldest ask first, across every node kind. |
| `PUT` | `/api/nodes/:id/awaiting` | Set or clear the awaiting-Ken marker on any node (`{awaiting, note}`). The UI's one-click clear is this call with `awaiting: false`. |
| `GET` | `/api/board` | The whole board in one request: active sprints with progress, the ranked queue, what is blocked and by what, programs with slices, the awaiting lane, per-project depth, newest reports, the status-transition ticker, and every reporting source's freshness. No parameters — see [api.md](api.md#the-board-rollup-970). |
| `GET` | `/api/reports` | List agent reports (filters `source`, `limit`; newest first). |
| `GET` | `/api/reports/:node_id` | One report with its findings and comments. |
| `POST` | `/api/handoffs` | Create a handoff and attach it to the nodes it describes (`has_handoff` edges) in one call. |
| `GET`, `PATCH` | `/api/handoffs/:node_id` | One handoff (Markdown body + the nodes it is attached to), or update title/summary/body/tags/archived. |

Example:

```bash
curl -s http://localhost:8090/api/work-items | jq
```

See [api.md](api.md) for the normative contracts: the collection-read
envelope and filters, the two-level read contract, and the relationship label
registry with its direction semantics.

### Response and error contract

Two rules hold across REST and MCP alike (WI #524, #525):

**Mutations validate, acknowledge, and return the entity.** Every create and
update checks that its target exists *and is the right kind*, then returns the
full row a read would return — no bare `{"ok": true}`. Deletes return
`{"deleted": true|false}`, so "nothing to delete" is distinguishable from
"deleted". A missing or wrong-kind target is a 404: `PATCH /api/cards/<id>`
where `<id>` is a work item's node fails and changes nothing, rather than
archiving the work item and reporting success.

**Errors are typed.** Error bodies are
`{"error": "<message>", "code": "<code>"}`, where `code` is stable and
machine-readable; MCP tool errors carry the same pair as
`{"message", "code"}` in an `isError` result.

| `code` | HTTP | Meaning |
| ------ | ---- | ------- |
| `invalid_input` | 400 | A value the caller supplied is not acceptable — unknown status, t-shirt size, `wi_type`, card status, disposition, unparseable date, a blank value where the schema says `minLength: 1` (comment body, link URL), an area outside the work item's project, an unresolvable parent, or a selector that does not resolve (unknown `project`/`project_id`/`area`/`area_id`, or an id *and* a name passed together). |
| `not_found` | 404 | The named or keyed entity does not exist — including single-item reads, which 404 rather than answering `200 null`. |
| `conflict` | 409 | Well-formed but at odds with server-enforced state (frozen past, stale reorder). |
| `internal` | 500 | A genuine server fault. Only this class should ever be retried blindly. |

Blank required text is checked in korg-core too (sprint 020). The `CHECK`
constraints on `comment.body` and `link.url` always rejected it, but the failure
surfaced as an `sqlx` error — so a caller who sent an empty string was told
`internal` and shown the raw constraint violation, i.e. "korg has a problem,
retry", for input that would never be accepted.

Vocabularies are validated in korg-core, so an unknown value comes back as a
400 naming the whole allowed set rather than a 500 carrying raw Postgres text:

- `wi_status`: `open`, `resolved`, `done`, `closed`
- `wi_type`: `task`, `bug`, `chore`, `feature`, `research`, `tweak`, `brainstorm`, `maintenance`
- `wi_tshirt`: `XS`, `S`, `M`, `L`, `XL`, `Huge`, `Unknown`
- card `status`: `Backlog`, `Research`, `OnDeck`, `Active`, `Done`, `Cut`
- link `disposition`: `Unread`, `Done`, `Revisit`, `Summarized`, `VaultSaved`
- proposal `status`: `proposed`, `active`, `done`, `declined`
- program `status`: `active`, `holding`, `done`
- report `status`: `ok`, `attention`, `problem`
- project `status`: `active`, `archived`
- project `category`: `AI`, `Dashboard`, `EVAL`, `Fun`, `Infrastructure`, `Ops`, `Other`
- schedule `cadence`: `once`, `weekly`, `monthly`, `quarterly`, `yearly`
- schedule `anchor_mode`: `completed`, `created`
- schedule `status`: `active`, `paused`, `done`
- schedule substitutions: `YEAR`, `MONTH`, `DAY`, `DATE`, `QUARTER`
- source `freshness`: `fresh`, `stale`, `retired`, `unrated`
- source `asserts`: `ok`, `attention`, `problem`, `unknown`

## MCP endpoint

The MCP server is mounted inside `korg-api` at `POST /mcp` using rmcp's
Streamable-HTTP transport in **stateless JSON mode** — each POST is an
independent JSON-RPC request/response, so no SSE session needs to be managed.
Host validation is disabled, matching the REST API's no-auth posture on a
trusted network.

Point any MCP client at:

```
http://<host>:8090/mcp
```

The full tool list, by category, is the
[tool catalogue](api.md#tool-catalogue) in api.md — the one place it is written
down, drift-tested against the code.

List the available tools with a raw request:

```bash
curl -s -X POST http://localhost:8090/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'mcp-protocol-version: 2025-06-18' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | jq
```

## Work-item status lifecycle

Statuses are validated server-side (WI #285): `open`, `resolved`, `done`,
`closed` — anything else is rejected on create/update.

| Status | Set by | Meaning | Default list visibility |
|--------|--------|---------|------------------------|
| `open` | anyone | not started / in progress | visible |
| `resolved` | agent (or Ken) | implemented; may still need a user test or PR | visible |
| `done` | agent (or Ken) | agent satisfied with the implementation | visible |
| `closed` | **Ken only** (or at his direction) | out of sight unless searched for | hidden (filter unchecked) |

Typical flows: `open → resolved → done` (agent lifecycle), `→ closed` when
Ken sweeps; straight `open → done` for small verified agent work.

## Project metadata

Projects carry lifecycle metadata (WI #246): `status` (`active | archived`),
`machines` (which hosts the working copy lives on), `src_path` (where on that
host), `deploy_to` (where it deploys), `category`, `description` and `notes`.
The Work Items rail shows only active projects unless "show all" is checked,
and the ✎ control opens an edit panel (everything editable except the name).
Agents: `update_project` MCP tool / `PATCH /api/projects/:name`.

`status` was four values until WI #828 narrowed it to two. `maintenance` and
`inactive` both held **zero rows** and neither had semantics distinct from
`archived`, and an unused vocabulary value is a standing invitation to invent a
meaning nobody agreed. The status answers one question — *is this a legitimate
target for new work?* — and that has two answers. Since sprint 038 the server
answers it too: creating in, or moving into, an archived project is
`invalid_input` naming the status, rather than a silent mis-file (WI #884). Where `archived` covers both
*superseded* (`kwi`/`kcard` → korg) and *dormant* (`trt-llm-explore`), the
difference lives in the `description` prose, because both filter identically.

**`description` is a routing contract, capped at 160 characters.** One line,
first clause task-shaped ("harness conventions and sprint layout", not "a
repository containing…"), plus a boundary where a sibling project plausibly
claims the same work: "Not X — that's ‹project›." Written for an agent with no
prior context deciding where a work item goes. Longer prose — deploy topology,
build commands, house conventions — goes in **`notes`**, which is unbounded and
returned only by `get_project` and REST, never by the lean `list_projects` or
the instructions roster.

`category` is a **closed vocabulary** (WI #678):
`AI | Dashboard | EVAL | Fun | Infrastructure | Ops | Other`. It was free text
until migration 0018 and had drifted to `ai`/`AI`/`tooling`/`infra`/`fun`/NULL;
`update_project` now validates it the way every other vocabulary is validated,
and the edit panel offers a select rather than a text box. `null` is still
legal — `create_project` takes only a name, so a project starts uncategorised.

The rail **colors each project by its category**, replacing a hash of the
project name that put 20 of 31 projects within 12 degrees of another. Only
active projects are colored; archived and uncategorised ones render with no
color, which doubles as a signal that a project is not live. A **by category** checkbox groups the rail under category
headings; the default stays alphabetical. Adding a category means one entry in
korg-core's `PROJECT_CATEGORIES` plus its hue in `CATEGORY_HUE`
(`web/src/lib/domain.ts`) — the map is typed over the generated vocabulary, so
the build fails until the hue exists.

**Writes take a project by name or by id** (WI #575): pass either `project`
(the name) or `project_id`, never both — and likewise `area` or `area_id`.
Names are resolved, never created, and an unknown one is a 400 that names
`list_projects` as the remedy. Full rules in
[api.md](api.md#selecting-a-project-or-an-area).

### Reading projects: three tiers, three questions (WI #828)

Routing a work item used to be done from agent memory rather than from korg —
`list_projects` returned every column of every row, so every consumer paid the
widest question regardless of the narrow one it asked.

| Tier | Surface | Question |
|---|---|---|
| 1 | the MCP `instructions` roster | "what projects exist" |
| 2 | `list_projects`, lean default | "does this belong here?" |
| 3 | `get_project(name)` | "tell me everything about this one" |

```
list_projects(status?: "active"|"archived"|"all", detail?: "lean"|"full")
  -> {items, omitted: {archived: N}}
```

Defaults are **active only, lean**: `name`, `description`, and `status` only
when it is not `active`. Roughly a 5× smaller payload than the old shape, and
the saving is the field projection rather than the row filter.

It returns an envelope rather than a bare array — a deliberate exception to the
convention in [api.md](api.md#collection-read-shapes) — because `omitted` is
what stops an agent concluding "there is no such project" from "you did not ask
for archived ones". `detail:"full"` restores every column including `notes` and
the `created`/`updated` timestamps, so a maintenance pass makes one call rather
than one per project.

Those timestamps arrived late (WI #905). The columns have existed since 0001
and migration 0013 has advanced `updated` on every write since #529, but
`ProjectRow` never selected them — so the two reads that promise "every
column" returned ten of twelve, and projects were the last kind whose recency
an agent could not read. They stay out of the lean tier: a timestamp answers
*when*, not *does this belong here?*.

**`list_proposals` joined this shape in WI #852**, for the same reason and with
the same two knobs. Over MCP it defaults to the live queue (`proposed` +
`active`, unarchived) and a lean projection without `summary`; `status:"all"`
and `detail:"full"` are the escape hatches, and `omitted` is `{done, declined,
archived}`. It measured ~46k tokens unfiltered, 71% of it `done` proposals no
caller had ever wanted. `GET /api/proposals` is unchanged — the Planning page
renders those summaries.

**`list_work_items` joined it in WI #861**, with the same two ideas and one more:
the lean projection is now the *only* MCP projection (the full tier is
`get_work_item`), and the row filter is `wi_status`, which defaults to everything
not terminal. `omitted` is `{closed, archived}`. `survey_work_items` was a
deprecated alias for it and was deleted in sprint 038 (WI #871) once its last
caller moved; `GET /api/work-items` still returns full rows, because the Work
Items page searches `content` in the browser.

**The object shape changed underneath both proposals reads in WI #860**:
`summary` is a ≤500-character routing contract (CHECK-enforced by migration
0021, `invalid_input` above it) and the analysis lives in `notes`, unbounded and
returned by `get_proposal`, `detail:"full"` and REST. The migration moved every
over-cap summary verbatim rather than truncating it — see `docs/api.md`.

Tier 1 is rendered from live data at `initialize`, so every new agent session
gets a current roster with no restart: korg is a long-running HTTP server that
can go weeks between deploys, and anything baked at process start goes
arbitrarily stale. It lists **names only** — anything in `instructions` is paid
by 100% of sessions, while descriptions only earn their tokens in a routing
session. It fails open: if the query errors, the instructions ship without a
roster rather than failing `initialize`.

## Comments

Comments are editable (WI #232): ✎ in the UI, `update_comment` MCP tool,
`PATCH /api/comments/:id`. `created` is preserved; `updated` advances.

## Reports

A **report** is one automated agent's writeup for one day: `source` (the
reporter's id, e.g. `kmon`), `report_date`, a `status` of `ok | attention |
problem`, a one-line `summary` for the list view, a markdown `body`, an optional
`model`, an `escalated` flag, and a set of **findings** — work items the run
turned up, linked by the `finding` edge (see the
[label registry](api.md#label-registry)).

`create_report` is an **upsert keyed on `(source, report_date)`**. A re-run the
same day replaces the body *and* the finding set — findings you omit are
unlinked — while keeping the same `node_id`, so comments and any links made to
the report survive the re-run (D-7). The response echoes `findings_linked`:
`wi_number`s that did not resolve are dropped silently, so compare it against
what you asked for.

Reads are symmetric across transports: `list_reports` / `GET /api/reports`
(newest first, summary fields only, optional `source` filter) and `get_report` /
`GET /api/reports/:node_id` (full body plus the linked finding work items).

Writing is MCP-only by design — there is no `POST /api/reports`. Reports are
written by agents, which speak MCP; the REST side exists so the UI and `curl`
can read them.

## Data model in brief

Everything is a **node** sharing one surrogate id space, so any kind can link to
any other through a single generalized `relationship` edge:

- **work item** — keeps a stable, user-facing serial `wi_number` (referenced by
  external project docs) that is *not* the primary key column, though since the
  0009 identity migration it holds the **same value** as the node id. Both
  spellings therefore address the same row, and `get_work_item` accepts either
  (#966) — passing both is `invalid_input`, not a guess.
- **card** — kanban card (status, rank, tags).
- **link** — reading-list URL.
- **sprint_proposal** — an agent-planning proposal (title, a ≤500-char `summary`
  routing contract, unbounded `notes` for the analysis, status, drag-orderable
  `rank`, `pinned`); covers work items via the same `relationship` mechanism,
  label `covers`, rather than a dedicated join table. **Single-project** since
  sprint 043: a project is required, and every covered work item is in it —
  cross-project work is bundled by a program, the layer above proposals.

Cross-cutting attributes (project, category, tags, archived, timestamps) live on
`node`. Projects are a unified taxonomy; areas stay project-scoped; tags and
category are shared across kinds.

## Importing existing kwi / kcard data

If you are migrating from the legacy `kwi` and `kcard` tools, see the
[migration guide](migration.md).

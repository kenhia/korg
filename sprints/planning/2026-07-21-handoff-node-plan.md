# Handoff node plan

Date: 2026-07-21
Status: planned; begin after the korg deep review and resulting cleanup

## Context

Handoffs are currently passed through session text, repository files, or
occasionally klams. Files are simple and local. Klams makes a handoff durable
and cross-machine, but neither approach makes it part of korg's work graph. A
handoff cannot currently be attached visibly to a work item or sprint proposal,
discovered from its normal read path, or opened in the korg UI.

The storage problem is small. The retrieval contract is the important part.
Before Sprint 012, decisions in work-item comments were easy for agents to miss
because `get_work_item` did not return or signal them. Sprint 012 fixed that with
a two-level contract:

- collection views include `comment_count`, signaling that more context exists;
- the focused `get_work_item` read inlines comments up to a cap and reports
  `comments_truncated` when a follow-up call is required.

A handoff feature must not reintroduce the same invisible-context failure. An
agent following the primary read path for a work item or sprint proposal must at
least discover every related handoff without having to remember that an
unprompted `neighbors` call might reveal important context.

## Decision

Add a first-class `handoff` node and a conventional handoff skill.

These are complementary:

- the node provides durable, cross-machine, cross-agent storage and generalized
  links to the work it describes;
- the skill provides a consistent authoring template, creates and links the node
  atomically, and teaches sending and receiving agents how to use it;
- a repository file remains an explicit fallback when korg is unavailable.

Do not model handoffs as work items. A handoff is an artifact describing work or
a contract, not independently schedulable work. A `handoff` work-item type would
leak into backlog, sizing, status, survey, planning, and queue-refill semantics,
then require special-case filtering throughout those surfaces.

## Goals

1. Store a concise but complete handoff document in korg.
2. Relate a handoff to any relevant korg node, especially work items and sprint
   proposals.
3. Make handoff existence conspicuous in collection responses.
4. Return useful handoff context from focused work-item and proposal reads.
5. Give agents one atomic operation that cannot create an orphaned handoff by
   forgetting the relationship step.
6. Provide a focused web viewer for human review.
7. Keep handoffs portable across machines, repositories, sessions, and agents.

## Non-goals

- Replace work items, sprint proposals, comments, or klams.
- Add a workflow/status lifecycle to handoffs in the first version.
- Track producing and consuming model/session identities as required schema.
- Turn korg into a general document store or repository-file mirror.
- Automatically infer that arbitrary relationships carry required context.
- Build this before the planned deep review and cleanup of korg. That review may
  change API conventions, read projections, relationship semantics, or UI
  boundaries; update this plan against the resulting architecture first.

## Domain model

Add `handoff` to `node.kind` and add one detail table following the existing
typed-node pattern:

```text
handoff
  node_id   bigint primary key references node(id) on delete cascade
  title     text not null
  summary   text not null
  body      text not null
```

Use shared `node` metadata for project, category, tags, archived state, and
timestamps. Keep the body as Markdown. `summary` must be short enough for compact
references and UI lists; `body` carries the full state.

Do not add dedicated foreign keys to work items or proposals. Use generalized
relationships so the same handoff can describe a proposal, several work items, a
report, another handoff, or a cross-project contract.

Adopt one canonical relationship label, provisionally `has_handoff`, with the
subject node on the left and the handoff on the right. Confirm direction and
canonicalization against the relationship implementation during cleanup. Handoff
retrieval must work from either side and must not rely on callers knowing storage
orientation.

## Read contract

Apply the Sprint 012 rule: collection reads signal context; focused reads return
it.

### Compact reference

Return useful metadata rather than only a bare node ID:

```json
{
  "node_id": 431,
  "title": "Generator output contract",
  "summary": "JSON schema and compatibility expectations",
  "updated": "2026-07-22T12:00:00Z"
}
```

The exact JSON should follow conventions established by the deep review, but a
reference must tell both a person and an agent why it matters.

### Work-item reads

- `survey_work_items`: include `handoff_count` only.
- `list_work_items`: include `handoff_count`; add compact references only if
  payload measurements show that they remain cheap.
- `get_work_item`: return `handoff_count`, `handoffs`, and
  `handoffs_truncated`. Prefer complete documents up to a small cap; compact
  references are acceptable only if tool descriptions and skills require fetching
  each document before acting.

`handoff_count` is the true total. `handoffs_truncated` must never be false when
only part of that total is returned.

### Sprint-proposal reads

Proposals currently have a list operation but no authoritative detail operation.
Add `get_proposal(node_id)` as the primary read for starting or reviewing a
sprint. It should return:

- proposal fields;
- covered work-item references;
- `handoff_count`, `handoffs`, and `handoffs_truncated`;
- enough context signaling for covered work items that an agent cannot silently
  miss required state.

`list_proposals` should include at least `handoff_count`. The `start-sprint` skill
should move from manually composing `list_proposals`, `neighbors`, and
`list_work_items` to the focused detail read.

### Handoff reads

Add `get_handoff(node_id)`, returning node metadata, title, summary, Markdown
body, and related-node references. Add a bounded list operation only if a
demonstrated UI or agent workflow needs discovery independent of a related node;
relationship-based discovery is the primary path.

## Write contract

Provide an atomic operation such as:

```text
create_handoff(title, summary, body, related_node_ids, project_id?, category?, tags?)
```

Creation and all requested relationships belong in one transaction. Reject an
empty `related_node_ids` list by default. An intentionally standalone handoff
should require an explicit opt-in rather than occur because an agent forgot the
linking step.

Provide update and archive operations. Relationship changes can continue through
the generalized `relate`/`unrelate` surface unless real workflows justify a more
focused operation.

Mirror intentional MCP/REST semantics. Provisional surfaces, subject to the deep
review's API recommendations:

- MCP: `create_handoff`, `get_handoff`, `update_handoff`;
- REST: `POST /api/handoffs`, `GET/PATCH /api/handoffs/:node_id`;
- focused work-item and proposal responses updated as described above.

## Handoff skill

Create a `handoff` skill that uses korg MCP tools and handles both sending and
receiving work. A handoff should capture:

- purpose and current state;
- scope and relevant korg/repository references;
- decisions already made and their rationale;
- interfaces, schemas, or behavioral contracts;
- changes made and validation performed;
- unresolved questions, risks, and known dead ends;
- concrete next actions and completion criteria.

The skill should:

1. Identify the work items, sprint proposal, or other nodes that own the work.
2. Create the handoff and relationships in one MCP call.
3. Return a durable `korg:<node_id>` reference and summarize attachments.
4. On receipt, use the owning node's focused read, then fetch every handoff not
   already inlined before implementation begins.
5. Treat truncation flags as mandatory follow-up work.
6. Fall back to an explicitly named repository Markdown file when korg is
   unavailable, reporting that the handoff is local-only and not linked in korg.

Update `start-sprint` to consume proposal handoffs. Update relevant agent
instructions and MCP tool descriptions to say that handoffs are required context,
not optional related reading.

## Web UI

Add a handoff viewer that renders title, summary, Markdown body, metadata, and
related nodes. Integrate entry points into existing context surfaces rather than
making users discover a separate document silo:

- work-item detail shows related handoffs conspicuously;
- proposal cards/detail show handoff count and open the viewer;
- node preview understands the `handoff` kind;
- generalized relationship views can open a handoff like any other node.

A dedicated handoff index is optional and should be driven by an observed need.
The first version's primary UX is "see the handoff from the work it belongs to."
Follow UI architecture and design recommendations from the deep review rather
than freezing current component patterns into this plan.

## Migration and compatibility

1. Add a migration widening the `node.kind` check and creating the handoff detail
   table and any proven indexes.
2. Preserve all current node IDs and relationship semantics.
3. Do not automatically migrate klams entries or arbitrary local handoff files.
4. Manually migrate only useful living handoffs, with source references where
   appropriate.
5. Update node preview dispatch so missing detail rows fail legibly instead of
   appearing as generic nodes.

## Tests and acceptance criteria

### Core

- Create a handoff and several relationships atomically.
- Reject missing related nodes and verify no partial insert remains.
- Update, archive, and preview a handoff.
- Retrieve handoffs from both sides of a relationship regardless of stored
  orientation.
- Keep count/truncation semantics exact past the inline cap.

### MCP and REST

- Create and fetch a complete handoff.
- Work-item survey/list/detail responses obey their respective
  count/reference/body contracts.
- Proposal list/detail responses obey the same contract and include covered WIs.
- Errors and null/not-found behavior match reviewed API conventions.
- MCP and REST represent the same domain semantics, with intentional transport
  differences documented.
- Response-size tests cover several large handoffs and capped collections.

### Skills

- Creating a handoff leaves no unlinked document.
- `start-sprint` discovers and reads proposal and covered-WI handoffs.
- Truncated responses trigger follow-up reads.
- File fallback is explicit and does not claim cross-machine durability.

### Web

- Related handoffs are visible from work-item and proposal flows.
- Markdown, long unbroken content, missing metadata, and multiple handoffs render
  without overlap at desktop and mobile widths.
- Keyboard navigation, focus, and accessible names are verified.
- Browser tests cover opening a handoff from each owning context.

### End-to-end

An agent given only a work-item number or sprint-proposal ID can follow the
documented primary read path and cannot unknowingly miss that a related handoff
exists. It can retrieve the full handoff, detect truncation, and continue the
work on another machine without access to the original session or repository
file.

## Implementation sequence

1. Complete independent deep reviews and consolidate them into a cleanup plan.
2. Ship relevant response, API, relationship, documentation, and UI cleanup.
3. Reconcile this plan with the resulting architecture and name changed
   assumptions explicitly.
4. Implement the database, core model, and focused read projections.
5. Add MCP/REST writes and reads, then contract tests.
6. Build the viewer and owning-context entry points.
7. Add/update the handoff and `start-sprint` skills plus tool descriptions.
8. Deploy and verify the acceptance path against the live korg instance.

## Questions after the deep review

- Should focused reads inline complete bodies or compact references plus mandatory
  follow-up reads? Decide using measured response sizes and agent failure risk.
- What cap and ordering should apply when several handoffs are attached?
- Should a handoff be mutable in place, or should substantial revisions create a
  successor relationship for history?
- Is `has_handoff` the right label, and does relationship storage preserve the
  direction its public API claims?
- Does proposal detail belong in a generic node-detail abstraction or remain a
  domain-specific operation?
- Should standalone handoffs ever be allowed, and what workflow needs them?
- Should completed handoffs be archived manually, inherit owning-work state, or
  remain permanently active artifacts?
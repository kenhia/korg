# Sprint 017 — name-keyed selectors for agent writes

Proposal `korg:576`. Covers WI #575, plus the `area_id` half Ken added when
starting the sprint. A deliberate wedge between B4 (sprint 016) and B5, sized to
one WI: it is a contract change, and B5's whole job is documenting the contract,
so it had to land first or B5's docs sweep would be stale on arrival.

## The bug this closes

While filing WI #574 at the end of sprint 016, an agent — me — passed
`project_id: 1` meaning korg. It succeeded. The work item landed in `kwi`, an
archived project, and the response said nothing was wrong.

That is a **silent wrong write**, and it is the same failure class the whole
2026-07 cleanup has been closing: `update_card` mutating a work item and
reporting success (WI #525), `update_work_item` clearing a parent it could not
resolve (F-06), `relate` surfacing a typo'd node id as a raw 500 (WI #524). The
common shape is an operation that accepts a plausible-but-wrong identifier and
does something confident with it.

It was also an inconsistency nobody had named. Every *read* filter in korg
already speaks project **names** — `list_work_items(project)`,
`list_cards(project)`, `list_proposals(project)`, `list_areas(project)`,
`update_project(name)` — and every response row carries `project` as a name.
Only the create/update verbs demanded a bare integer, and those are precisely
where being wrong costs the most.

## What changed

`project` (a name) is accepted anywhere `project_id` is, and `area` anywhere
`area_id` is, across `NewWorkItem`, `WorkItemPatch`, `NewCard`, `CardPatch`,
`NewLink`, `NewTopic` and `NewProposal` — so `create_work_item`,
`update_work_item`, `create_card`, `update_card`, `create_link`, `create_topic`
and `propose_sprint`, on both transports.

Resolution lives in `korg-core`, next to `require_kind`/`require_node`, which is
why both transports behave identically without either of them knowing the
feature exists.

Three rules, each with a reason:

**Never both.** `project_id` + `project` together is `invalid_input`, *even when
they agree*. The tempting alternative — "id wins" — silently discards one of two
things the caller explicitly asked for, which is the failure mode this sprint
exists to remove. Refusing to guess is the whole point.

**Resolve, never create.** An unknown name errors. This is the part most likely
to get reverted by accident later, so it is worth stating plainly: sprint 015
(WI #537) removed project-*name* acceptance from `update_card` because it
**created** the project as a side effect of a card edit — a hidden write inside
an update. That removal stands. This change is resolve-**or-fail**, the opposite
in the dimension that mattered, and
`crates/korg-core/tests/selectors.rs::an_unknown_project_name_is_actionable_and_writes_nothing`
fences it by asserting the project count is unchanged after a failed resolve.

**Errors name the remedy.** An unresolvable name points at `list_projects` /
`list_areas`. This is korg's existing idiom — `vocab::validate` already puts the
full allowed set in the message on the principle that the error doubles as the
documentation an agent needs to retry — adapted for a set too large to inline.

### The one suggestion, and the one deliberately omitted

A name that differs only in **case** gets answered with the real one: `KORG` →
*"no project named 'KORG' — did you mean 'korg'?"*. That is the realistic
near-miss, it is unambiguous, and it costs one query.

There is **no fuzzy matching**. A confidently wrong "did you mean…" would invite
exactly the misfile this sprint is about, and `list_projects` is one call away.
The WI marked it a nice-to-have; on reflection it is a mis-feature.

Case is *suggested*, never *accepted* — silently normalising `KORG` to `korg`
would be its own surprise, and two projects differing only in case would become
ambiguous.

### Areas resolve in the project the row ends up in

Area names are unique only within a project (`UNIQUE (project_id, name)`), so
`area` has to resolve against the project the work item will have **after** the
update, not the one it had before. Moving and re-tagging in a single call is
the case that makes this visible, and it is tested directly: two projects each
own an area called `ui`, and a move-plus-retag has to land on the *destination*
project's one.

Consequently the area name resolves **inside** the transaction, where the
effective project is already computed, while the project name resolves before it
(like `parent` does) so an unresolvable name changes nothing.

An `area` with no project at all is `invalid_input` naming that specific
problem, rather than a lookup that mysteriously finds nothing.

## Two error codes changed

Both are deliberate, and neither had test coverage before — which is how the
inconsistency survived:

- **Unknown `project_id`** was a foreign-key violation surfaced as a raw
  Postgres error in a **500**. It is now `invalid_input`. This is the same shape
  WI #524 fixed for `relate`'s endpoints; nobody had checked the project column.
- **Unknown `area_id`** claimed **`not_found`**, which reads as "the work item
  you addressed is missing" when in fact the work item is fine and an argument
  is wrong. It is now `invalid_input`.

That gives one statable rule where there were two conventions:
**the operation's own target missing is `not_found`; a selector that does not
resolve is `invalid_input`.** Documented in `docs/api.md`.

## What sprint 016 bought

Adding `project` to a struct took one edit per struct and reached the REST body,
the MCP tool schema, and the generated TypeScript with **zero** hand-edits —
`just gen` produced 63 lines of schema and the client types followed. Before B4
this same change would have been five hand-synchronised copies per struct, and
the odds of all seven tools ending up consistent would have been poor.

The one thing generation could not do was the prose: the derived `description`
comes from the Rust doc comment, so the doc comments had to be rewritten as
single lines — a wrapped `///` becomes a description with literal newlines in
it. Worth knowing before the next struct gains a field.

## A gate fix, found by using it

`just check`'s freshness step was `just gen && git diff --exit-code`. That is
the standard CI idiom and it works on a clean checkout — but it also fails on a
working tree whose generated files have been legitimately regenerated and not
yet committed, which is the normal state halfway through a sprint. It failed the
first time this sprint ran it, on correct output.

It now hashes the generated files before and after `just gen` and asserts
regeneration changes nothing, which is the property actually wanted and is
independent of what git happens to know. Verified in both directions: green on
the real tree, and red when a deliberate edit was injected into `korg.ts`.

## No UI change

The web app holds real ids (it renders the project rail from `list_projects`),
so nothing in `web/` needed to change. Notably the cards page still does
`createProject(name)` *then* patches with the id — that explicit two-step is
WI #537's fix for the silent-creation bug, and name resolution does not replace
it, because resolve-or-fail cannot create the new project the user just typed.

## Verified

- `just check` green: fmt, `just gen` leaves the tree clean, `clippy -D
  warnings`, full suite.
- **9 new core tests** (`crates/korg-core/tests/selectors.rs`) covering every
  write by name, patch-by-name plus `null`-unassigns, the unknown-name error
  and its no-side-effect fence, the case suggestion, both-selectors conflict,
  unknown `project_id`, unknown `area_id`, and area-resolves-in-the-new-project.
- **Both transports fenced**: a REST test in `contract.rs` (status *and* `code`
  together, per D-5) and an MCP test in `server.rs` asserting the `isError`
  body carries `invalid_input` and names `list_projects`.
- `pnpm check` / `lint` clean; Playwright unchanged.
- No production access during development.

## Notes for what follows

- **B5** now has a frozen surface to document, which was the reason for the
  sequencing. Its docs sweep should pick up the new `docs/api.md` section on
  selectors.
- The uniform selector rule (`not_found` = target, `invalid_input` = argument)
  is worth applying to the remaining id arguments when B7 sweeps coverage —
  `parent` already follows it, `source_node_id` on the daily-plan verbs has not
  been checked.

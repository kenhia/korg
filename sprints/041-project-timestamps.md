# 041 — Close the loop: project timestamps join "every column"

Proposal korg:906 (rank 4.9). Covers one work item: **#905** (XS, bug).

## Goal

Make the two reads that promise "every column" keep the promise. `get_project`
and `list_projects detail:"full"` returned ten of twelve project columns —
`created` and `updated` were never in `ProjectRow`'s SELECT, so projects were
the last kind whose recency an agent could not read.

This is the last turn of the agent-surface loop before plan §5's drop to
low-rank continual improvement, sized to the one gap 040's measurement re-run
found.

## What shipped

- `ProjectRow` gains `created`/`updated` (`OffsetDateTime`, rfc3339 on the
  wire, `string` in the ts-rs bindings — the same shape every other row uses),
  and `PROJECT_SELECT` selects them. One struct, one query string; every
  project read flows through both, so `get_project`, `get_project_detail`,
  `list_projects detail:"full"`, `update_project`'s return value and
  `GET /api/projects` all pick it up at once.
- The two "every column" descriptions are now true, and say what the fields
  are for: `get_project` names `created`/`updated` as "how stale this
  project's routing metadata is", and `list_projects`' enumeration includes
  them.
- `just gen` — ts-rs bindings and the MCP schema snapshot.
- `docs/usage.md`'s three-tier section records why the timestamps arrived late
  and why they stay out of the lean tier.
- Tests: [`crates/korg-core/tests/sprint041.rs`](../crates/korg-core/tests/sprint041.rs),
  two of them. Written failing first — the first run was eight `no field
  created on type ProjectRow` errors.

## Decisions

- **The lean routing row stays lean.** #828's tiering rule is that the default
  view carries only what answers *does this belong here?*; a timestamp answers
  *when*. The second test asserts the absence on serialized JSON rather than
  leaving it to the struct definition, so the tier boundary is pinned rather
  than incidental.
- **No migration.** The columns have existed since 0001 and migration 0013
  (#529) has attached `touch_updated()` to `project` since then. This is
  purely a projection fix — 0013's own header called the shot in writing.
- **The test asserts the trigger, not just the fields.** `updated > created`
  after an edit is the claim that matters; two fields that always equal each
  other would satisfy the schema and none of the intent.
- **Descriptions gained a reason, not just a field name.** "including `notes`
  and `created`/`updated`" would have been true; saying what the timestamps
  tell you is what makes an agent reach for them.

## Follow-ups

On deploy, the overseer re-runs plan §5's two exit measurements. Measurement 2
is now one `get_project` call plus a check that no new surface landed
meanwhile. Both green → the program drops to low-rank continual improvement.

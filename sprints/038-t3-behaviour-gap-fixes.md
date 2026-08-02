# 038 — T3 behaviour-gap fixes: honest totals, honest writes

Proposal: korg:892 (rank 4.6, pinned). The fix sprint over sprint 037's
re-review findings, plus the alias deletion #867 un-gated.

## Goal

Close the seven WIs 892 covers, each with a regression test that pins the
behaviour rather than the bug:

| WI | size | what |
| --- | --- | --- |
| #883 | S | paginated `total` collapses to 0 when `offset` overshoots |
| #884 | S | archived projects silently accept new work |
| #885 | M | `updated` frozen on ordinary WI/card/proposal edits |
| #886 | S | daily-plan past-day freeze "unenforced" |
| #887 | XS | `src_path` CHECK surfaces as `internal` with raw Postgres text |
| #890 | XS | two tool-description nits |
| #871 | XS | delete the `survey_work_items` deprecated alias |

## What happened

### #886 does not reproduce — the probe was reading a UTC clock

Every write path already enforces the freeze, and has since sprint 011:
`create_item`, `reorder_day`, `delete_item` and `move_item` each compare
`plan_date` against `LifecycleContext::today`, and both transports build that
context from `KorgConfig::lifecycle_context()`. Nothing routes around it.

What the T3 probe hit is that production runs `KORG_TIMEZONE=America/Los_Angeles`
while the probing session dated its calls in UTC. The probes ran at
2026-08-02T01:10–01:11Z, which is **2026-08-01 18:10 PDT** — so `plan_date:
"2026-08-01"` was the server's *today*, not a past day. Create, delete and
reorder were all correct to succeed.

The finding is refuted, but it points at a real gap: **nothing on the surface
tells an agent what the server thinks "today" is**, so any session reasoning
from its own UTC clock is a day ahead for the seven hours between 17:00 local
and midnight. Fixed at that root instead:

- `FrozenPast` and `TargetPast` carry both dates and name the server's local
  today in their message, so the next agent to trip over the boundary is told
  where the boundary is rather than left to infer it. History's end refusal got
  its own variant, `HistoryNotPast` — it was `InvalidRange(&'static str)`, which
  structurally could not name a date.
- Five daily-plan tool descriptions now say the boundary is the server's local
  date, derived from its configured timezone rather than the caller's clock.
- `past_days_are_frozen_and_the_refusal_names_the_servers_today` pins the freeze
  per operation against a fixed `today`, so the "unenforced" reading cannot be
  filed a third time, and the module docs carry the timezone analysis.

### #883 — the window-function total

`list_work_items_lean` was the only paginated read computing `total` with
`count(*) OVER()`, which returns no rows at all — and therefore
`unwrap_or(0)` — once `offset` lands past the last row. `list_links`,
`list_cards` and `list_topics` all used a separate scalar `COUNT`, which is
why they never showed the defect. `neighbors` and `related_context` also use
`count(*) OVER()` but take no `offset`, so an empty result there genuinely
means zero edges; both left alone.

Fixed by folding `total` into the `wi_omitted` round trip as a third
`count(*) FILTER (…)` — no new query, and the count now comes from a statement
whose result set is never empty.

### #884 — archived projects as write targets

`resolve_project` and `resolve_project_patch` are the two chokepoints every
write goes through (`create_work_item`, `create_card`, `create_link`,
`create_topic`, `propose_sprint`, `create_handoff`, and the `update_card` /
`update_work_item` project moves). Neither looked at `project.status`, so a
known-but-archived name resolved and mis-filed — the exact failure the
`not_found`-on-unknown-name behaviour exists to prevent.

Both now reject an archived target with `invalid_input` naming the status —
the point of naming it being that a stale name reads as *retired* rather than
misspelled, and the remedy is a different project, not a different spelling.
`update_project` is deliberately *not* routed through this: setting `status`
is how a project comes back, and a test asserts it still can. The MCP roster
sentence gained four words saying an archived name is refused rather than
filed, since the claim is now enforced and worth knowing before the failure.

### #885 — the frozen `updated`

`updated` lives on `node`; the 0001 touch trigger fires on writes to `node`
and `comment` only. Ordinary field edits of work items, cards and proposals
write the *satellite* table (`workitem`, `card`, `sprint_proposal`), so they
never touched `node` — which is why only `archived`/`tags`/`category`/project
moves (all node-column writes) advanced it.

Fixed app-side with a `touch_node` helper rather than a satellite trigger: the
importer's second pass (`UPDATE workitem SET parent_node_id`) would otherwise
clobber `node.updated` for every parented row, and the node trigger's
unconditional `NEW.updated := now()` makes that unrecoverable — `fidelity.rs`
asserts those timestamps.

`update_link` / `set_link_disposition` / `mark_link_read` had the identical
defect (untested by T3, same mechanism) and are fixed in the same pass.
`update_handoff` and `upsert_report` had already hand-rolled this exact
`UPDATE node SET updated = now()` inline, which is why handoffs and reports
looked correct to T3; both now call the helper, so the invariant is one
function rather than three copies and an absence.

### #887 — src_path validated app-side

`check_src_path` mirrors migration 0019's `project_src_path_canonical` regex
and returns `invalid_input` naming the rule that broke, in the shape of the
description-cap error the same tool family already gets right. The CHECK stays
as the backstop.

### #890 — the two nits

1. `relate`'s near-miss promise is now true rather than dropped: the
   suggestion pass gained a separator-insensitive comparison, so `depends-on`
   suggests `depends_on`. The registry is a closed five-entry set and `-`/`_`
   is the one confusion it invites by construction (it holds both `depends_on`
   and `related-to`), so this is not fuzzy matching — the pass can only ever
   match a label that differs in nothing else, and `blocks` still gets the bare
   registry with no guess, which is asserted alongside.
2. `get_work_item`'s "page the tail via list_comments" became "fetch the full
   thread via list_comments" — `list_comments` returns a bare array and has no
   pagination.

### #871 — alias deleted

Gate confirmed clear: no skill *calls* `survey_work_items`. The tool
registration, `ops::SurveyWorkItems`, its `From` impl, the dispatch arm, the
two schema helpers only it used and the catalogue's deprecation note are gone.
The surface goes 48 tools → **47**. The REST `GET /api/work-items/survey` route
is untouched — it is the Review page's endpoint and never went through the
deprecated op struct.

The two `server.rs` suites written against the alias were *retargeted* onto
`list_work_items` rather than deleted: the paging walk and the #851
archived-default fence are coverage of a read that still exists, and deleting
them would have quietly narrowed the surface's test coverage as a side effect
of a cleanup. The empty-page assertion for #883 went into the retargeted paging
test, which is where a reader looks for paging behaviour.

### The fences were checked against the bug, not just the fix

Every behaviour fix here is asserted by a test that passes — which proves
nothing on its own, since a test asserting the wrong thing also passes. So each
one was re-run against a deliberately reverted `repo.rs`: the window-function
`total`, a neutered `touch_node`, the status check dropped from
`project_id_for_name`, and `check_src_path` unwired. All five behaviour tests
fail against those, and only those, then pass again on restore.

## Decisions

- **#886 refuted, not fixed.** The WI is closed with the timezone analysis
  rather than a behaviour change; what shipped is the discoverability fix that
  would have prevented the false positive.
- **App-side touch over a DB trigger for #885**, for the importer-fidelity
  reason above. The trigger is the more robust shape in the abstract; it is
  not worth breaking a faithful import to get it.
- **`total` folded into the existing omitted query** rather than adding a
  fourth scalar-count read, keeping `list_work_items_lean` at two round trips.
- **Links included in #885's fix** though T3 never probed them: same
  mechanism, same class, and leaving them would have made the regression test
  assert an invariant the code only half held.

## Follow-ups

- `refill-queue`'s SKILL.md carries a parenthetical saying `survey_work_items`
  "still answers" — false as of this sprint. Lives in the agent-skills repo,
  so it is a separate one-line change there, not korg's to make.
- #888 (links: no archive/delete, no timestamps) and #889 (area lifecycle)
  from T3 remain unclaimed by any proposal.
- Re-run plan §5's exit measurements once this deploys; #883–#887 and #890
  were six of the seven behaviour gaps that failed the works-as-documented
  measurement.
- The `updated` class has one member left: REVIEW.md F-18, `project.updated`.
  Projects have their own touch trigger (0013) and are not node-backed, so they
  were outside this fix's mechanism entirely — worth confirming rather than
  assuming, since #885 turned out to be its wider sibling.
- `check_src_path` is stricter than the CHECK for non-ASCII whitespace
  (Rust's `is_whitespace` vs Postgres' `[:space:]`). Stricter is the safe
  direction — nothing reaches the constraint that the app would accept — and
  the corner is a no-break space in a filesystem path, so it is recorded rather
  than reconciled.

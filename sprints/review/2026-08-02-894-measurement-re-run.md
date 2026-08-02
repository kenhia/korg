# Measurement re-run — the T3 fixes, verified against production (2026-08-02)

Sprint 040, proposal korg:894 — the agent-surface program's second exit gate.
Method: T2's and T3's — call the tools against production (kubsdb :5674),
measure, never reason from schemas or docs. This session read plan §5, the
T3 findings doc, and 038/039's records before probing; `docs/api.md`,
`repo.rs` and `tools.rs` stayed unread until every finding below was written.

Scope, per 894's briefing comment: re-verify each T3 finding, sweep 039's
never-audited new surface with fresh eyes, answer F-18, and re-run plan §5's
two exit measurements. Lighter than a full sweep by design.

**Surface size: 50 tools**, confirmed independently from the session's own
`tools/list` (47 after 038's alias deletion, +3 from 039).

---

## The one gap — `project` rows expose no timestamps on any surface

**Answering F-18, which the plan asked this session to confirm rather than
assume.** The answer is not the one the checklist anticipated: the question
is not whether `project.updated` advances, but that **nothing can observe it
in the first place.**

- `get_project {name:"korg"}` — documented as "every column — including
  `notes`" — returns `areas, category, deploy_to, description, gh_repo, id,
  machines, name, notes, src_path, status`. No `created`, no `updated`.
- `list_projects {detail:"full"}` — documented as "every column including
  machines, src_path, gh_repo, category and notes" — same set. No timestamps.
- `GET /api/projects` on production returns the same projection, so this is
  not an MCP-only omission.

Every other kind carries both: work items, cards, proposals, topics,
handoffs, reports, comments, and — since 039 — links.

**Why it matters, and why it is small.** #885's premise was that `updated` is
evidence of recency. That premise now holds everywhere except projects, which
are precisely the rows whose routing metadata (`description`, the boundary
clauses, `src_path`) an agent most needs to know the age of. But no live
workflow is broken by it today, and the fix is a projection change at most.

**Class:** schema-vs-behaviour. Two tool descriptions claim "every column"
and deliver a subset. That is the same shape as 039's link finding — where
the premise correction was that timestamps were *unprojected, not absent* —
and this session deliberately wrote the finding from the outside before
checking which of the two applies here.

**Confirmed after the finding was written: unprojected, not absent — and
korg predicted this itself.** The `project` table carries both columns, and
migration `0013_project_touch.sql` (WI #529) attaches the `touch_updated()`
trigger to it, so `project.updated` *does* advance on every write. Its own
header says why nobody can tell:

> Latent today (ProjectRow doesn't expose timestamps) but a booby trap for
> anything that starts sorting projects by recency.

So F-18's answer is: the trigger is fine, and has been since 0013. The gap is
one struct — `ProjectRow` ([repo.rs:2003](../../crates/korg-core/src/repo.rs#L2003))
— omitting two fields, and two tool descriptions promising otherwise. The fix
is XS: add the fields, regenerate bindings, correct nothing else. 0013 called
it a booby trap and left it armed; #885 is what walked into the room, by
making `updated` load-bearing everywhere else.

---

## Every other T3 finding: fixed, verified live

| WI | probe | result |
| --- | --- | --- |
| #883 | `list_work_items {project:"korg", offset:100000}` | `items:[]`, **`total:22`**, `omitted` intact — T3 got `total:0` |
| #884 | `create_work_item {project:"kris"}` | `invalid_input`, "project 'kris' is archived and cannot take new work", routes to `list_projects` |
| #885 | ordinary edits on all three kinds | WI 903 `04:36:38 → 04:36:56`; card 877 `01:05:45 → 04:38:45`; proposal 894 `01:47:03 → 04:17:20` |
| #886 | `create_daily_plan_item {plan_date:"2026-07-31"}` | `invalid_input`, "must be the server's local today (**2026-08-01**) or later" |
| #886 | `reorder_daily_plan {plan_date:"2026-07-31"}` | `conflict`, "past daily plan structure is frozen: … before the server's local today, **2026-08-01**" |
| #887 | `update_project {src_path:"~/src/tools/korg (dev copy)"}` | `invalid_input` naming the rule ("contains whitespace"), no Postgres text, routes to `notes` |
| #888 | `list_links` | rows carry `created`/`updated`/`archived`; `update_link {archived:true}` round-trips; probe link 878 gone |
| #889 | `list_areas {project:"korg"}` | every row carries `description` |
| #890 | `relate {label:"depends-on"}` | "did you mean 'depends_on'?"; `get_work_item` now says "one unpaginated array" |

**#886 stayed refuted, and the trap fired on schedule.** This session's own
clock read 2026-08-02 while the server's local today was 2026-08-01 — the
exact off-by-one that produced T3's false finding. The refusals naming the
date are what made it a non-event, which is 038's root-cause fix earning its
keep rather than merely passing a test.

**#885's prospective caveat held without incident.** Link 123 (created
2026-06-25, untouched since) still reads `updated == created`. That is
correct, not a regression, and the briefing's warning is what kept it from
being filed as this loop's #886.

---

## 039's new surface — first fresh-eyes audit

Never reviewed before this session. The load-bearing claim is the
refuse-if-referenced contract on both new deletes; it holds precisely, in
both directions.

- **`delete_link` refuses when referenced.** Probe link 904 with one edge and
  one comment → `conflict`: "node 904 is referenced by 1 relationship(s),
  1 comment(s) — delete is for rows that were never real; archive it instead,
  or remove the references first." Counts the references and names their
  kinds.
- **`delete_area` refuses when referenced.** Area `probe-894` with one work
  item filed under it → `conflict`: "still has 1 work item(s) filed under it
  — move them off it first; deleting would silently unfile them." Names the
  count and the consequence.
- **Both succeed once clean.** References removed → `{deleted:true}` on both.
  The contract is a gate, not a refusal-shaped wall.
- **Both are honest on a miss.** `delete_link 999999` and
  `delete_area {name:"no-such-area-894"}` → `{deleted:false}`, not
  `not_found`, matching their documented return.
- **`update_area` renames and re-describes.** `probe-894` →
  `probe-894-renamed` kept id 54, and WI 903 followed the rename
  (`area:"probe-894-renamed"`) exactly as documented. Renaming onto an
  existing name → `conflict`, "project 'korg' already has an area named
  'ui'".
- **`update_link` is one transaction.** `archived` + `disposition` + `read`
  in a single call, `updated` advanced `04:36:59 → 04:37:36`; archived rows
  leave the default `list_links` and appear under `archived:true`.

The disposal doctrine's own worked example ran end to end during cleanup:
this session's probe link and probe area were both hard-deleted after their
references were removed, and the probe work item — which has identity, so
`delete` is not offered for it — was categorised `EVAL` and archived.

---

## Works as documented (spot-checks, with the calls)

- **Envelope math.** `list_work_items {project:"korg"}` → `total:22`,
  `omitted:{archived:2, closed:112}`; `wi_status:"all"` → `total:134` =
  22+112 ✓. An explicit filter zeroes the corresponding `omitted` field.
- **Caps count the caller's real input.** `propose_sprint` with a 550-char
  summary → "is 550 characters; the routing contract caps it at 500. Put the
  analysis in `notes`" ✓; `update_project` with a 178-char description →
  the same shape, counted ✓. Nothing partially stored either time.
- **Linking layer.** `covers` left-kind and `has_handoff` right-kind both
  validated with precise messages ✓; self-edge rejected ✓; undirected reverse
  returned the existing edge id (612) rather than a duplicate ✓; `neighbors`
  rendered `directed:false` correctly ✓; `unrelate` → `{deleted:true}` ✓.
- **Handoff guards.** Empty `related_node_ids` rejected naming
  `allow_standalone` ✓; `[903, 999999]` → `not_found` with **no partial
  insert** (903 carried no dangling edge afterwards) ✓.
- **Focused reads.** `get_work_item 903` inlined the `related` block with
  kind, label, direction and title ✓; `get_topic 879` served an archived
  topic with the flag visible ✓; `get_proposal 466` (a work item) →
  `not_found`, "no proposal with node_id 466" ✓.
- **Comments.** `list_comments` returns a bare unpaginated array ✓;
  `update_comment` preserved `created` (04:37:13) and advanced `updated`
  (04:38:46) ✓; `delete_comment` → `{deleted:true}` ✓.
- **Selector guard.** `update_work_item` with both `project` and
  `project_id` → `invalid_input`, "pass either project_id or project, not
  both" ✓.
- **Idempotent creates.** `create_project {name:"korg"}` → existing id 11 ✓;
  `create_area` updated the description in place ✓.
- **Deprecated aliases still function while steering.** `mark_link_read` and
  `search_topics` both returned correct results ✓ (`survey_work_items` is
  gone, as 038 shipped).

Unchanged ergonomic observations, not gaps: the topics corpus is still empty
in production; `neighbors` rows still carry refs without titles (the focused
reads' `related` block remains the fix).

---

## Exit measurements (plan §5)

1. **Is the korg block in the global `~/.claude/CLAUDE.md` empty?**
   **YES.** Read live this session: 51 lines, carrying klams,
   deferred-tools and remote-edit sections. No korg block. Unchanged from
   T3 — nothing regressed back into it across 038 and 039.
2. **Are the findings all "works as documented"?**
   **NO — but by one XS finding, down from seven.** The projects-timestamps
   gap above is the only schema-vs-behaviour claim that failed.

**Verdict: the loop turns once more, narrowly.** Strictly by §5's rule, both
measurements must hold and one did not, so a WI is filed and the program
stays where it is. But the shape of the result is the real signal: T3 found
seven behaviour gaps across the surface; this pass found one, in a projection,
on the only kind that had not been through a timestamps sprint. Every fix
from 038 and 039 held under fresh probing, and 039's new surface — audited
here for the first time — needed no corrections at all.

The honest reading is that this is the last turn before the drop, not
evidence the program needs another round of the same size. That call is
Ken's; §5's rule as written says turn, and this doc does not pre-empt it.

---

## Declared residue (production writes that outlive this session)

- **WI 903** — probe, archived, `category:"EVAL"`, tagged
  `probe`/`agent-surface-894`. Work items have identity, so archive is their
  disposal; the `EVAL` category is 039's doctrine for test residue.
- **Card 877** (T3's probe, already archived) — its `rank` was moved to 1.5
  to exercise the card half of #885. No new residue; an existing archived
  probe row was reused deliberately rather than creating another.
- **Report 880** (T3's) — untouched. Reports still have no disposal; this
  session created none, for that reason.
- Probe link 904, probe area `probe-894`, comment 363 and edge 612 were all
  **hard-deleted** on exit. Links total back to 4, korg areas back to 4.
- The `korg` project row was probed only with calls that were **rejected**
  (`src_path` and `description` cap violations), so nothing was stored. Its
  `description` and `src_path` are as they were found.
- Daily plan days left exactly as found — every daily-plan probe was a
  refusal.

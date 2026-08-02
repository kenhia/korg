# 039 — Disposal semantics: links, areas, and what delete means

Proposal: korg:893 (rank 4.7, pinned). The lifecycle cluster T3 exposed, taken
with the question it belongs to: three kinds with no honest way to end a row's
life, and no written rule saying which ending is which.

## Goal

Decide **once** what disposal means in korg, write it down, then build only the
mechanics that decision implies.

| WI | size | what |
| --- | --- | --- |
| #855 | S | what "delete a project" should mean; `Agent-Plan`/`feedhub`/`loglens` |
| #888 | S | links have no archive/delete path, and expose no timestamps |
| #889 | S | area descriptions are write-only; areas have no lifecycle at all |

## The decision

korg had two disposals in the code (`archived` on nodes and projects; hard
`DELETE` for comments, plan items and edges) and **no rule** saying which
applies to what. #855 names the cost precisely: using one status for three
different claims is how the corpus drifts.

The rule, now in `docs/api.md` under *Disposal semantics*:

> **`archived` means "real, but not a target for new work."** Lossless,
> reversible, keeps history. This is the disposal for anything with identity:
> work items, cards, proposals, topics, reports, handoffs, links, projects.
>
> **A hard delete means "this was never real."** Irreversible, and it
> **refuses when the row is referenced** — the caller has to resolve the
> reference explicitly rather than have the database silently cascade or
> null it out. It exists only where mis-capture is routine and there is no
> history worth keeping: comments, plan items, edges, links, areas.
>
> **Test residue is a `category`, not a disposal.** `EVAL` (migration 0018)
> already makes harness leakage findable as a group. It needs no new
> mechanism.

The refuse-if-referenced clause is the load-bearing half. Postgres would
otherwise answer the question for us and answer it wrongly in both directions:
`relationship` and `comment` cascade from `node`, so deleting a link would take
its edges and thread with it; `workitem.area_id` is `ON DELETE SET NULL`, so
deleting an area would silently orphan every item filed under it. Neither is a
decision a caller made.

### What that settles for #855

All four of #855's questions, with no schema change:

1. **Is there a delete at all?** Yes, but not for projects. A project always
   has identity — it is the routing target for real work, and #828 already
   settled `archived` as "not a target for new work". There is nothing left
   for a project delete to mean.
2. **What happens to rows that reference them?** Refuse. Same shape as the two
   new deletes below.
3. **Are the three cases identical?** No, and the doctrine separates them:
   - `Agent-Plan` is a **superseded idea** — exactly the history the archive
     exists to keep. It stays archived. Deleting it would destroy the record
     of the thing `sprint_proposal` replaced.
   - `feedhub` / `loglens` are **test residue** — a category question.
4. **Does `EVAL` already solve the second case?** **Yes — verified live against
   kubsdb before writing any code:**

   ```
   get_project("feedhub") → category EVAL, status archived
   get_project("loglens") → category EVAL, status archived
   ```

   Both already carry the marker 0018 added for exactly this. Two thirds of
   #855 dissolved without a line of Rust, which is what 816 predicted.

**So #855 ships as a written rule plus one data fix, not a `delete_project`
verb.** The cheap hypothesis was the right thing to test first.

### The drift #855 asked us to fix first

`Agent-Plan`'s five placeholders (#108–#112) each carry a comment saying
*"Superseded by real sprint_proposal node NNN. Archived; kept for history."*
and none of them is archived. Verified live:

```
list_work_items(project:"Agent-Plan", wi_status:"all", archived:null)
  → total 5, omitted {archived: 0, closed: 0}
```

`omitted.archived: 0` over an unfiltered read is the proof: all five are live
rows claiming to be archived ones. Honoured the comments — set the flag. Left
`wi_status` alone; `closed` is reserved for Ken. After:

```
list_work_items(project:"Agent-Plan", wi_status:"all")
  → total 0, omitted {archived: 5, closed: 0}
```

Nothing lost, nothing deleted, and the rows now say what their comments say.

### What it settles for areas

An area is a **label with a description**, not a record with a history — it has
no node identity, no `archived` column, and nothing accumulates on it. So areas
get a delete, not an archive, and no migration. Retiring an area means moving
its work items off it first; the refusal is what forces that to be a decision.

## What shipped

### #888 — links

- `LinkRow` now carries `created` and `updated`. **No migration needed** —
  links are nodes, and `node` has carried both since 0001. The proposal
  guessed a link-timestamps migration; the columns were already there and
  simply were not selected.
- `update_link` accepts `archived`, so the disposal `list_links` has always
  documented is finally reachable from the surface that documents it. The
  0001 touch trigger already bumps `updated` on the flip (#885's fix in 038
  covers the ordinary-edit case).
- New `delete_link` — refuses with `conflict` if the link has any edge,
  comment, or daily-plan reference; `{deleted: false}` if there was no such
  link, matching `delete_comment`.

### #889 — areas

- `list_areas` and `get_project`'s inlined areas return `description`. The
  idempotent-description contract `create_area` documents is now verifiable
  from the surface that offers it.
- New `update_area` — rename and/or re-describe. Rename was the one genuinely
  missing capability; `create_area` already handled description idempotently.
- New `delete_area` — refuses with `conflict` naming the count if any work
  item is filed under it.

### Docs

- `docs/api.md` gains the *Disposal semantics* section — the rule above, plus
  the per-kind table `docs_drift` can hold to.
- Tool count 47 → 50.

## Follow-ups

- **Probe link 878 is disposed after deploy, not before.** `delete_link` does
  not exist on kubsdb until this ships, so the stuck row #888 cites as live
  evidence is cleared as a ship step. **Done — see below.**

## Deployed 2026-08-02

PR [#42](https://github.com/kenhia/korg/pull/42), squash-merged as `baec94c`.

| | |
| --- | --- |
| Image | `korg:baec94c1bb9e` (label `…image.revision=baec94c1bb9eb8f91cdc0f14f06fc870fbd2e531`) |
| Rollback target | `korg:21c6d597` (sprint 038) |
| Migrations | none — 21 before, 21 after |

`post-deploy-check.sh --compare` exited 0 with **every** row count unchanged
(work_items 611, cards 30, links 5, proposals 124, reports 24, projects 36,
topics 1; node_count 806). No migration in this sprint, so an unchanged schema
is the expected result rather than a lucky one.

### Verified live, per fix

- **50 tools** over MCP, including `delete_link`, `update_area`, `delete_area`.
- **#888** — `/api/links` rows now carry `archived`, `created`, `updated`.
- **#889** — `/api/areas?project=korg` returns descriptions, and three of the
  four had one already: *"Timebox slots, slot templates, weekly schedule."*,
  *"korg backend"*, *"User facing web UI"*. **Written long ago and unreadable
  from the surface until this deploy** — the bug, visible in its own fix.
- **The refusal** — `DELETE /api/areas {project:"korg", name:"engine"}` →
  `409 conflict`, *"area 'engine' still has 5 work item(s) filed under it"*,
  and the area survives with its five items. The load-bearing clause holds in
  production, not just in tests.
- **Probe link 878 disposed.** `DELETE /api/links/878` → `{"deleted":true}`;
  links total 5 → 4. The doctrine's first worked example against the exact row
  that motivated it: a probe capture that was never real, so a delete rather
  than an archive.
- Deep link `/plan` → 200.

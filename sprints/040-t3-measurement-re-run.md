# 040 — Measurement re-run: verify the T3 fixes, close the loop or drop the rank

Proposal korg:894 (rank 4.8), the agent-surface program's second exit gate.
Covers no work items by design — it produces them.

## Goal

Re-verify each T3 finding against production, sweep 039's never-audited new
surface with fresh eyes, answer F-18, and re-run plan §5's two exit
measurements. Both hold → the program drops to low-rank continual
improvement. Either fails → file a WI per gap and the loop turns again.

No code shipped in this sprint. The deliverables are the findings doc and
what it produced.

## What happened

Everything is in [`sprints/review/2026-08-02-894-measurement-re-run.md`](review/2026-08-02-894-measurement-re-run.md).
The short version:

- **All nine T3 findings verified fixed against production**, each with the
  call that proves it. Surface size confirmed at **50 tools** independently
  from the session's own `tools/list`.
- **039's new surface audited for the first time** — `delete_link`,
  `delete_area`, `update_area`, the link timestamp/archived projection. The
  refuse-if-referenced contract, the load-bearing claim, holds in both
  directions: it refuses with counts and kinds when references exist,
  succeeds once they are gone, and returns `{deleted:false}` honestly on a
  miss. No corrections needed.
- **One gap found: #905** — `project` rows expose no `created`/`updated` on
  any surface, while `get_project` and `list_projects detail:"full"` both
  promise "every column".

## The trap fired, and the briefing defused it

The session's own clock read **2026-08-02** while the server's local today
was **2026-08-01** — precisely the off-by-one that produced T3's false #886.
It was a non-event because 038's refusals name the date: "must be the
server's local today (2026-08-01) or later". That is the difference between
a fix that passes a test and a fix that prevents the next session's mistake.

The briefing's other warning earned its keep too: #885's fix is prospective,
and link 123 (created 2026-06-25, untouched since) still reads
`updated == created`. Correct, not a regression — and exactly the shape that
would have become this loop's #886 if filed.

## Decisions

- **F-18 is answered, and reframed.** The question was whether
  `project.updated` advances given projects' own 0013 touch trigger. It does.
  Nothing can observe it. The finding is the projection, not the trigger —
  and `0013_project_touch.sql`'s own header predicted this outcome in
  writing ("Latent today (ProjectRow doesn't expose timestamps) but a booby
  trap…"). #885 is what armed the trap, by making `updated` load-bearing
  everywhere else.
- **The finding was written before the source was read**, per T2/T3's method,
  and only then checked against the migration. This is what turned it from
  "projects have no timestamps" into "projects have timestamps nothing
  projects" — the same premise correction 039 had to make about links, caught
  before publication this time rather than after.
- **No new probe reports.** Reports still have no disposal, so the cheapest
  correct choice was to create none.
- **Reused T3's archived probe card** (877) to exercise the card half of
  #885 rather than creating a second immortal card. Its `rank` moved to 1.5;
  declared in the residue section.

## Exit measurements

1. **korg block in the global `~/.claude/CLAUDE.md` empty?** **YES** — read
   live, 51 lines, no korg block. Nothing regressed into it across 038/039.
2. **All findings works-as-documented?** **NO** — by one XS finding (#905),
   down from T3's seven.

**Verdict: the loop turns once more, narrowly.** §5's rule is that both must
hold; one did not, so #905 is filed and the program stays where it is. The
shape of the result is the real signal though — every 038 and 039 fix held
under fresh probing, 039's new surface needed no corrections, and the single
remaining gap is a two-field struct change that a migration header flagged
as latent three programs ago. This reads as the last turn before the drop
rather than a call for another round of the same size, but that judgment is
Ken's; the rule as written says turn.

## Follow-ups

- **#905** (XS) — the fix. A natural single-WI proposal, or a rider on
  whatever lands next in korg.
- After #905: re-running measurement 2 is cheap — it is one `get_project`
  call plus a check that no new surface landed meanwhile.

## Residue

Declared in full at the end of the findings doc. Net production residue from
this sprint: **WI 903** (probe, archived, `category:"EVAL"`) and **WI 905**
(the deliverable). Probe link, probe area, probe comment and probe edge were
all hard-deleted on exit — links total back to 4, korg areas back to 4. The
`korg` project row was probed only with calls that were rejected, so nothing
was stored.

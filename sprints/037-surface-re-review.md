# 037 — Re-review the korg surface, fresh session (T3)

Proposal: korg:868 (rank 4.5, the agent-surface program's exit gate).
Run 2026-08-02, immediately after ranks 4.1–4.4 landed and deployed.

## Goal

T2's method (#634) against the post-contract surface: a session with the
repo's docs and code deliberately unread calls every MCP tool against
production, measures, and compares behaviour to what the schemas and server
instructions claim. Deliverables: a findings doc, a WI per gap, and plan
§5's two exit measurements.

## What happened

The full record is `sprints/review/2026-08-02-t3-surface-re-review.md`.
47 of 48 tools called (create_area skipped deliberately — its probe row
would have been immortal, which is itself finding 7). All probe writes
archived/deleted afterwards; residue declared in the findings doc.

**Eight WIs filed:** #883 (paginated `total` lies past the last row),
#884 (archived projects accept new work), #885 (`updated` frozen on
ordinary edits of WIs/cards/proposals — the sharpest one), #886 (daily-plan
past-day freeze unenforced), #887 (src_path CHECK → raw internal error),
#888 (links: no archive/delete, no timestamps), #889 (area descriptions
write-only, no area lifecycle), #890 (two description nits).

**Exit measurements:** the global CLAUDE.md korg block is empty — **pass**
(T2's two traps were fixed at source, the block emptied rather than
growing). Findings all works-as-documented — **fail** (7 behaviour gaps).
One short of the both-hold condition, so the program stays at rank rather
than dropping to low.

Worth saying plainly: the 2026-08 contract work held. No token bombs, no
lying defaults about corpus size, the linking layer airtight, the cap
errors exemplary. The new gaps are a tier smaller than what T2 found — the
loop is converging.

## Decisions

- create_area deliberately not live-fired (immortal row); the read-surface
  gap was provable without it.
- Probe writes went to production under `probe` / `agent-surface-037` tags
  with archive-on-exit — same declared-residue discipline as T2.
- REVIEW.md (T1) was read only after exploration completed, to
  cross-reference F-18 and the daily-plan "model citizen" irony.

## Follow-ups

- A fix sprint over #883–#890 (mostly S/XS; #885 is the M) is the natural
  rank-4.6 successor; then re-run §5's measurements.
- Plan §5's self-enforcement note applies to #883: pin `total`'s meaning on
  empty pages in docs_drift / a contract test, not just in the fix.

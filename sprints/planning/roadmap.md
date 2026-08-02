# Roadmap

> The general plan for this project. Keep it current; detail lives in the
> sprint records. The active program plan is
> [2026-08-korg-agent-surface.md](2026-08-korg-agent-surface.md).

## Now

- **Agent-surface program, endgame** — ranks 4.1–4.6 shipped (sprints 034–038
  + the agent-skills consumer sweep; the global CLAUDE.md korg block is empty
  on the fleet). 868's re-review (037) failed the works-as-documented
  measurement with seven behaviour gaps; 038 closed six of them plus #871's
  alias deletion, and refuted the seventh (#886 — the probe read a UTC clock,
  not a broken freeze). 038 deployed 2026-08-02 with per-fix live
  verification; the surface is 47 tools. Remaining: **893** (disposal
  semantics — #888/#889, the two T3 gaps still open, plus #855), then
  **894 re-runs plan §5's exit measurements** — with 893 landed, the
  works-as-documented half has a real chance of passing. Side quest: 896
  (agent-skills, rank 0.5) deletes refill-queue's dead alias mention.

## Next

- 825 — row-contract markers (`has_handoff`, in-proposal, has-details); also
  what lets `/api/work-items` go lean (036 D-8).
- #846 — the korg-side `cancel-in-progress` scoping (one line; the skill half
  is verified live).
- Staleness work from 816 §6: #862 `docs_drift` extensions, agent-skills #863
  `start-sprint` premise check.

## Later / Ideas

- #855 — true-delete decision for `Agent-Plan`/`feedhub`/`loglens`.
- #842 — production DB credential lives only in the running container
  (security; belongs to infra triage, not this program).

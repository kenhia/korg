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
  not a broken freeze). 039 added the disposal doctrine + link/area
  lifecycle (50 tools). 040's measurement re-run verified every fix held;
  measurement 2 failed by **one XS finding** (#905 — project rows expose no
  timestamps while promising "every column"). Remaining: **906 (rank 4.9)**
  ships #905, then the overseer's one-call re-measurement — on green the
  program drops to low-rank continual improvement.

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

# Roadmap

> The general plan for this project. Keep it current; detail lives in the
> sprint records. The active program plan is
> [2026-08-korg-agent-surface.md](2026-08-korg-agent-surface.md).

## Now

- **Agent-surface program, endgame** — ranks 4.1–4.4 shipped and deployed
  (sprints 034–036 + the agent-skills consumer sweep; the global CLAUDE.md
  korg block is empty on the fleet). Remaining: #871 (delete the
  `survey_work_items` alias, XS, un-gated), then 868 — the fresh-session
  re-review that is the program's exit gate.

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

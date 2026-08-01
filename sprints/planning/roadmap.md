# Roadmap

> The general plan for this project. Keep it current; detail lives in the
> sprint records. The active program plan is
> [2026-08-korg-agent-surface.md](2026-08-korg-agent-surface.md).

## Now

- **Agent-surface program** — make the MCP surface cheap to read and honest
  about what it returns. Step 0 (this harness) done; next is "stop the
  bleeding": #852 (`list_proposals` unbounded) + #851 (`survey_work_items`
  archived default).

## Next

- Proposal 817 — e2e coverage against a real database ("fix the instrument").
- The collection read contract: proposal object shape (`summary` → short
  contract + overflow field, with migration) and work-item read tiering.
- Consumers: the korg block in the global `CLAUDE.md` shrinks to empty as the
  traps get fixed at source.
- Re-review with a fresh session — measure, don't reason from schemas.

## Later / Ideas

- #846 — CI cancels the merge commit's run (XS).
- #855 — true-delete decision for `Agent-Plan`/`feedhub`/`loglens`.
- Staleness work from 816 §6: `docs_drift` extensions, `start-sprint`
  premise check.
- #842 — production DB credential lives only in the running container
  (security; belongs to infra triage, not this program).

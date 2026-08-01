# 033 — Step 0: retire Phoenix, adopt the kproject harness

**PR:** #37, squash-merged as `c5f756f`, 2026-08-01.
**Program:** agent-surface (`sprints/planning/2026-08-korg-agent-surface.md`), §4 Step 0.

## Goal

korg carried no project-level agent instructions at all — no `CLAUDE.md`, no
`.github/copilot-instructions.md` — while a program of agent sprints was about
to start in it. Fix that, and clear out the dead Phoenix harness.

## What shipped

- Deleted `.phoenix/` (196K of dead `trace.jsonl` + snapshots, untracked) and
  stripped the last two references (`.gitignore`, `.dockerignore`).
- `kproject-init` (migration path): managed harness block + a korg-specific
  Project section into `CLAUDE.md` and `.github/copilot-instructions.md`
  (identical content), seeded `sprints/planning/roadmap.md` from the program
  plan.
- Committed the pending `.scratch/` gitignore line — the plan's §7
  overseer/worker protocol depends on it being ignored.

## Deployed 2026-08-01

- Image `korg:c5f756f3d90a` (label `org.opencontainers.image.revision`
  confirmed on the running container). Rollback target: `korg:e03a194f6394`.
- `post-deploy-check.sh --compare`: every count identical to the pre-deploy
  baseline (`node_count` 772, `node_max` 859, migrations 20 → 20). No schema
  migration in this deploy.
- Smoke: `/plan` 200 over tailnet HTTPS; MCP surface exercised live by the
  same session immediately after (work-item reads/writes).
- First ship after agent-skills' Phase 7.3 change: this deploy record is the
  `[skip ci]` push — checked against korg #846's acceptance criteria below.

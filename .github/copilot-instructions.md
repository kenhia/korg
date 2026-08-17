<!-- kproject:begin — managed by kprojects; do not edit inside this block -->
## kproject conventions

This project uses the kproject minimal harness
(<https://github.com/kenhia/kprojects>). Keep context small; prefer doing
over ceremony.

### Layout

- `sprints/` — the project's evolution, one record per PR-sized unit of
  work (a "sprint")
  - `planning/` — planning docs; at minimum `roadmap.md` (the general plan)
  - `review/` — more formal reviews as the project matures
  - sprint records: `###-<short-name>.md` for small projects, or a
    `###-<short-name>/` directory of files for larger/more formal ones
  - a sprint record is one informal narrative: goal, decisions, what
    shipped, follow-ups — written during the sprint, not after
  - projects that deploy end the record with a `## Deployed` section:
    what shipped, where, when, and what was verified live — appended
    after the deploy, not predicted before it
- `docs/` — project documentation, architecture, usage
- `.scratch/` — git-ignored scratch space for user or agent ephemera;
  use it instead of /tmp
- `justfile` — dev recipes; default recipe is `@just --list`; `just check`
  runs the CI gates; `just deploy` (or variants) if the project deploys
- `.env` — git-ignored; tokens and environment vars

### Workflow

- One sprint ≈ one PR. Sprint proposals and work items are managed in
  `korg`; durable cross-project knowledge goes in `klams`.
- Mark each work item resolved as its work completes — don't batch the
  resolutions into sprint-ship. A proposal's progress should be readable
  while the sprint is running, which is the only time it is useful.
- If the korg or klams MCP tools are unavailable in your session, say so
  up front — don't silently work around missing infrastructure.
- A few projects share contract surfaces with siblings and have a
  **guiding plan** constraining how those change; most have none, and one
  grep is the whole cost of finding out. Grep the `index.md` routing
  table in `kai:~/src/tools/cross-project-planning` — a local path on
  kai, read through kaed from any other host (`root: "kai:src"`, path
  `tools/cross-project-planning/…`); don't clone a second copy. Not
  listed → nothing applies. Listed → read the mapped plan folder before
  planning sessions and before changing a contract surface it names, and
  amend the plan in the same ship when what you build diverges from it.
- TDD preferred: write the failing test first when practical.

### Tooling preferences

- Rust managed by `cargo`; format with `cargo fmt`, lint with
  `cargo clippy --all-targets` (test targets included deliberately — a gate
  that skips them is a gate that lies)
- Mirror `rust-toolchain.toml`, `rustfmt.toml` and `clippy.toml` from a
  sibling homelab repo rather than generating them
- License is MIT unless specifically directed otherwise
<!-- kproject:end -->

## Project

korg is the homelab's **system of record for work**: one typed-node +
generalized-edges data model over Postgres holding work items, cards, sprint
proposals, reading links, reports, handoffs, and projects/areas. A Rust
workspace serves the web UI, REST API and MCP endpoint
from one binary; production runs on kubsdb at `:5674`.

### Build / test

- `just check` runs everything CI enforces (fmt, generated-artefact drift,
  web checks, clippy, full tests). It mirrors `.github/workflows/ci.yml` —
  keep the two in step.
- `just gen` regenerates the ts-rs bindings and the MCP schema snapshot after
  changing any shared operation struct, response row, or vocabulary;
  `just check` fails if the checked-in output is stale.
- The migration snapshot suites run only when `snapshots/*.dump` is present —
  see `KORG_SNAPSHOT_TESTS` in `docs/setup.md`. Rehearse migrations against a
  **fresh** dump; a stale one can pass a rehearsal production would fail.

### Read first

- `docs/api.md` — normative agent-facing contracts: tool catalogue,
  collection-read envelope, relationship label registry.
- `crates/korg-core/src/repo/` — queries and projections, one module per
  domain; `mod.rs` re-exports them all, so callers still say `repo::`.
- `crates/korg-mcp/src/tools.rs` — the MCP tool surface.
- `crates/korg-migrate/` — numbered migrations; each header states its
  rollback semantics.
- `web/` — the SvelteKit UI.

### Gotchas

- The `docs_drift` suite (`crates/korg-mcp`) asserts the docs match the code —
  tool catalogue, README tool count, collection-shape table. Change code and
  docs together or `just check` fails.
- This repo carries `.sprint-deploy` (`deploy-kubsdb`), so `/sprint-ship`
  Phase 7 deploys merged main to kubsdb automatically. For UI iteration use
  the kai dev loop, not a deploy.
- The current program plan of record is
  `sprints/planning/2026-08-korg-agent-surface.md`. Its §7 protocol applies
  when two sessions share this tree: never `git add -A` / `git add .` /
  `commit -a` — stage explicit paths only.

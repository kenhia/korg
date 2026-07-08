# Sprint 010 — done status, editable comments, project metadata (+ Fable review)

Proposal korg:288 — WIs #285 (anchor), #232, #246, #234. Follows the
sprint-009 close that left format issues unfixed.

## WI #285 — status lifecycle

`open | resolved | done | closed` is now the validated vocabulary
(create + update reject anything else; MCP schemas carry the enum and the
who-sets-what semantics — `closed` is documented as Ken-only). The web
constant had dead values ("active", "draft" — zero rows in the DB) and no
"done"; Quick Edit now offers exactly the four. The status *filter* was
already data-derived, so done items were listed correctly all along.

## WI #232 — editable comments

`update_comment` end to end: repo fn (preserves `created`, trigger
advances `updated`), `PATCH /api/comments/:id`, MCP tool, ✎ edit-in-place
in the Comments component (Ctrl-Enter saves, Esc cancels).

## WI #246 — project metadata

Migration 0011: `status` (active|maintenance|inactive|archived, validated),
`machines[]`, `deploy_to[]`, `category` on project. Exposed through
list_projects (REST+MCP), editable via `PATCH /api/projects/:name` and the
new `update_project` MCP tool (name immutable). Work Items rail: only
active+maintenance shown by default ("show all" checkbox), stable
hash-based name colors (first guess — tune by eye), ✎ opens an
edit-everything-but-name panel, Project Details block shows the new
fields. NOTE: panel lives in the left rail rather than the suggested
right-hand pop-out — revisit with the color tuning if it reads wrong.

## WI #234 — korg DB backup

No code: verified already delivered by k-homelab's `korg-backup` recipe
(nightly pg_dump → /gratch/backups/korg, restore-tested 2026-07-08).

## Fable review notes

- fmt drift (10 files) + 1 clippy warning from the 009 close — fixed first.
- Statuses were the only unvalidated enum-ish field on the write path:
  proposals use a real PG enum (`sprint_proposal_status`), cards likewise
  constrained. #285 closes the gap for work items; project.status is
  validated in code.
- The MCP tool-count is asserted as a magic number in TWO test files
  (korg-api/tests/mcp_http.rs, korg-mcp/tests/server.rs) — both needed
  bumping for the two new tools. Fine at this scale; consolidate if it
  bites again.
- WI #284 (deep-link 404) confirmed fixed in 009 (`spa_tests::deep_links_
  serve_shell_with_200`).

## Gates

cargo test --workspace: 25 suites green (3 new sprint010 tests:
status vocabulary enforced, comment edit round-trip, project metadata
round-trip incl. invalid-status rejection). cargo fmt --check clean,
clippy clean, svelte-check 0/0.

# Sprint 003 — Make work-item comments real (node-scoped comments)

## Context

Comments in korg are already stored against **any** node — the schema is
`comment.card_node_id BIGINT REFERENCES node(id)` ([0001_init.sql:121](crates/korg-core/migrations/0001_init.sql#L121)),
with no check that the node is a card. But comments are only *surfaced* for
kanban cards: the UI renders them solely in the Cards edit modal
([cards/+page.svelte](web/src/routes/cards/+page.svelte)) and the only REST
routes are `/api/cards/:node_id/comments`.

This came to light because an agent added two substantial comments to **WI #104**
(`kvllm`, node 163) via the `add_comment` MCP tool shipped in sprint 002 — they
persisted fine but are invisible in the work-item detail view. The `card_node_id`
naming is misleading and the feature is half-exposed.

**Goal:** generalize comments to be honestly node-scoped end-to-end and surface
them in the work-item detail/edit view, so notes on WIs (from agents or the user)
are visible and manageable. Confirmed decisions: full generalization + a shared
`<Comments>` Svelte component.

## Plan

### 1. DB migration — `crates/korg-core/migrations/0007_comment_node_rename.sql` (new)
- `ALTER TABLE comment RENAME COLUMN card_node_id TO node_id;`
- `ALTER INDEX comment_card_created_idx RENAME TO comment_node_created_idx;`
- Fast, non-destructive; auto-applied on deploy (`connect()` runs `migrator().run()`,
  [lib.rs:19](crates/korg-core/src/lib.rs#L19)).

### 2. korg-core — [crates/korg-core/src/repo.rs](crates/korg-core/src/repo.rs)
- `Comment` struct: rename field `card_node_id` → `node_id`.
- Update the SQL in `list_comments` / `add_comment` to select/insert `node_id`.
- Function signatures already take a bare node id; keep param names generic
  (`node_id`). No behavior change.

### 3. REST API — [crates/korg-api/src/lib.rs](crates/korg-api/src/lib.rs)
- Replace `/api/cards/:node_id/comments` with generic
  `GET/POST /api/nodes/:node_id/comments` (same `list_comments`/`add_comment`
  handlers; they already take a node id). Keep `DELETE /api/comments/:id`.
- The cards page is the only consumer of the old path, so migrate it (step 5)
  rather than keep an alias.

### 4. MCP — [crates/korg-mcp/src/tools.rs](crates/korg-mcp/src/tools.rs)
- Rename the `card_node_id` arg → `node_id` in the `add_comment` / `list_comments`
  descriptors and the `AddCommentArgs` / `CardNodeIdArgs` structs (rename the
  latter to `NodeIdArgs`-style). Update tool descriptions to say "a node (work
  item or card)". Pre-adoption breaking change — acceptable and correct now.

### 5. Web — extract shared component + mount on WIs
- **New** `web/src/lib/components/Comments.svelte`: takes `node_id: number`,
  self-contained list + add (Ctrl/⌘-Enter) + delete, lifted verbatim from the
  cards modal markup ([cards/+page.svelte:498-513](web/src/routes/cards/+page.svelte#L498))
  and its handlers (`addComment`/`removeComment`/load in `openEdit`). Preserve
  the `data-testid` hooks (`comment-list`, `comment-input`) so card e2e tests
  keep passing.
- **api.ts** ([web/src/lib/api.ts:195](web/src/lib/api.ts#L195)): rename
  `cardComments` → `nodeComments` and point all three methods at
  `/api/nodes/:node_id/comments` + `/api/comments/:id`. Rename `CardComment`
  interface field `card_node_id` → `node_id` (or `Comment`).
- **Cards page**: replace the inline comment block + its state/handlers with
  `<Comments node_id={editing.node_id} />`.
- **Work-item detail view** — DEVIATION FROM PLAN: comments landed in the
  read-only `detailView` snippet in
  [work-items/+page.svelte](web/src/routes/work-items/+page.svelte) (a sibling of
  the existing Relationships section), NOT inside `WorkItemForm`. Rationale: the
  complaint was "WI *details* don't show comments" — the detail view is what the
  user looks at, and this makes comments visible without entering edit mode.

### 6. Tests
- **core** ([tests/domain.rs] or reads.rs): add a comment to a **work-item**
  node, list it back — proves node-scoping.
- **mcp** ([crates/korg-mcp/tests/server.rs](crates/korg-mcp/tests/server.rs)):
  update the sprint-002 comment assertions to the `node_id` arg; add a case
  commenting on a WI node.
- **api** ([crates/korg-api/tests/api.rs] / mcp_http.rs): hit the new
  `/api/nodes/:id/comments` routes.
- **web e2e** (`web/tests/e2e/`): a WI-comments spec (add → visible → delete);
  keep the existing card-comment coverage green via the shared component.

### 7. Deploy to kubsdb
- Same procedure as sprints 001/002 (`docker build` → `docker save | ssh kubsdb
  docker load` → recreate container via `bash -s`). Migration 0007 auto-applies
  on startup.
- **Verify the payoff**: WI #104 (node 163) now shows its 2 existing comments in
  the work-item view; add/delete a throwaway comment through the UI and via the
  `node_id` MCP arg.

### 8. Ship
- `/sprint-ship`: docs (note node-scoped comments + `/api/nodes/:id/comments` in
  README/usage.md), PR, squash-merge, clean local. Branch `003-wi-comments`,
  sprint docs in `sprints/003-wi-comments/`.

## Verification
- `cargo test --workspace` + `cargo clippy --workspace --all-targets` green.
- `cd web && pnpm build` and the Playwright comment specs pass.
- Post-deploy: `GET /api/nodes/163/comments` returns the 2 kvllm comments; they
  render in the WI detail view; MCP `list_comments {node_id:163}` matches.

## Out of scope
- Editing comment bodies (only add/delete today) — keep as-is.
- Comment authorship/attribution metadata — not modeled; not adding now.
- The other deferred low-value MCP gaps (`set_node_tags`,
  `set_link_disposition`, `recent_project`).

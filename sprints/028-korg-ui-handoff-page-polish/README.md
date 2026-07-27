# Sprint 028 — korg UI: handoff reading page, category colors, papercuts

Proposal `korg:623`. This branch lands the three items that need no one at the
screen — **#583**, **#678**, **#621** — leaving **#622** (the interactive polish
pass) and **#570** (the Review-completed page, whose side-by-side breakpoint
wants tuning on Ken's wide monitor) for a session with Ken present.

Carries **migration 0018**, which is data-only. Ships via `deploy-kubsdb`.

## #583 — filter dropdowns dismiss like dropdowns

One-line problem, and the fix was one line too — until the test found the real
one.

`MultiSelectFilter` is a bare `<details>`, so it closed only by clicking its own
summary again: all five filters could sit open at once. Added an outside
`pointerdown` (capture phase, so a `stopPropagation` elsewhere cannot strand a
panel) and Escape, which also returns focus to the summary. Opening one filter
closes the others for free — their summary is "outside" this one.

**`bind:open` is the trap.** It was the obvious first cut and it desyncs. The
write-back rides the `toggle` event, and clicking filter B's summary *while
filter A is open* leaves B's DOM open with B's Svelte state still `false` — so
B's effect never registers its listeners and Escape does nothing. The DOM and
the state are two sources of truth for one boolean, and the native one wins
silently. `open` is now one-way (state → DOM) with the summary's own click
`preventDefault`ed, so the state is the only thing that opens or closes the
panel.

That failure was invisible to `svelte-check` and to a casual click-through — it
needs the *specific* A-then-B sequence. `filter-dismiss.spec.ts` reproduces it,
and also guards the assumption `preventDefault` rests on: Enter and Space on a
focused `<summary>` dispatch a click, so the control stays keyboard-operable.

## #678 — project color by category

The rail hashed the project *name* to a hue. Deterministic, but arbitrary and
non-scaling: over the live projects it put 20 of 31 within 12 degrees of another
(klams 326 / feedhub 327 were one degree apart) while leaving whole arcs empty.
"It feels random" because it was.

Four parts, per the WI:

1. **`PROJECT_CATEGORIES`** in korg-core `vocab.rs` (`AI`, `Dashboard`, `EVAL`,
   `Fun`, `Infrastructure`, `Ops`, `Other`), validated in `update_project` and
   exported to TypeScript by `just gen`. The MCP `update_project` schema now
   advertises the enum, so an agent sees the closed set.
2. **Migration 0018**, below.
3. **Project Details**: the free-text `category` input is a select over the
   generated vocabulary, no custom escape hatch — the LB-2 treatment. `(none)`
   is a real option, not a placeholder: `create_project` takes only a name, so
   uncategorised is a state a project can legitimately be in.
4. **The rail colors from the category**, `projColor(name)` deleted. Only
   active/maintenance projects are colored; inactive, archived and uncategorised
   render with none, which doubles as a signal that a project is not live.

`CATEGORY_HUE` is typed `Record<ProjectCategory, number>` deliberately: adding a
category to korg-core and running `just gen` breaks the web build until its hue
is chosen. That is the "one registry edit plus its color" contract, enforced.

### The grouping toggle (Ken, this sprint)

Added on top of the WI: a **by category** checkbox on the rail that groups
projects under category headings, with alphabetical as the default. The WI had
flagged the ordering question and said not to change it without asking; Ken's
answer was to offer both rather than pick. Groups run in vocabulary order
(alphabetical), not hue order — the point of grouping is that a category sits in
a predictable place, and hue order would reshuffle the rail every time a hue is
retuned. Uncategorised projects, and any holding a legacy value on a database
that predates 0018, land in a trailing **Uncategorised** group rather than
vanishing. The choice persists to `localStorage`, like the sticky project: it is
a view preference, not a navigation step.

### Migration 0018

Data-only — `project.category` has been a nullable `TEXT` with no constraint
since 0011, so there is no DDL. Two consequences carried into
`docs/operations.md`: it creates **no rollback boundary** (rolling the image back
across this deploy is a clean re-tag, unlike 0017), and a wrong value is fixed by
another `UPDATE`.

Ordering matters the way LB-1 had to precede LB-2: enforcement ships in the same
release, so any row still holding `ai`/`AI`/`tooling`/`infra`/`fun` would fail
the next `update_project` on that project. The true-up lands with enforcement and
the postcondition refuses to leave one behind.

Three things follow 0016's precedent:

- **The touch trigger is disabled across the update.** `project_touch_updated`
  fires BEFORE UPDATE, so a bulk category set would stamp `updated` on every
  project. A fix-up is not a content edit. 0016 hit exactly this.
- **The mapping lives in a temp table**, not spelled twice, so the `UPDATE` and
  the postcondition cannot drift apart. Keyed by name, because project names are
  immutable (#246) — 0016's rehearsal is what caught a hardcoded node-id list
  being coupled to the instant it was written.
- **Counts are not hardcoded.** Asserting "32 rows" would fail on a fresh
  install and on any dev database. The postcondition asserts the two invariants
  that hold on any snapshot: every mapped project carries exactly its mapped
  category, and no project carries an off-vocabulary one. A NULL category is
  reported via `RAISE NOTICE`, not failed — it is legal.

`migration_0018_trues_up_categories_without_touching_updated` runs the real file
over seeded legacy values and asserts all of it, including that `updated` moved
on zero rows and that the postcondition actually fires on a leftover bad value.
The *mapping itself* is Ken's taxonomy and is verified by rehearsal against a
restored dump (`docs/operations.md`), not by a test.

## #621 — the handoff reading page

korg's **first per-node detail route**, so the shape was the decision.

**`/handoffs/:node_id`, not a generic `/nodes/:id`.** The payloads genuinely
differ: `GET /api/handoffs/:id` returns the full Markdown body plus the LB-3
`related` block, which is strictly more than the `/api/nodes/:id` preview shape
carries. A generic page would either render the thin preview — losing the whole
point of a wider read surface — or branch on kind internally, which is a
kind-specific page wearing a generic URL. It would also promise something korg
cannot deliver: only handoffs have a full-page surface today, so `/nodes/123` for
a card would 404 or render a stub. Nothing here is a one-way door — per-kind
routes can be added as each kind earns one, and a later `/nodes/:id` can redirect
by kind.

The page renders the title, summary, chips and timestamps, the read-only body at
the layout's default `max-w-5xl` (against the slide-over's `max-w-md`), the nodes
it is attached to, and a `Comments` thread — nodes are comment-generic (0007), so
that needed no backend. **The body stays read-only**: handoffs are authored
through the API and the `handoff` skill, and a reviewer responds with a comment
rather than editing the document. The "Attached to" block matters more than it
looks — arriving by bookmark or pasted URL is otherwise a dead end.

Entry point is `NodePreview`'s new "Open full page ↗", shown only for
`kind === "handoff"` since nothing else has a route. The slide-over stays the
quick peek.

## Verification

`just check` green. e2e run against a throwaway Postgres — **50 passed**, with
three new cases: `filter-dismiss.spec.ts` (the A-then-B sequence that broke
`bind:open`, plus keyboard operation), the handoff full-page journey
(preview → route → comment → direct URL visit), and an axe scan of the new route.
The a11y suite's route list covers only *static* routes, so the handoff page
needed its own seeded case rather than sitting quietly outside the floor.

Two pre-existing failures were found and one fixed. Both were confirmed by
running the same specs against an unmodified checkout:

- **`work-item-edit.spec.ts` — fixed.** It asserted the relationship picker
  offers exactly four labels; `has_handoff` joined the registry in sprint 025 and
  the list was never updated. e2e is out of CI (`docs/setup.md`), so nothing
  caught it.
- **`daily-planner.spec.ts` — filed as #701, not fixed here.** `:133` fails
  *every Monday*: it asserts on `planner-day-<yesterday>`, and the planner renders
  a Mon–Sun week, so on a Monday "yesterday" is the previous week's Sunday and is
  never rendered. Found on 2026-07-27, a Monday. Two others in the same file are
  sensitive to rows left by earlier runs. Planner work is outside this sprint's
  scope; the honest fix (pinning the clock) is on the WI.

Both handoff specs were also hardened to click "All projects" first — they seed
project-less work items, and the page lands on whichever project was touched
most recently, so on a busy database the seeded row was filtered out and the test
failed for reasons that had nothing to do with handoffs.

## Deploy

`deploy-kubsdb` after merge. **Migration 0018 runs at startup.** Rehearse it
against a restored nightly dump first (`docs/operations.md`, "Rehearsing a data
migration") — the mapping names all 32 projects that existed on 2026-07-26, and
the interesting number is any project created since, which converges to NULL and
is reported by the migration's own `RAISE NOTICE`.

Rollback is a clean image re-tag: 0018 changes data in an existing column, not
schema, so the previous image reads the same database.

## Deployed 2026-07-27

- **Image**: `korg:ac8dd731b6bb` — revision
  `ac8dd731b6bb510201206ece18548e24839641be` (the squash-merge of PR #29).
  Confirmed live: the container's `org.opencontainers.image.revision` label
  matches `git rev-parse HEAD` exactly.
- **Rollback target**: `korg:a325ace77648` (sprint 027). 0018 is data-only, so
  an image re-tag fully reverts — no dump restore needed.
- **CI**: green on PR #29 (rust 3m48s, web 34s).
- **Rehearsal skipped** at Ken's call, on the strength of a pre-deploy read of
  the live project list: all 32 production projects were named in the mapping,
  so no project had been created since it was written and the migration had no
  unmapped rows to converge to NULL.

**Verified live:**

- `post-deploy-check.sh --compare`: **OK**. Every row count unchanged
  (work_items 474, cards 28, links 4, proposals 85, reports 23, projects 32),
  `node_count` 620 and `node_id_seq` 703 both stable. Schema moved exactly one
  step: `migrations 17 → 18`.
- **The true-up landed exactly as predicted**, project for project — Ops 8,
  AI 8, Dashboard 5, Infrastructure 5, Fun 3, EVAL 2, Other 1 = 32, with **zero
  NULL and zero off-vocabulary**. The nine `ai` / one `AI` / three `tooling` /
  two `infra` / two `fun` / fifteen NULL of the prior corpus are gone. The
  migration emitted no `RAISE NOTICE`, which is the correct signal that nothing
  was left uncategorised.
- **#621's route works on a cold URL**: `GET /handoffs/619` → 200, so the
  adapter-static SPA fallback serves the first dynamic route korg has ever had.
  This was the one thing that could have passed every local check and still
  broken in production, which is why the e2e spec asserts a direct visit rather
  than only the in-app navigation. `GET /api/handoffs/619` returns the full
  payload (2689-char body plus its `related` block); `/api/handoffs/999999`
  still 404s, so the error contract is intact. `/plan` still 200s.

#583 and the rail's colors and grouping are client-side and were not
smoke-testable by curl; they are covered by `filter-dismiss.spec.ts` and by the
category data above, and get their real look in the #622 session.

Proposal `korg:623` deliberately stays **active** — #622 and #570 are still
open under it, and `.korg-sprint-proposal` is left in place so the polish
session's ship closes it out.

# Sprint 027 — quick wins: correctness, CI, deploy tooling

Proposal `korg:624`, covering WIs #578, #579, #580, #584. A no-interaction sweep
of small fixes cleared ahead of the UI sprint (`korg:623`) and Agent Space. No
migration; ships as a normal rebuild.

`#570` was **moved out** to `korg:623` on 2026-07-24: its title reads "review
helper" but its body specifies a new *Review completed* page whose width
behaviour explicitly wants interactive tuning — UI work, not an ops sweep.

## What each item turned out to be

Three of the four differed from their WI description in ways worth recording.

### #579 — the bug was already half-fixed; the work was the fence

Sprint 020 had already corrected both `docs/api.md` and the MCP
`server_instructions`, so the false claim an agent could read was **gone before
this sprint started**. What remained was the contract question, and Ken decided
it (2026-07-24): the five unpaginated reads **stay bare arrays** — `total` would
always equal `items.length`, `limit` would never clip, and enveloping would touch
both transports, the generated TS and every web caller to say nothing new.

So the deliverable is the fence, in two layers:

- `docs_drift.rs::the_collection_read_shapes_agree_across_docs_and_instructions`
  — pure text, no DB: api.md's four-shape table and the server instructions must
  classify the same reads identically, so fixing one text and forgetting the
  other fails the build.
- `dispatch.rs::collection_reads_return_the_shape_the_instructions_promise` — the
  stronger one: parses the instructions' two groups, asserts **every** advertised
  `list_*`/`survey_work_items` tool is classified (a new collection read cannot
  ship undocumented), then dispatches each and checks the observed wire shape.

Mutation-tested before being accepted: claiming `list_reports` is paginated fails
with `` `list_reports` is documented as paginated but returned [...] `` — the
exact sprint-020 bug, named. api.md's "whether they should be enveloped is open"
line is replaced with the decision.

### #584 — the bug had already bitten this repo, twice

Item 1 was one line (`docker save korg:latest "korg:${REV:0:12}"`), but checking
the live host showed the damage was **not hypothetical**: `korg:24495eb83ef8`
(sprint 025) and `korg:893e4bbe6069` (026) were absent from kubsdb even though
both sprint READMEs name a `korg:<sha>` rollback target — 026's names 025's
image specifically. The documented rollback path did not resolve.

The images survived as dangling layers and each carries its own
`org.opencontainers.image.revision` label, so the names were recoverable.
Retagged from those labels; kubsdb now holds 8 named images (018–026) and every
rollback target recorded in a sprint README resolves again. The recovery loop is
written into the skill's Rollback section.

Item 2 added a schema section to `post-deploy-check.sh`, over SSH
(`KORG_DB_SSH`, default kubsdb): migration count + max version, `node`
min/max/count, `node_id_seq` state. Reported like the row counts and **failing**
on the two never-benign directions — a dropped migration or a rewound sequence.
Optional by design: it skips with a note when ssh is unavailable, so the script
still runs against a local instance, and the baseline format
(`{counts, schema}`) stays readable for pre-#584 baselines via `.counts // .`.
Both regression paths mutation-tested; forward movement stays quiet.

### #580 — two causes, and the recorded diagnosis needed refining

The WI blamed a tall Backlog column pushing the drop point off screen. Measuring
found that is real but incomplete, and that a second, unrelated cause was the one
failing the WI's own acceptance command:

1. **A race, not a hard miss.** `add()` posts no rank, so a UI-created card takes
   the default `rank: 0`, ties with every other card and sorts *last* — the
   bottom of Backlog, not the top the old comment claimed. Scrolling it into view
   leaves the target drop zone far above the viewport (measured at 176 Backlog
   cards, 720px viewport: grab y=651, target y=**-11709**, scrollY=12133). It
   still passes sometimes because svelte-dnd-action auto-scrolls toward a pointer
   held at the viewport edge — so the drop races the library's rAF against the
   test's fixed waits. That is why it read as flaky rather than broken.
2. **Colliding titles.** `drag ${Date.now()}` is unique only if no two runs share
   a millisecond, and `--repeat-each=5` runs its copies in parallel; two
   identically-named cards make the locator ambiguous before the drag starts.
   Reproduced: 2 of 3 parallel repeats died this way, 3 of 3 sequential passed.

Fixed by seeding the card over the API with `rank: -1` (top of Backlog, level
with the target, no scrolling and no race at any board size) and making the title
unique per worker/repeat. A geometry assertion stays as a tripwire — it is what
caught the -11709 in the first place. Rejected: the keyboard interface (the tile
deliberately consumes Enter/Space for the edit modal — a sprint-019 a11y
decision), clamping into the target box (when scrolled down it is entirely off
screen, so there is nothing to clamp to), and filtering the board (that panel
renders only in the table view). Acceptance verified at 62 **and** 187 cards.

### #578 — routine, but only CI can confirm it

`actions/checkout@v4 → v7`, `pnpm/action-setup@v4 → v6`,
`actions/setup-node@v4 → v7`; all three declare `using: node24`, which is what
clears the warning. `Swatinem/rust-cache@v2` already did — which is why it was
never named in it. Verified before bumping that `pnpm/action-setup` still takes
`package_json_file` (dropping it is how the workflow failed on its first run) and
that setup-node v7 keeps `node-version`/`cache`/`cache-dependency-path`.

Its acceptance — "a CI run's log contains no Node deprecation warnings, both jobs
green on a PR and on main" — can only be observed on a real run, so it is
confirmed at ship time rather than locally.

## Verification

`just check` green (43 suites, fmt/clippy/gen-check/web-check). The two new
fences and both schema-regression assertions were mutation-tested — each was made
to fail on purpose before being kept. The e2e acceptance ran against a throwaway
187-card board.

## Deploy

`deploy-kubsdb` after merge. **No migration** — a normal rebuild, and rollback is
a clean image re-tag to `korg:893e4bbe6069` (sprint 026). This sprint's own
deploy is the first to exercise the fixed `docker save` (both tags reaching the
host) and the schema baseline end to end.

## Deployed 2026-07-25

- **Image**: `korg:a325ace77648` — revision
  `a325ace7764864a42ebe2829db57d5459c237eaa` (the squash-merge of PR #28).
- **Rollback target**: `korg:893e4bbe6069` (sprint 026). No migration, so an
  image re-tag fully reverts.
- **CI**: green on PR #28 (run 30145048213) and on main (run 30145167065).
- **#584 proved itself in flight.** `docker save` reported `Loaded image:
  korg:latest` **and** `Loaded image: korg:a325ace77648`, and
  `docker image inspect korg:a325ace77648` on kubsdb resolved with no manual
  step — the first deploy for which that is true. The schema section also ran
  for the first time on both ends of a real deploy.
- **#578 verified on both surfaces** (branch-green does not imply main-green):
  zero "target Node.js / forced to run on Node" matches in either full log. Two
  unrelated deprecation lines remain and are named on the WI — Vite's CJS API
  (from svelte-check) and `punycode` DEP0040 (inside rust-cache's own process);
  neither is an action runtime.
- **Verified live**:
  - `post-deploy-check.sh --compare`: **OK** — every row count stable *and*
    schema stable (migrations 17, `node_id_seq` 626 unchanged, which is the
    correct result for a sprint with no migration).
  - #579's fenced contract holds against production: the live MCP `instructions`
    classify exactly the documented five paginated / six bare reads, and
    `/api/work-items` + `/api/cards` answer with envelopes while
    `/api/proposals`, `/api/reports` and `/api/projects` answer with bare arrays.

This sprint changed no runtime code — the fences, deploy tooling, e2e spec and CI
config are all build- and ops-side — so the live checks confirm the deploy is
faithful rather than exercising new behaviour.

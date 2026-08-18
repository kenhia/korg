# 066 — korg-native full-text search

**Proposal:** korg:1395 · **Items:** #1177 (M) · project `korg`.

Port khound's korg lexical engine in-house. khound stopped at Gate A on
2026-08-09, but its korg leg was the part that worked, and search is substrate:
the korg+ guiding plan's sweep of 1395 says *proceed — search is substrate,
korg is the right home*, with one constraint carried into design (below).

**Kickoff decisions (Ken, 2026-08-18):** full scope — engine, web search box,
**and** the MCP tool in one pass; and the engine choice gets *measured* before
it is taken, per #1177's own instruction.

**Guiding plan:** korg is listed in `cross-project-planning/index.md` → `korg+/`.
Two decisions bind this sprint: **GP-1** (search is korg's, consumers read it —
kfdc/kfo/korg-dash never grow their own index) and the 1395 sweep row (an MCP
search tool must honour the 2026-08 agent-surface read contract: lean rows, an
honest `omitted`/truncation envelope, bounded fields). No `GP-n` needed amending
at kickoff; re-checked at close-out.

## The engine decision: Postgres FTS, measured

#1177 framed this as *embedded tantivy (the ported, ranking-proven code) vs
Postgres FTS (no new dependency)* and said measure before assuming. Measured.

The arbiter is the frozen suite `kubs0:~/khound-eval/suite-002.toml` — 12
korg-content queries over 11 distinct nodes, keys authored from the corpus, not
harvested from any tool's ranking. The recorded reference numbers from khound's
`sprints/review/gate-a-second-run.md`, metric **korg-content top-3**:

| tool | korg-content top-3 |
|---|---|
| khound / tantivy (post-fix) | 10/12 |
| korg listing (title-only scan) | 9/12 |

Rehearsed against a fresh production dump (2026-08-18: 2,092 documents — 1,312
nodes plus 783 comments), document set = every node kind's title+prose plus one
document per comment, `setweight` A on locator+title and B on body,
`websearch_to_tsquery` with the AND→OR relaxation. The only free variable is
the ranking function, so all six standard options were swept:

| ranking | top-1 | top-3 |
|---|---|---|
| `ts_rank_cd(tsv,q,32)` (the obvious default) | 4/12 | 7/12 |
| `ts_rank_cd(tsv,q,1\|32)` | 6/12 | 9/12 |
| `ts_rank_cd(tsv,q,2\|32)` | 2/12 | 5/12 |
| `ts_rank(tsv,q)` | 10/12 | 12/12 |
| **`ts_rank(tsv,q,1\|32)`** | **12/12** | **12/12** |
| `ts_rank(tsv,q,2\|32)` | 2/12 | 3/12 |

**Decision: Postgres FTS**, ranked `ts_rank(tsv, q, 1|32)`. It beats the
ported engine's recorded number on the same frozen suite, adds no dependency,
and — the part that changes the design rather than just the score — it deletes
a whole subsystem #1177 budgeted for. Search stops being *derived state*: a
persisted tsvector is maintained by Postgres inside the writing transaction, so
there is no rebuild story, no refresh trigger, no `get_board` change detector,
no staleness to report. #1177's "index is derived state; delete and rebuild is
the recovery story" is superseded by "there is no index to recover".

**Why normalization `1` is the whole result, not a knob.** The default
`ts_rank_cd` has no length term and no IDF, so under the OR relaxation — which
is where every prose query lands — long documents win on word count alone; one
sprawling work item (WI-826) took the top slot for four unrelated queries.
Normalization `1` divides by `1 + log(length)`, which is Postgres's stand-in
for BM25's length normalization, and that is exactly the term the relaxation
needs. (`32` only rescales to 0..1; it cannot reorder.)

### What the 12/12 does not mean

The parameter was swept against all 12 suite queries **including its 4
holdouts**, so the suite's own holdout split carries no independent signal here.
An independent set was authored to replace it: 5 queries about korg's history
whose keyed nodes were established from documents read directly
(`crates/korg-core/src/repo/mod.rs`'s split note → #1345, the guiding plan's
substrate queue → #1411/#1414) and whose wording deliberately avoids the node
titles. On that set the chosen ranking scores **3/5 top-3, 1/5 top-1** — still
dominating `ts_rank_cd` (0/5) and plain `ts_rank` (2/5), which is the comparison
the parameter choice rests on, but a long way from 12/12.

The gap is the honest reading of the suite: its queries were written by someone
looking at the target node, so they share its vocabulary. Both misses are pure
vocabulary mismatch — "why was the giant repository query file broken up" does
not lexically meet "Split repo.rs into a repo/ module directory". That is the
documented ceiling of a lexical engine, and it is the same wall khound hit from
the other side; it is not a defect to fix in this sprint. **Recorded so the
next person does not read 12/12 as the real-world hit rate.**

### Two things the rehearsal settled in passing

- **On-the-fly `to_tsvector` is not an option.** 403 ms for one query over 2,092
  documents, growing linearly — it fails khound's own p50 < 300 ms bar at
  today's corpus size. The tsvector must be persisted and GIN-indexed.
- **The acceptance run must search *everything*.** All 12 keyed nodes are
  closed or resolved. #1177's "not-done-by-default" scoping is a **UI default**;
  the suite fails outright if that default reaches the engine.

## What shipped

**Migration 0029** — a generated, GIN-indexed `search_tsv` on each of the nine
tables that carry prose (the eight node-kind detail tables plus `comment`).
`attachment` is deliberately absent: a filename and a mime type are not a
document, and offering it as a filter would promise a search that can only
return nothing.

Two constraints shaped the SQL rather than the design shaping them. A
generation expression must be IMMUTABLE, and `concat_ws` and `date::text` are
only STABLE — hence `||`/`coalesce` throughout, and hence `report` indexing its
`source` rather than its `report_date` (the date is a filterable column, not
search text). And a generated column can only see its own row, which is why
`comment` derives *both* locator spellings from its own `node_id`: since the
0009 identity migration a work item's node id **is** its `wi_number`, so
`WI-836` reaches that item's comments with no join. For a comment whose parent
is not a work item the `WI-<id>` token names a locator that cannot exist — the
id space is shared, so nothing else can claim it.

**`repo::search`** — one query builder, one set of scope predicates shared
verbatim by the count and the page so the two cannot disagree about what was
searched. `total` is counted in its own statement, never `count(*) OVER()`
(#883).

**MCP `search` + `GET /api/search`** — same knobs, same rows, honouring the
2026-08 agent-surface read contract the queue sweep asked for: lean routable
rows (no bodies — the locator is the follow-up), the paginated envelope, and an
`omitted` cascade. Two fields no other read has:

- **`relaxed`** — whether this answer is the any-term fallback. This is the
  sprint's one genuine addition to khound's design, and it is a direct response
  to how khound failed: a query path that returned zero while reporting
  `ok, hits=0` for a whole gate run. An answer that cannot say whether it is
  strict or broad is the bug, not a cosmetic omission. `dispatch.rs` asserts it
  is present and boolean.
- **`parsed`** — the tsquery that actually ran, including the any-term form when
  `relaxed` is true. Reporting the parse that was *abandoned* would describe a
  search that did not happen.

**Web** — a search box in the **header**, not the nav. Search is something you
do *from* a page, not a place you go; a tab would have made it a destination and
put the box one click further from everywhere that isn't it. `/search` holds the
whole query in the URL, so a result set is linkable and Back works, and the
"search everything" toggle sets scope *and* archived together — "search
everything" that still hid archived rows would be a lie the UI told itself.
Hits open in the existing generic `NodePreview`, which already handles all nine
kinds, so nothing kind-specific was written.

### Decisions taken during the sprint

- **AND→OR relaxation as a text rewrite of the *parsed* query** (`&` → `|`),
  not a second Rust-side tokenisation. Stemming, stop-word removal and quoted
  phrases survive intact, and there is no way for the two tokenisers to drift
  because there is only one.
- **Exclusions are never relaxed.** `foo -bar` relaxed to `foo | !bar` matches
  every document *without* `bar` — the opposite of the request.
- **Scope is a filter, never a ranking term.** A not-done *discount* would make
  the same answers merely harder to find, which is worse than absent because it
  looks like working. khound's staleness-discount ideas stay dead, as #1177
  said.
- **A closed parent takes its comments with it.** Otherwise the default hides a
  decision and shows the argument about it.
- **`ts_headline` snippets**, not the leading 320 characters khound returned.
  The fragment is centred on the matched terms, so it often carries the answer
  outright — the eval charged a follow-up read whenever it didn't.

### The nine tests

`crates/korg-core/tests/sprint066.rs` pins the contract, not the ranking. The
load-bearing one is `the_relaxation_reaches_every_filtered_path`: it runs the
same prose query unfiltered, kind-filtered, project-filtered and both, and
requires the item back from each. "korg has one query builder" is a claim, and
that test is the evidence for it — khound's Gate A M3 failure was exactly this
claim being false.

`web/tests/e2e/search.spec.ts` covers the header box and the relaxed banner;
`/search` joined the a11y route floor.

## Verification

Rehearsed against a **fresh** production dump (2026-08-18), restored into a
throwaway Postgres and migrated by `korg-api` itself — so 0029 was applied to a
real corpus, not a test fixture, and the run below went through the whole stack
over HTTP.

- **Migration:** clean apply, `_sqlx_migrations` 28 → 29, nine `search_tsv`
  columns.
- **Acceptance (`scope=all`, the suite's setting):** **korg-content top-1
  12/12, top-3 12/12** — against khound/tantivy's recorded 10/12 top-3 and the
  title scan's 9/12.
- **Latency, end to end over REST:** p50 **11 ms**, p95 **47 ms** against
  khound's own gate of p50 < 300 ms / p95 < 1.5 s. (On-the-fly `to_tsvector`
  was 403 ms for the same query, which is what made the persisted column
  non-optional.)
- **Snippet contract:** spot-checked — the `korg2-reset-blast` snippet returns
  the `TRUNCATE node, project, area RESTART IDENTITY CASCADE` clause verbatim,
  answering the question without a follow-up read.
- **Default scope on the same corpus:** 65 live hits with
  `omitted {terminal: 437, archived: 3}` — the envelope doing its job.
- `just check` green; 81/81 e2e green on a fresh database.

### One pre-existing flake, fixed in passing

The full e2e run surfaced a latent strict-mode violation that is not this
sprint's: `destructive-confirm.spec.ts` used
`getByRole("button", { name: "Go" })`, and Playwright matches an accessible
name by case-insensitive **substring** by default. The Work Items page renders
one button per project, so any project whose name contains "go" — the schedules
suite files `e2e-due-pill-gone-…` in parallel — makes the locator ambiguous. It
passed only while that project happened not to exist yet. Fixed with
`exact: true`.

The header search box collided with the same class of thing: its placeholder was
`Search…`, which `filter-dismiss.spec.ts` matched alongside the Work Items
filter box. Renamed to `Search korg…`, which is clearer for a human too — a
page-level filter and a corpus-wide search should not read as the same control.

## Follow-ups

- **Vocabulary mismatch is the ceiling, not a bug** (`3/5` on the independent
  set). Bridging it means semantics, not a ranking knob; nothing is filed,
  because khound already established that the lexical leg alone does not get
  there and that the next step is a different kind of index.
- `attachment` filenames are unindexed. Deliberate, and trivially reversible if
  "which image was that" ever becomes a real question.

## Deployed

**2026-08-18** — `kubsdb.encke-wahoo.ts.net:5674`, image
`kubsdb.encke-wahoo.ts.net:5000/korg:4c7d0249627f`
(`org.opencontainers.image.revision=4c7d0249627fa340fedd0a22bedcb53a2659e7c6`,
the squash-merge of PR #71). Rollback target: `korg:e292e855fed4` (sprint 065),
confirmed present in the registry before the build.

Backups verified current beforehand — `korg-20260818-031940.sql.gz`, 1.59 MB,
larger than the previous night's — which matters more than usual here, because
a rollback across 0029 is a schema boundary and needs a dump restore rather than
a re-tag.

**Migration 28 → 29, and the counts prove it was DDL-only.**
`scripts/post-deploy-check.sh --compare` diffed every tracked count against the
pre-deploy baseline: cards 30, links 4, projects 50, proposals 250, reports 43,
work items 945, nodes 1312 — **all unchanged**, `node_max` and the sequence
still 1414. The only deltas were `migrations 28 → 29` and `migration_max
28 → 29`. Schema landed as declared: nine `search_tsv` columns, nine
`*_search_idx` GIN indexes. Database 15 MB → **23 MB**, matching the migration
header's stated cost (a tsvector is the same order of size as its document).

Verified live over the tailnet FQDN, not just `/api/health`:

| check | result |
|---|---|
| Identifier `WI-836` | **strict** (not relaxed), 10 hits — `korg:836#comment-441` and `WI-836` itself at ranks 1–2 |
| Prose: *"what does korg-migrate reset actually truncate"* | **relaxed**, `WI-528` at rank 1 — the suite's keyed answer |
| …its snippet | carries `TRUNCATE node, project, area RESTART IDENTITY CASCADE` verbatim — answers without a follow-up read |
| Default scope | 1 live hit with `omitted {terminal: 20, archived: 0}` — the envelope reporting what it hid |
| MCP `search` over `POST /mcp` | 3 hits, `relaxed:false`, `omitted` present — the agent transport, not just REST |
| `/search`, `/`, `/work-items` | HTTP 200 |
| Header box in the **deployed** bundle | `Search korg…` + its `sr-only` label found in the layout chunk; the page chunk carries `Search everything` |

The revision assertion in the deploy script passed, so the running container is
provably the commit that was built — the check that catches a `compose pull`
which silently did nothing.

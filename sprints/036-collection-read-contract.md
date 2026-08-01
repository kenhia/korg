# 036 — The collection read contract: proposal object shape, work-item read tiering

**Proposal:** korg:866 (rank 4.3) — covers **#860** and **#861**.
**Program:** agent-surface (`sprints/planning/2026-08-korg-agent-surface.md`),
§4 rank 3 — *the real work*, run after 817 made the e2e suite trustworthy.

## Goal

Generalize 816's proven three-tier read contract (§2 of the program plan) past
projects, to the two collections that still violate it.

- **#860** — `sprint_proposal.summary` holds the entire analysis. Proposals are
  written as plans, so summaries are the longest prose in korg. A bounded
  routing contract, an unbounded `notes` beside it, and a migration that
  preserves every character.
- **#861** — `list_work_items` returns full `content`+`details` for every row
  and hides nothing terminal, while `update_work_item`'s schema claims `closed`
  is "hidden by default". One lean, terminal-excluded, `omitted`-enveloped read;
  `survey_work_items` retires.

## The measurement this sprint starts from

`GET /api/proposals` on the deployed instance, 2026-08-01 (119 rows, the whole
corpus including archived):

| status | n | sum chars | p50 | p90 | max |
|---|---|---|---|---|---|
| done | 97 | 142,188 | 1,286 | 2,728 | 4,711 |
| proposed | 17 | 40,604 | 2,303 | 5,389 | 5,710 |
| declined | 3 | 5,884 | 2,207 | — | 3,417 |
| active | 2 | 795 | — | — | 505 |

**96 of 119 rows exceed 500 characters.** Total summary payload 189,471 chars.

Work items, same instance: **597 rows**, of which **~78% are `closed`**; the
non-closed, non-archived count the Work Items page footer settled on in 035 is
**133**.

## Decisions

### D-1 — the cap is 500 characters

#860 recommended ≤500 and left the final bound to this sprint, "sized against
what `start-sprint`'s pick actually needs". After #852 the pick never sees
`summary` at all — the lean queue row is `node_id`, `title`, `status`,
`project`, `rank`, `pinned`, `covered_count`, `comment_count`. So the cap is not
sizing the pick; it is sizing the **full** row: `get_proposal`,
`list_proposals detail:"full"`, and the Planning card.

500 is what the three live rows this program authored were written to
(866: 505, 867: 487, 868: 499 — 866 is 5 over, which is the cap doing its job on
its own sprint). It also leaves the mechanical derivation below room to end on a
word boundary rather than mid-clause.

Enforced twice, deliberately: a CHECK constraint in 0021 so no writer can get
past it, and a length check in `korg-core` so the failure is `invalid_input`
naming the field and the limit rather than a raw constraint violation in a 500.

### D-2 — the migration derives, and derives from structure rather than length

`char_length`-truncating 96 summaries would produce 96 plausible-looking routing
lines nobody wrote — the failure 0019 refused for `kcard`'s path and 0020
refused for project descriptions. But proposals cannot take 0020's way out
(`description = NULL`, backfill later): `summary` is `NOT NULL` and the Planning
page renders it.

So the derivation reads the **first paragraph** — everything before the first
blank line — which is how these summaries are actually written: a lede that says
what the bundle is, then the analysis. Simulated against all 119 production rows
before writing the SQL, the ledes read as genuine summaries:

> *"Clear the entire open korg backlog in one pass — five small items, all in the
> korg repo. Leads with #392 (S, highest priority): …"* — #394, 828 chars → 497

Where the lede itself exceeds the budget it is cut back to a word boundary. A
` […]` marker is appended whenever anything was dropped, so a shortened summary
never reads as a complete one. Total: 189,471 chars → 43,859, and **every
original character is in `notes`, verbatim**.

The CHECK goes on **VALID immediately**, unlike 0019's — step 2 guarantees no
row can exceed it, so nothing is being tolerated and nothing needs deferring
(0020 §3's reasoning, verbatim).

### D-3 — `notes` on the full row, not the lean one, and the payload stays flat

Mirroring projects (#828): `get_project` and REST carry `notes`, the lean list
never does. `ProposalRow` gains `notes`; `ProposalLeanRow` does not.

The move is what keeps this cheap: the analysis *relocates* from `summary` to
`notes` rather than being duplicated, so `GET /api/proposals` — which the
Planning page deliberately fetches unfiltered (#622) — carries about the same
bytes it did before, redistributed. Same for `list_proposals detail:"full"`.
What changes is that the **default** MCP reads and the Planning card now see
500 chars where they saw 2,300.

### D-4 — live routing lines are a data pass, not node ids baked into a migration

#860 asks for real short contracts on the live (`proposed` + `active`) rows,
authored rather than derived. That cannot happen before the migration — the
analysis has nowhere to live until `notes` exists — and it must not happen
*inside* the migration, because `UPDATE … WHERE node_id = 866` is meaningless on
a fresh install and wrong on any restored dump.

So: the migration derives mechanically for every over-cap row, and the live
rows are re-authored **after** the deploy, over MCP, reading each row's `notes`
as the source. Recorded here so a sprint that ships without the pass is visibly
unfinished rather than quietly so.

### D-5 — one lean work-item read, with the same three-mode status filter proposals got

`repo::list_work_items_lean` is `survey_work_items`' projection plus the two
things it lacked:

| `wi_status` | rows |
|---|---|
| absent | `open` + `resolved` + `done` — everything not terminal |
| `"all"` | every status |
| one of the four | exactly that status |

Identical in shape to `proposal_status_predicate` (#852) and
`project_status_predicate` (#828), and the vocabulary split is the program
plan's §8 answer 1: `closed` is terminal; `resolved` and `done` stay visible
(`done`'s visibility is already a promise in `update_work_item`'s own schema,
`resolved` is Ken's may-want-to-see, and together they are a handful of rows).
`WI_LIVE_STATUSES`/`WI_TERMINAL_STATUSES` are fenced by the same
partition test #852 wrote for proposals.

`omitted` is `{closed, archived}`, computed as the same cascade: `archived`
counts what the archived filter hid, `closed` is counted only over rows that
passed it, so an archived closed item lands in `archived` and nowhere else.

### D-6 — no `detail: "full"` on `list_work_items`

`list_proposals` has one (#852 D-4) because a proposal's full row is one field
of prose and `full` had to keep something reachable. The program plan's §2 says
the opposite for the general case — *"No detail flag: if you want detail, you
want one row"* — and work items are why it says it: their full row is `content`
+ `details` across hundreds of rows, which **is** the payload problem. The full
tier is `get_work_item`, unchanged, and it already inlines comments and edges.

### D-7 — `survey_work_items` becomes a deprecated alias, not a deletion

#861's title says it retires; its body says keep it "for a deprecation window as
an alias if consumers need one, then delete". A consumer needs one: the
`refill-queue` skill calls it every run and lives in agent-skills, whose rework
is rank 4 (proposal 867, WI #864). Deleting the tool here would break a shipped
skill to save one catalogue row.

So it stays registered, dispatches to the identical read, and its description
opens `DEPRECATED`. It keeps its own paging defaults (limit 50, not 200) so
existing callers page exactly as they did. Deleting it is 867's follow-up, and
this record names it as such.

### D-8 — MCP goes lean; REST stays as it is

034's D-6 again, for the same reason and with a sharper edge. `/api/work-items`
serves the Work Items page, whose client-side search reads `content` and whose
has-details ticker reads `details` — dropping either from the REST row silently
breaks two features. 035 anticipated that "when #861 lands … the same walk
fetches 217 KB instead of 950 KB"; that is true only once a lean row carries a
`has_details` marker, which is exactly **#813's** work in proposal 825. #861
says to pay the row-contract cost once and coordinate with 825; this is the
coordination — we ship the tiering, 825 adds its markers to the same row, and
the web walk goes lean in that sprint rather than half-lean in this one.

The REST survey endpoint (`/api/work-items/survey`) does take the new defaults,
because it shares the core read. Safe: the Review page is its only caller and
passes `wi_status` explicitly per status.

### D-9 — the "hidden by default" sentence becomes true

`update_work_item`'s schema has claimed since sprint 010 that `closed` is
"hidden by default", and nothing on the MCP surface hid anything — 445 of 574
rows came back on every call. That is the same schema-vs-behaviour class as
#851/#852, found while premise-checking #861. D-5 makes the sentence true rather
than rewriting it to match the old behaviour.

## What shipped

**#860 — the proposal object shape**

- **Migration 0021** — `sprint_proposal.notes` (unbounded), every over-cap
  summary moved into it verbatim, the summary derived from the first paragraph
  with a ` […]` marker, and `sprint_proposal_summary_routing_line`
  (`char_length(summary) <= 500`) added VALID. Postcondition asserts invariants
  rather than counts, so it holds on a fresh install: no row over the cap, and no
  marked summary without `notes` beside it. Re-appliable by construction.
- `ProposalRow.notes`, `NewProposal.notes`, `ProposalPatch.notes`
  (double-option, so absent leaves it and `null` clears it) — plus
  `PROPOSAL_SUMMARY_MAX` and `check_proposal_summary`, which make an over-long
  summary `invalid_input` naming the cap and the remedy on both write paths.
- The node preview shows `notes` as its body where there is one, with `summary`
  demoted to a field — otherwise the preview would render the shortest form korg
  holds and nothing else.
- The Planning card gets a **Notes** disclosure. The row already carries the
  text, so the long form is one click away rather than one fetch.

**#861 — the work-item read tiering**

- `repo::list_work_items_lean` + `WorkItemListLean` + `WorkItemOmitted`, with
  `wi_status_predicate` mirroring `proposal_status_predicate` exactly.
  `repo::survey_work_items` is gone; `repo::list_work_items` stays as the REST
  read and says so in its doc comment.
- `vocab::WI_LIVE_STATUSES` / `WI_TERMINAL_STATUSES` + the
  `wi_statuses_partition_cleanly` fence.
- `ops::ListWorkItems` gains `wi_status` (vocabulary + `"all"`);
  `SurveyWorkItems` survives only to keep the alias's paging defaults and
  converts into it.
- MCP dispatches both names to one read. `update_work_item`'s status sentence
  now names the filter that makes it true.
- REST `/api/work-items/survey` shares the core read (so its no-status default
  narrows too); `/api/work-items` is untouched.

**Contract texts and their fences**

- `server_instructions()`: both work-item reads named as carrying `omitted`, and
  the `closed` default stated.
- `docs/api.md`: catalogue deprecation note (two tools became three), shape-table
  parenthetical, `omitted` prose, the filters row, a new "The work-item list is
  lean and terminal-excluded" section, and a new proposal object-shape section.
  `docs/usage.md`: the survey's REST semantics, the #861/#860 paragraphs beside
  #852's, and the `sprint_proposal` kind description.
- `dispatch.rs` asserts `omitted: {closed, archived}` on **both** names rather
  than only classifying them — asserting per name is what would catch the alias
  being wired to a different read.

**Tests** — `korg-core/tests/sprint036.rs` (5): the migration applied to a
seeded over-cap corpus, asserting the derived summary is a *prefix* of the
original rather than a paraphrase and that the cut lands on a word boundary;
the cap rejected on both write paths, including the exactly-at-cap case; `notes`
round-trip and clear; the `omitted` cascade proved by arithmetic
(`shown + closed + archived == everything`, which is 7 not 6 if an archived
closed row is double-counted); status validation. `korg-mcp/tests/server.rs`
(2): the lean default over the wire — projection, `omitted`, the alias returning
byte-identical results with its own `limit` — and the proposal tiering, with the
cap asserted as an `invalid_input` naming the remedy.

`just check` green: fmt, gen-check, web checks, clippy `-D warnings`, 44 suites.

## Rehearsal — against a dump taken minutes before, not last night's

`docs/operations.md`'s standing recipe, on a fresh `pg_dump` of production
(2026-08-01 22:5x) restored into a throwaway Postgres 18 container. Last night's
nightly would have missed every row this program filed today, including the five
proposals whose summaries were written to the shape being enforced.

| | before | after |
|---|---|---|
| proposals | 119 | 119 |
| summaries over 500 chars | **96** | **0** |
| total `summary` chars | 189,471 | **43,858** |
| rows with `notes` | — | **96** |
| marked summary with no `notes` (`lost`) | — | **0** |

Then the assertion that matters, checked row by row against the pre-migration
corpus fetched from the live API: **all 96 moved rows have `notes` byte-identical
to their original summary, all 23 untouched rows are unchanged, every derived
summary is a prefix of its original, and the total character count is
preserved exactly.** Nothing lost, nothing invented.

Run twice: once through `psql` (the SQL), once through the **real sqlx
migrator** by booting `korg-api` against the restored dump, which recorded
version 21 `success=true` and emitted the migration's own NOTICE. The second run
is what proves it applies as a numbered migration to a database already holding
1–20, not just as a script.

Sampled live rows read as genuine summaries — 866 itself derives to 499 chars,
losing only its closing "coordinate the row contract with 825" clause, which is
the cap biting its own sprint exactly as D-1 predicted.

## Deployed 2026-08-01

- PR **#40**, squash-merged as `9c452bc`. Image `korg:9c452bcf933f`
  (`org.opencontainers.image.revision` confirmed on the running container).
  Rollback target: `korg:a81bfd8ba179` — but note 0021 is a schema boundary, so
  see `docs/operations.md` before crossing it backwards.
- `post-deploy-check.sh --compare`: every row count identical to the pre-deploy
  baseline (work items 597, cards 29, links 4, proposals 119, reports 23,
  projects 36; `node_count` 782, `node_max` 869). **Migrations 20 → 21** — the
  first schema change this program has deployed.
- The migration announced itself in the container log at startup: *"proposal
  analysis moved to notes on 96 rows"*.
- Backups current at deploy time: `korg-20260801-032006.sql.gz` (685 KB, larger
  than the prior night's 589 KB), timer active.

### Verified live, against production data

**#860 — the migration on the real corpus.** Re-ran the rehearsal's row-by-row
comparison against the deployed instance, using the pre-deploy fetch as the
reference: 119 rows, **96 moved, 23 untouched, zero problems**. `summary`
189,471 → 43,858 chars, and every original character is preserved — each moved
row's `notes` is byte-identical to its old summary, and each derived summary is a
prefix of it.

The contract is live and it rejects rather than truncates:

```
update_proposal(866, summary: <501 chars>)
→ isError, code invalid_input
  "proposal summary is 501 characters; the routing contract caps it at 500.
   Put the analysis in `notes` — it is unbounded, and that is what it exists for."
```

`get_proposal(866)` returns `summary` 499 / `notes` 505; the lean queue row
carries neither field.

**#861 — the lean read over MCP**, against 597 work items:

| call | `total` | `omitted` | payload |
|---|---|---|---|
| `list_work_items` (default) | **133** | `{closed: 447, archived: 17}` | **27,671 bytes** |
| `list_work_items` `wi_status:"all"` | 580 | `{closed: 0, archived: 17}` | — |
| `survey_work_items` (alias) | 133 | identical, `limit: 50` | — |
| the old equivalent — `GET /api/work-items?limit=500` | — | — | **984,779 bytes** |

**35.6× smaller**, and the cascade adds up exactly: 133 + 447 + 17 = 597, the
whole corpus. 133 is also the number 035 verified the Work Items page footer
computing independently, so the two surfaces now agree on what "live work" means.
The row keys are exactly the lean projection — no `content`, no `details`.

**D-4's data pass, done.** All 12 live proposals that had derived summaries were
re-authored from their `notes` over the REST write path (the same core function,
so the cap applied): **0 of 18 live rows still carry a `[…]` marker**, the
longest live summary is 499 chars, and a re-check confirms no row's `notes`
drifted from its original in the process. #744 — the one whose derivation came
out at 135 characters — now has a real contract.

## Follow-ups

- **Delete `survey_work_items`** when agent-skills #864 (proposal 867, rank 4.4)
  moves `refill-queue` onto `list_work_items`. Tool, `ops::SurveyWorkItems`,
  catalogue note and the `From` impl go together.
- **825 carries the row contract from here.** #813's `has_handoff`, in-proposal
  and has-details markers belong on `WorkItemSummary`; when the has-details
  marker exists, `/api/work-items` can go lean and 035's expectation that the
  Work Items walk drops from 950 KB to 217 KB comes true (D-8). One contract
  revision, one `just gen`.
- **`refill-queue`'s summary guidance is now wrong** — it tells the writer to
  mirror "the tone of the existing proposals' summaries", which after this means
  *short*. Rank-4 consumer work, flagged in #860 and unchanged by this sprint.
- **The 23 untouched proposals have no `notes`**, and that is deliberate:
  the column means "there is more", not "there was". Nothing needs backfilling.

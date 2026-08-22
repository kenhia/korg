# 072 — parked proposals and parked programs

Proposal korg:1546, slice 1 of program korg:1549. Covers #1534 (M) and
#1535 (M). Branch `072-parked-proposals-and-programs`.

## Goal

korg had no way to say *blocked on something with no end date* above the
work-item level. The evidence was sitting in the corpus: korg:1478 had
been `active` for a month because kai's 5090 was out for RMA, carrying a
comment asking people not to pick it up from Planning, and program
korg:1480 was `holding` — true about the motion, wrong about the reason.
`declined` was the only alternative the vocabulary offered and it asserts
the opposite thing, a decision *not* to do the work.

A comment doing a status's job is the tell that a status is missing.

#810 had already answered this for work items and the answer was not
reopened: `parked` is **live, not terminal** — visible, sorted below a
divider, never hidden by korg. This sprint carried that to two more node
kinds. Both work items insisted the other be read first, because they
share one decision (how a parked row reaches `get_board`) and answering
it twice would have meant kfdc writing two filters and GP-19 being wrong
the day it was written.

## The two decisions the work items demanded

**The board shape.** Parked proposals ride at the end of `queue`; parked
programs at the end of `programs`. Not a third bucket. The argument
against one is that it makes every consumer merge two lists to render the
*ordinary* case correctly, in order to describe a row that `status`
already describes — and `BoardProposal.status` was already serialized, so
kfdc's filter costs it nothing. A parked row that was `active` still
lands in `queue`, not `active`: On Deck is "what could be picked up", and
below its divider is where "not this one" belongs. Leaving it in Fire
Missions would be the board claiming work is underway on a row that
cannot move, which is the failure `queued` was added to fix one level up.

**The proposal migration.** `sprint_proposal.status` had been a real PG
enum since 0008, so the small diff was `ALTER TYPE … ADD VALUE`. It was
costed rather than defaulted, as #1534 asked, and the conversion to
`TEXT` + CHECK won on three counts:

| | `ADD VALUE` | convert to TEXT + CHECK |
|---|---|---|
| code diff | none | **one line** — a single write site drops `$2::sprint_proposal_status` |
| next status | a second spelling to remember | the same widen programs already use |
| authority | enum is a second one | `vocab` alone (#526), DB as backstop |
| transactions | restrictions under `sqlx::migrate!` | none |

The measurement that decided it: every *read* site already casts
`status::text` — they had to, to talk to the enum — so the conversion is
invisible to all of them. `grep sprint_proposal_status` found exactly one
write. The "bigger migration" was one line of Rust.

0031 is DDL only. `parked` is a value nothing has yet, so unlike 0030
there was nothing to backfill and no row changed meaning.

## The subtle one: declared vs derived

#1535's real content, and the reason it is not the proposals WI again.
`queued` is **derived** — korg maintains it as a fact about the slices
and promotes out of it on three write paths. `parked` is **declared**.
They can disagree, and where they do the declaration stands.

Two things had to be decided rather than discovered:

- **Nothing promotes a parked program.** A slice starting underneath must
  not lift it; a parked program may legitimately hold `active` slices.
  No clause was added — `promote_queued_programs_over` gates on `queued`,
  so every other status is already excluded — but "it falls out" is not
  something a comment should assert on its own, so
  `starting_a_slice_never_unparks_its_program` exercises all three
  promotion paths against the database.
- **`parked` is not a *started* status.** This one is a genuine fork. A
  proposal can be parked from either live status, so the literal cannot
  say whether the work began. Admitting it to `PROPOSAL_STARTED_STATUSES`
  would promote a program whose only slice went `proposed → parked` — an
  ACTIVE callout on work nobody has touched, #1424 verbatim. Excluding it
  costs nothing the other way, because the promotion is one-directional:
  a slice that went `active → parked` promoted its program on the way
  through and korg never demotes behind that.

  The honest residue, named in `vocab.rs` rather than papered over: a
  program created around an already-parked slice is born `queued` whether
  or not that slice once ran. korg cannot tell from the status, and
  reading the transition log to guess would be korg inventing history.

**Un-parking is not korg's to guess**, and this is where the design
reaches the UI. The status a row was parked out of is not recoverable
from the row — a proposal parked out of `proposed` and one parked out of
`active` are indistinguishable afterwards. So the tool requires a target,
and the Planning card offers *Resume* and *To queue* as two buttons
rather than one "Unpark" that would have to pick. The rollback notes in
0031 say the same thing to a future operator: reverse from the
`transition` rows, not from a plausible default.

## What the fences caught

Four failures, all before any of the code they guard was touched, and
each one is the argument for the fence existing:

| fence | what it caught |
|---|---|
| `proposal_statuses_partition_cleanly` / `program_statuses_partition_cleanly` | the new value unclassified — the fifth status with `holding`'s ambiguity, exactly what the program fence's comment predicted |
| `the_membership_predicate_matches_the_proposal_vocabulary` (sprint 042) | two SQL fragments in `concat!`ed consts still spelling the live set as `('proposed', 'active')` |
| `Record<ProgramStatus, string>` in two Svelte pages | `parked` reaching the UI with no palette treatment |
| `the_usage_vocabulary_list_matches_vocab_rs` | `docs/usage.md`'s two vocabulary bullets |

The sprint-042 tripwire is the interesting one, because it forced a
decision rather than an edit: **does a parked proposal still *claim* the
work items it covers?** It does. The marker answers "is anything already
planning this item?", and a parked bundle is a deferred plan, not an
abandoned one — that is the whole distinction from `declined`. Reading it
as unclaimed would invite a curator sweeping for uncovered work to bundle
the item into a second proposal, leaving korg holding two plans for it
with no way to tell which was meant.

The `Record<ProgramStatus, …>` failure is worth noting as the *good*
version of kfdc #1444: korg's own pages key their palette by the
vocabulary, so a new status is a compile error rather than an undefined
class. `parked` got a treatment — slate, quieter than everything else and
nearest `done`, but keeping a hue where `done` has none, because dormant
is not finished and the two must not converge at a glance.

## Also in scope, easy to forget

- **`awaiting.rs`'s ghost-free lists.** `parked` is not terminal, so an
  awaiting marker on a parked row persists — parking defers the work, it
  does not answer the question. That was already true by construction,
  but two of the three exclusion lists were hand-written pairs
  (`('done', 'declined')`, `'done'`) beside a card clause that already
  read its vocabulary constant. They now all read `vocab`'s terminal
  sets. A hand-written status list is precisely how `parked` got missed
  one vocabulary over.
- **Deconfliction (#978).** Unfinished is derived per kind from the
  terminal sets, so a parked blocker still blocks. Confirmed with a test
  rather than assumed.
- **Ordering.** `parked_last` is one helper in `repo/common.rs`, used by
  both proposal queue reads, the board and `list_programs` — because a
  consumer that saw parked last in `list_proposals` and interleaved on
  the board would have to write its own sort to reconcile them, which is
  the derivation GP-19 forbids. It sorts **outside `pinned`**: the divider
  is absolute, and pinning orders within a half. A pinned parked proposal
  is "this one first, when it comes back", not "this one now".
- **Search.** `search.rs` hides each kind's terminal rows, so parked stays
  findable by default. Correct already; pinned with a test, because the
  predicate is literals inside a `const` raw string and the failure mode
  would be a later tidy-up making deferred work unfindable exactly when
  somebody goes looking for it.
- **One literal.** `vocab::PARKED_STATUS` is spelled once and referenced
  by all three vocabularies, fenced by `parked_is_one_literal_everywhere`.

## Cross-project

`korg` is in the cross-project-planning routing table, so the amend rule
applied. **GP-19** was authored and committed to that repo in this ship:
*parked is a visibility class korg owns, for every node kind; a consumer
may filter it but never invent its own notion of dormant.* It binds
korg-dash, kdeskdash and korg-vs as well as kfdc. GP-18 was the previous
highest; precedent for authoring a GP from a korg WI is GP-16 (#1467) and
GP-17 (#1468).

**The consumer gap is deliberate and short.** Once this deploys, kfdc
renders a parked program with no palette treatment — `Operations.svelte`
gives an unknown literal the neutral base on purpose, and
`Operations.svelte.test.ts` already pins that using `parked` as its
hypothetical. Undecorated is survivable; mis-decorated is what #1444 cost
when `queued` arrived wearing the amber in-flight default. Slice 2
(korg:1548) ships straight after.

## Shipped

- `0031_parked_proposals_and_programs.sql` — enum → TEXT + CHECK for
  proposals, CHECK widen for programs, DDL only.
- `vocab.rs` — `PARKED_STATUS`, both vocabularies and both live sets
  widened; `PROPOSAL_STARTED_STATUSES` deliberately unchanged with the
  reasoning recorded; four new fences.
- `repo/common.rs` — `parked_last`, the shared ordering key.
- `repo/{proposals,programs,board,awaiting,planning,work_items}.rs` —
  ordering, the board's queue dispatch, vocabulary-derived exclusions,
  the widened claim predicate.
- MCP tool descriptions, `docs/api.md` (a new normative section),
  `docs/usage.md`, `just gen` output.
- Planning page: parked section below a labelled rule, Park / Resume /
  To queue controls. Programs list and detail: palette treatment and the
  same divider.
- `tests/sprint072.rs` — 16 tests.
- cross-project-planning: GP-19 (committed there, 7d29e68).

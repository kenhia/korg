# 061 — Scoped re-review: the sprint 044–059 surface

**Proposal:** korg:1348 (rank 2) · **Work item:** #1346 (research, M)
**Branch:** `061-review-044-059-surface` · Run 2026-08-17.

The re-review the agent-surface program close-out (plan §16) triggers on "new
surface or session friction" — both prongs had fired. Sequenced after the
repo.rs split (sprint 060), which was this review's prerequisite and held:
every owning module was read whole.

## What shipped

- **`sprints/review/2026-08-17-044-059-surface-re-review.md`** — the T-style
  review (plan §5): every 044–059 owning module read in full, representative
  operations traced against production, findings labeled observed /
  code-confirmed / preference with severity and file:line evidence, a
  works-as-documented ledger with the calls, and a ranked shortlist. Zero
  probe residue (reads only).
- **Findings**: 2 medium (due schedules surface nowhere anyone looks — a due
  signal sat invisible for nine days; `parked` missing from every
  covered-work rollup bucket), 5 low, nits, 1 observation. Plan §5
  measurements: CLAUDE.md korg block still empty — PASS; all-works-as-
  documented — FAIL by the two mediums, a tier smaller than T3's findings.
  The loop is converging.
- **Every actionable finding backed by a WI** (Ken's direction for this
  review, superseding the proposal's top-N gate): korg #1385–#1391, tag
  `review-2026-08`.

## The korg backlog pass (wrap-up, Ken + this session)

The review's wrap-up widened into clearing the korg project backlog:

- **Proposals filed**: 1392 (re-review fix sprint, #1386–#1391), 1393 (due
  schedules + #1113 fortnightly), 1394 (UI papercuts: #1115/#1162/#1168/
  #1310, with premise corrections — #1115's comments-UI half shipped in 055,
  #1168's bare status control in 049), 1395 (FTS, #1177), 1396 (quick wins:
  #466/#1146/#1398).
- **Meta-plan gate** (Ken's call): claude-cleo #1397 — a guiding plan doc
  aligning korg/kfdc/kfo, absorbing #1169's substrate sketch so that
  brainstorm can close. Proposals 1393 and 1395 carry `depends_on` edges to
  it (they render in the board's Deconfliction panel, fittingly);
  1392/1394/1396 are plan-independent and run anytime.
- **Dispositions**: #1346 resolved; #1349 parked (its own gate answered
  against starting — the split removed the friction); #1398 filed for the
  report-sources noise (retire the four probes, kyac deliberately not, plus
  the fresh-above-unrated ordering fix).

End state: korg's live backlog fully covered — every open item is in a
proposal, parked, or owned by #1397's close-out.

## Not deployed

Deliberately — the sprint is docs-only (one review doc + this record; no
code, no migration, `migrations` stays 27). A deploy would rebuild a
byte-equivalent binary under a new tag for nothing. The next code sprint
deploys as usual.

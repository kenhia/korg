-- 0021_proposal_notes_and_routing_summary.sql — WI #860, the second half of the
-- agent-surface program's §2 contract (sprints/planning/2026-08-korg-agent-surface.md).
--
-- 0020 did this for projects: a short, CHECK-enforced contract field plus an
-- unbounded `notes` beside it, both arriving in the same migration so that
-- capping the small field destroyed nothing. Proposals are the same defect at
-- larger scale — they are written as plans, so `summary` holds the entire
-- analysis and is the longest prose in korg.
--
-- Measured on the deployed instance 2026-08-01 (119 rows, archived included):
--
--   status    n   sum chars    p50     p90     max
--   done     97     142,188  1,286   2,728   4,711
--   proposed 17      40,604  2,303   5,389   5,710
--   declined  3       5,884  2,207      —    3,417
--   active    2         795      —       —     505
--
--   96 of 119 rows exceed 500 characters; 189,471 chars of summary in total.
--
-- DDL + DATA. Rollback consequences, the same shape as 0020's and no worse:
--
--   * Adding a nullable column is not a boundary in the dangerous direction — an
--     image built before this ignores `notes`. What an OLD image would notice is
--     that 96 summaries are now the first paragraph rather than the whole
--     analysis; it would render the short form and never show the rest.
--     Reversing is `UPDATE sprint_proposal SET summary = notes, notes = NULL
--     WHERE notes IS NOT NULL` plus dropping the constraint. Nothing is lost in
--     either direction, which is the point of moving rather than truncating.
--
-- Re-appliable on purpose (IF NOT EXISTS on the column, a guard on the
-- constraint, and an UPDATE whose WHERE clause is false once it has run). That
-- is what lets `sprint036.rs` seed an over-cap corpus and apply this file to it
-- rather than testing a paraphrase of it.

-- --------------------------------------------------------------------------
-- 1. `notes` — where the analysis lives from now on.
--
--    Named `notes`, matching projects. §7 Q3 of the program plan settled it on
--    the boring-and-consistent ground: a proposal already has a "summary", and
--    a second prose field needs a name that cannot be confused with it.
--
--    Unbounded. Returned by get_proposal, list_proposals detail:"full" and
--    REST — never by the lean queue row, which is where the token cost was
--    (#852). The analysis therefore RELOCATES rather than duplicating: the
--    Planning page's unfiltered fetch carries about the same bytes it did
--    before, redistributed across two fields.
-- --------------------------------------------------------------------------
ALTER TABLE sprint_proposal ADD COLUMN IF NOT EXISTS notes TEXT;

COMMENT ON COLUMN sprint_proposal.notes IS
    'The full analysis: why this bundle, what it depends on, what was measured. '
    'Unbounded. Returned by get_proposal, list_proposals detail:"full" and REST, '
    'never by the lean queue row. This is where prose that will not fit the '
    '500-character summary belongs.';

-- --------------------------------------------------------------------------
-- 2. Move every over-cap summary. MOVE, never truncate blindly.
--
--    Projects could take the easy way out here — 0020 set `description = NULL`
--    on the six over-cap rows and let a later data pass write real routing
--    lines. `sprint_proposal.summary` is NOT NULL and the Planning page renders
--    it, so this file has to leave something readable behind.
--
--    It derives from STRUCTURE, not from length: the first paragraph, which is
--    how these summaries are actually written — a lede saying what the bundle
--    is, then the analysis. Simulated against all 119 production rows before
--    this was written; the ledes read as genuine summaries (#394, 828 chars,
--    derives to 497: "Clear the entire open korg backlog in one pass — five
--    small items, all in the korg repo. Leads with #392 (S, highest priority):
--    …"). Where the lede itself overruns, it is cut back to a word boundary
--    rather than mid-word.
--
--    The ` […]` marker is appended to every derived summary, so a shortened one
--    never reads as a complete one. It is exact: a row is only touched when
--    summary > 500, and the lede of such a row is always strictly shorter.
--
--    Trigger note: sprint_proposal has no touch trigger (0013 added one to
--    `project` only), so unlike 0016/0018/0020 there is nothing to disable —
--    `node.updated` is not stamped by writes to this table.
-- --------------------------------------------------------------------------
WITH over_cap AS (
    SELECT node_id,
           summary AS full_text,
           -- The first paragraph, falling back to the whole summary when the
           -- text opens with a blank line (degenerate, but it must not derive
           -- to an empty string).
           COALESCE(
               NULLIF(btrim(split_part(summary, E'\n\n', 1)), ''),
               btrim(summary)
           ) AS lede
      FROM sprint_proposal
     WHERE char_length(summary) > 500
)
UPDATE sprint_proposal p
   SET notes   = o.full_text,
       summary = CASE
           WHEN char_length(o.lede) <= 496 THEN o.lede
           -- 496 = 500 minus the marker. `\s+\S*$` drops the partial word the
           -- cut left behind; it matches nothing when the prefix holds no
           -- whitespace at all, which is the honest outcome for a 496-character
           -- single token.
           ELSE regexp_replace(
                    regexp_replace(left(o.lede, 496), '\s+\S*$', ''),
                    '[[:space:],;:—-]+$', ''
                )
       END || ' […]'
  FROM over_cap o
 WHERE p.node_id = o.node_id;

-- --------------------------------------------------------------------------
-- 3. `summary` as a capped routing contract.
--
--    500 characters. #860 recommended the bound and left it to the sprint;
--    036's D-1 sized it against what the full row now carries rather than
--    against what the pick needs, because after #852 the pick never sees
--    `summary` at all.
--
--    VALID immediately, unlike 0019's — step 2 guarantees no row exceeds it,
--    and there is nothing here to tolerate. Same reasoning 0020 §3 recorded.
--
--    Guarded so the file can be re-applied; ADD CONSTRAINT has no IF NOT
--    EXISTS form.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'sprint_proposal_summary_routing_line'
    ) THEN
        ALTER TABLE sprint_proposal
            ADD CONSTRAINT sprint_proposal_summary_routing_line
            CHECK (char_length(summary) <= 500);
    END IF;
END $$;

COMMENT ON COLUMN sprint_proposal.summary IS
    'Routing contract, <=500 chars: what this bundle is, why now, roughly how '
    'big. Written for a session choosing a sprint from a queue, not for someone '
    'who has already chosen it. The analysis — measurements, dependencies, '
    'sequencing, alternatives considered — belongs in `notes`, which is '
    'unbounded and returned by every full read.';

-- --------------------------------------------------------------------------
-- 4. Postcondition.
--
--    Invariants that hold on ANY snapshot, never counts (0018/0019/0020): a
--    count assertion fails on a fresh install with zero proposals.
--
--    The second one is the one that matters. A derived summary carries the
--    marker; if its full text is not in `notes`, this migration destroyed
--    analysis, which is the one outcome #860 forbids outright.
-- --------------------------------------------------------------------------
DO $$
DECLARE
    over_cap bigint;
    lost     bigint;
    moved    bigint;
BEGIN
    SELECT count(*) INTO over_cap
      FROM sprint_proposal WHERE char_length(summary) > 500;

    SELECT count(*) INTO lost
      FROM sprint_proposal
     WHERE summary LIKE '% […]' AND notes IS NULL;

    IF over_cap > 0 OR lost > 0 THEN
        RAISE EXCEPTION
            'proposal summary/notes postcondition failed: over_cap=%, lost=%',
            over_cap, lost;
    END IF;

    SELECT count(*) INTO moved FROM sprint_proposal WHERE notes IS NOT NULL;
    IF moved > 0 THEN
        RAISE NOTICE
            'proposal analysis moved to notes on % rows; their summaries are now the '
            'first paragraph plus a […] marker (live rows are re-authored by hand '
            'after the deploy — 036 D-4)', moved;
    END IF;
END $$;

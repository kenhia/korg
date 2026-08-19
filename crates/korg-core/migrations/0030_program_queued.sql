-- 0030_program_queued.sql — WI #1424, proposal korg:1445, slice 1 of program
-- korg:1447 (the kfdc half is #1444).
--
-- 0023 gave `program.status` three values and the default `'active'`, and
-- `create_program` never wrote the column — so every program was born
-- `active` and kfdc's Operations panel put an ACTIVE callout on work nobody
-- had started. `queued` is the state before that, and it is `holding`'s
-- argument moved one step earlier in the lifecycle: a program that has not
-- begun is neither finished nor in flight, and collapsing it into `active`
-- makes a board claim work is underway when it is not.
--
-- DDL + DATA: two schema changes and one bounded backfill. The data half is
-- what the rollback claim below turns on. Rollback consequences, section by
-- section:
--
--   * §1 (widened CHECK). An older image's `program_status_check` has no
--     'queued', so reverting while queued rows exist makes them
--     unreadable-by-constraint the moment anything rewrites one — the same
--     shape 0023 §1 described for the kind vocabulary. Reverse by moving
--     every queued row to 'active' FIRST, then re-adding 0023's constraint.
--   * §2 (column default). Purely a default: existing rows are untouched and
--     an older image writing an explicit status never sees it. Reverse with
--     ALTER TABLE program ALTER COLUMN status SET DEFAULT 'active'.
--   * §3 (backfill) is the one that changes data. It is deliberately narrow
--     and it is NOT reversible by a symmetric UPDATE — after the fact there
--     is no way to tell a row this moved from one a writer set to 'queued'
--     on purpose. If it has to come back, the `transition` rows it writes are
--     the record of exactly which programs moved: reverse from those, not by
--     re-running the predicate.
--
-- Re-appliable: §1 and §2 are unconditional-but-idempotent, §3's predicate
-- excludes anything already carrying a transition, so a second run is a no-op
-- (every row it moved the first time now has one).

-- --------------------------------------------------------------------------
-- 1. `queued` joins the status vocabulary.
--
--    korg-core's `vocab` is the authority (WI #526); this is the backstop.
--    Ordered as the lifecycle reads — queued -> active <-> holding -> done.
-- --------------------------------------------------------------------------
ALTER TABLE program DROP CONSTRAINT IF EXISTS program_status_check;
ALTER TABLE program
    ADD CONSTRAINT program_status_check
    CHECK (status IN ('queued', 'active', 'holding', 'done'));

-- --------------------------------------------------------------------------
-- 2. A program is born queued.
--
--    `create_program` now writes the column explicitly from
--    `vocab::PROGRAM_INITIAL_STATUS`, so this default is reached only by a
--    direct INSERT — a migration, a fixture, a future writer. It is moved
--    anyway because a backstop that disagrees with the authority is how the
--    original bug would come back: 0023's `'active'` default was itself
--    reachable only because core left the column alone.
-- --------------------------------------------------------------------------
ALTER TABLE program ALTER COLUMN status SET DEFAULT 'queued';

-- --------------------------------------------------------------------------
-- 3. Backfill: programs that were never started.
--
--    Existing programs are `active` whether or not anyone has begun them, so
--    the screenshot on #1424 would still be wrong the day this ships. Decided
--    2026-08-19 as a one-shot correction rather than letting it drift: the
--    table is a handful of rows, and slice 2 needs a real program in the new
--    state to verify against rather than a hand-edited fixture.
--
--    Two conditions, and the second is the one that keeps this honest:
--
--    a. No included proposal has been started, where "started" is `active` or
--       `done` — korg-core's `PROPOSAL_STARTED_STATUSES`, the same definition
--       core now applies on every write path, and it carries the reasoning:
--       `declined` is a decision not to do the work, so a program whose slice
--       was dropped was never started, while `done` work plainly was begun
--       whether or not anyone marked it `active` on the way through. A program
--       with no slices at all matches, correctly: nothing has begun.
--    b. The program has never recorded a transition. `create_program` writes
--       none, so zero rows means nobody has ever set this status by hand.
--       A program somebody explicitly moved to `active` is them saying it is
--       running, and a backfill has no business overruling that.
--
--    Only `active` rows are considered: `holding` and `done` are statements
--    somebody made, and this is not the place to revisit either.
--
--    The transition rows are written by the same statement, so the correction
--    appears in the log exactly as an interactive one would — and they are
--    what makes §3 re-appliable and reversible.
-- --------------------------------------------------------------------------
WITH never_started AS (
    SELECT g.node_id
      FROM program g
     WHERE g.status = 'active'
       AND NOT EXISTS (
           SELECT 1
             FROM relationship r
             JOIN sprint_proposal sp ON sp.node_id = r.right_id
            WHERE r.left_id = g.node_id
              AND r.relationship = 'includes'
              AND sp.status::text IN ('active', 'done')
       )
       AND NOT EXISTS (
           SELECT 1 FROM transition t WHERE t.node_id = g.node_id
       )
), moved AS (
    UPDATE program
       SET status = 'queued'
     WHERE node_id IN (SELECT node_id FROM never_started)
    RETURNING node_id
)
INSERT INTO transition (node_id, from_status, to_status)
SELECT node_id, 'active', 'queued' FROM moved;

-- --------------------------------------------------------------------------
-- 4. Postcondition.
--
--    Structural only, never counts — a fresh install has no programs at all
--    (0018/0020/0021/0022/0023 all learned this).
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'program_status_check'
           AND pg_get_constraintdef(oid) LIKE '%queued%'
    ) THEN
        RAISE EXCEPTION 'program queued postcondition failed: CHECK does not admit queued';
    END IF;

    IF (SELECT column_default FROM information_schema.columns
         WHERE table_name = 'program' AND column_name = 'status') NOT LIKE '%queued%' THEN
        RAISE EXCEPTION 'program queued postcondition failed: default is not queued';
    END IF;

    -- The invariant the backfill and the core write paths share: a queued
    -- program has no slice that has started.
    IF EXISTS (
        SELECT 1
          FROM program g
          JOIN relationship r ON r.left_id = g.node_id AND r.relationship = 'includes'
          JOIN sprint_proposal sp ON sp.node_id = r.right_id
         WHERE g.status = 'queued'
           AND sp.status::text IN ('active', 'done')
    ) THEN
        RAISE EXCEPTION 'program queued postcondition failed: a queued program has a started slice';
    END IF;
END $$;

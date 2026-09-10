-- 0034_soaking.sql — WIs #2151/#2152/#2153/#2154, proposal korg:2162, slice 1
-- of program korg:2167 (design handoff korg:2150).
--
-- A program whose engineering is finished but whose acceptance can only be
-- satisfied by the passage of days had nowhere to sit: it stayed `active`, and
-- `active` is what kfdc's Operations panel means by "wants your attention". So
-- for two or three days the board generated demand nobody could satisfy.
-- `soaking` is that state — mostly done, waiting on tests that span time — and
-- the rest of this file is what makes it more than a label:
--
--   * `workitem.check_after` / `workitem.invalidated_if` — WHEN the evidence
--     can be judged, and WHAT voids the test. The second is the load-bearing
--     one. kmon #2058 was a real multi-day test that failed as a test because
--     slices of its own program overwrote the baseline it depended on, and
--     nobody noticed for a day. A soak lives inside a live fleet and is in the
--     blast radius of everything else running; a test that cannot say what
--     invalidates it cannot be checked, only waited on.
--   * `report.reviewed` — so an operations report that has been acted on stops
--     asking.
--
-- The `soaks` edge itself needs no DDL: it is a row in `relationship` under a
-- label korg-core's registry declares, which is the whole reason the array is
-- an edge and not a column.
--
-- These land in ONE migration on purpose. The MCP surface describes all four
-- facts together — a `soaking` status korg-core will emit, two work-item
-- columns the `soaks` refusal reads, and a report flag `list_reports` filters
-- on — so splitting them admits a window where the tool descriptions promise
-- something the database refuses.
--
-- DDL ONLY. No backfill, no data movement: every column added here is nullable
-- or has a default, and no existing row changes meaning. That is what makes
-- the rollback claims below clean, section by section:
--
--   * §1 (widened program CHECK). 0030 §1 and 0031 §2 exactly. An older
--     image's `program_status_check` has no 'soaking', so reverting while
--     soaking rows exist makes them unreadable-by-constraint the moment
--     anything rewrites one. Reverse by moving every soaking program to
--     'holding' FIRST — the status a failed soak returns to, and the honest
--     one for "started, nothing in flight" — then re-adding 0031's constraint.
--   * §2 (two workitem columns). Both nullable with no default, so an older
--     image ignores them and every existing row reads exactly as before.
--     Reverse with two DROP COLUMNs; the only loss is the soak metadata
--     itself, which is not derivable from anything else and should be read out
--     of `workitem` before dropping if it is ever wanted back.
--   * §3 (report.reviewed). NOT NULL DEFAULT false, so existing reports are
--     born unreviewed — correct, not a compromise: nothing recorded that they
--     had been acted on, and claiming otherwise would hide reports that still
--     want attention. Reverse with DROP COLUMN.
--
-- Rollback is therefore safe in either order for §2/§3 and gated on the data
-- move for §1.

-- --------------------------------------------------------------------------
-- 1. `program.status` admits 'soaking'.
--
--    A CHECK widen, 0030 §1 and 0031 §2 exactly. Ordered as the lifecycle
--    reads: queued -> active -> holding -> soaking -> done, with `parked` last
--    because it is off that line entirely (0031, #1535).
--
--    korg-core carries the ONE transition rule (`soaking` requires every slice
--    terminal and at least one live soak). It is not expressed here, and the
--    reason is the one this schema has given since 0014: core is the single
--    write path both transports share, and a rule that also lived in a trigger
--    is a rule with two authorities that drift. The CHECK's job is the
--    vocabulary, not the lifecycle.
-- --------------------------------------------------------------------------
ALTER TABLE program DROP CONSTRAINT IF EXISTS program_status_check;
ALTER TABLE program
    ADD CONSTRAINT program_status_check
    CHECK (status IN ('queued', 'active', 'holding', 'soaking', 'done', 'parked'));

-- --------------------------------------------------------------------------
-- 2. The two soak fields on a work item.
--
--    `check_after` is a DATE, not a timestamp: the question it answers is
--    "may this be judged today?", which is a calendar question, and a
--    timestamp would invite a timezone argument on a fact that does not have
--    one. It is deliberately NOT a korg schedule — a schedule materialises new
--    work items on a cadence, and this is a property of one test. One fact,
--    one place (design handoff korg:2150).
--
--    `invalidated_if` is prose on purpose. What voids a soak is a statement
--    about the live fleet — "invalidated if kai's baseline is regenerated" —
--    and no closed vocabulary was going to hold that. The value is that it is
--    written down in a field something reads, rather than buried in a body
--    where only a human who already knew to look would find it.
--
--    Both nullable: the overwhelming majority of work items are not soaks. The
--    `soaks` edge is what makes them required, and it enforces that in
--    korg-core where it can name the missing field in the error.
-- --------------------------------------------------------------------------
ALTER TABLE workitem ADD COLUMN IF NOT EXISTS check_after DATE;
ALTER TABLE workitem ADD COLUMN IF NOT EXISTS invalidated_if TEXT;

COMMENT ON COLUMN workitem.check_after IS
    'Soak fields (0034): earliest date this item''s evidence can be judged. Required by the `soaks` edge.';
COMMENT ON COLUMN workitem.invalidated_if IS
    'Soak fields (0034): what state, if it changes, voids this test. The #2058 lesson. Required by the `soaks` edge.';

-- --------------------------------------------------------------------------
-- 3. `report.reviewed`.
--
--    A column and a toggle, sized as such (#2154). No report classification
--    was added and none is wanted: `list_reports` already filters by `source`,
--    and the soak scan files under its own source, so the source value IS the
--    class.
--
--    NOT NULL DEFAULT false is the whole semantics. A same-day re-run of a
--    report resets it — handled in korg-core's `upsert_report`, because the
--    reset belongs with the replace and a DEFAULT cannot see an UPDATE.
-- --------------------------------------------------------------------------
ALTER TABLE report ADD COLUMN IF NOT EXISTS reviewed BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN report.reviewed IS
    'Reviewed flag (0034): someone acted on this report. Reset to false by a same-day re-run.';

-- --------------------------------------------------------------------------
-- 4. Postcondition.
--
--    Structural only, never counts — a fresh install has no programs, work
--    items or reports at all (0018/0020/0021/0022/0023/0030/0031 all learned
--    this the same way).
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'program_status_check'
           AND pg_get_constraintdef(oid) LIKE '%soaking%'
    ) THEN
        RAISE EXCEPTION 'soaking postcondition failed: program CHECK does not admit soaking';
    END IF;

    IF (SELECT data_type FROM information_schema.columns
         WHERE table_name = 'workitem' AND column_name = 'check_after') <> 'date' THEN
        RAISE EXCEPTION 'soaking postcondition failed: workitem.check_after is not DATE';
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'workitem' AND column_name = 'invalidated_if') THEN
        RAISE EXCEPTION 'soaking postcondition failed: workitem.invalidated_if is missing';
    END IF;

    IF (SELECT is_nullable FROM information_schema.columns
         WHERE table_name = 'report' AND column_name = 'reviewed') <> 'NO' THEN
        RAISE EXCEPTION 'soaking postcondition failed: report.reviewed is nullable';
    END IF;
END $$;

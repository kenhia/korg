-- 0032_starred_projects.sql — WI #1629, proposal korg:1630.
--
-- A handful of projects are hot for a week at a time, then the set moves on.
-- Both project rails (Work Items, Planning) grow a band of those at the top,
-- above the category groups, with the projects still shown in their normal
-- category position — the duplication is the feature, not an oversight.
--
-- WHY A COLUMN AND NOT localStorage (D-1, Ken 2026-08-26). Both rails already
-- keep `groupByCategory` in sticky per-browser storage, and starring could have
-- ridden the same path for free. It does not, because a set of hot projects is
-- a fact about the week's work rather than a display preference: it should read
-- the same in every browser, and an agent should be able to see it through
-- `list_projects`/`update_project` and set it. A preference korg cannot see is
-- a preference korg cannot act on.
--
-- WHY `starred` AND NOT `pinned`. `sprint_proposal` and `program` both carry a
-- `pinned` boolean, and reusing the spelling here was the tempting move. It is
-- the wrong one: pinning a proposal orders a queue, starring a project marks it
-- hot, and one word meaning two things across three node kinds puts that
-- ambiguity directly into the surface an agent reads. The UI says star (★/☆,
-- D-3) and the column says starred, all the way down.
--
-- DDL ONLY, nothing to backfill. `false` is the honest starting value for every
-- existing row — no project has ever been starred, so NOT NULL DEFAULT false
-- states a fact rather than guessing one. This is the 0031 §"DDL only" case,
-- not the 0030 case where an old default had mislabelled live rows.
--
-- NOT ON THE LEAN READ, deliberately. `starred` rides `ProjectRow` (the full
-- projection, which is what `GET /api/projects` and `list_projects detail:
-- "full"` return) and NOT `ProjectLeanRow`. The lean row exists to answer *does
-- this work belong here?* and every field on it is routing signal; hotness is
-- not evidence for that question. A routing agent that can see which projects
-- are hot is a routing agent with a thumb on the scale, which is the precise
-- misroute the lean projection was designed in #828 to prevent.
--
-- Rollback consequences: none beyond the column. An older image's
-- `PROJECT_SELECT` names its columns explicitly and does not mention `starred`,
-- so it reads and writes this table unchanged with the column present — the
-- value simply stops being surfaced, and stops being settable. Reverse with
-- `ALTER TABLE project DROP COLUMN starred`, which loses the starred set and
-- nothing else; it is a working set, not a record, and re-starring three
-- projects costs three clicks. No constraint, no index, no default to restore.
--
-- Re-appliable: `ADD COLUMN IF NOT EXISTS` is a no-op on second run.

-- --------------------------------------------------------------------------
-- 1. `project.starred`.
--
--    No index. The rails read every project in one unfiltered `GET
--    /api/projects` and partition client-side, so there is no query that
--    filters on this column to serve — an index would cost writes to speed up
--    a scan nothing performs.
-- --------------------------------------------------------------------------
ALTER TABLE project ADD COLUMN IF NOT EXISTS starred BOOLEAN NOT NULL DEFAULT false;

-- --------------------------------------------------------------------------
-- 2. Postcondition.
--
--    Structural only, and it must never count rows: a fresh install has no
--    projects at all, and on an existing database every row is `false` by
--    construction rather than by anything this migration decided.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
         WHERE table_name = 'project' AND column_name = 'starred'
    ) THEN
        RAISE EXCEPTION 'starred postcondition failed: project.starred is absent';
    END IF;

    IF (SELECT is_nullable FROM information_schema.columns
         WHERE table_name = 'project' AND column_name = 'starred') <> 'NO' THEN
        RAISE EXCEPTION 'starred postcondition failed: project.starred is nullable';
    END IF;

    IF (SELECT column_default FROM information_schema.columns
         WHERE table_name = 'project' AND column_name = 'starred') NOT LIKE '%false%' THEN
        RAISE EXCEPTION 'starred postcondition failed: project.starred has no false default';
    END IF;
END $$;

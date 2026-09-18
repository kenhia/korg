-- 0035_on_demand_sources.sql — WI #2183, proposal korg:2814, slice 2 of
-- program korg:2816.
--
-- A report source with NO cadence by design had nowhere honest to sit. The
-- three declarations 0025 gave it are each a lie about a different thing:
--
--   * a declared `cadence_days` is the largest lie — a months-old verdict
--     reads `fresh` and asserts its last status, which is #950's original
--     failure rebuilt by hand;
--   * `retired` says "deliberately ended", which is false for a source that
--     is working exactly as contracted;
--   * leaving it undeclared gives `unrated`, which is honest right up until
--     inference promotes it.
--
-- That last one is not hypothetical and is the reason this is a migration
-- rather than a note. Measured on 2026-09-17: `kyac` — an interactive tool
-- whose own note records that `unrated` is its correct state — is being served
-- `stale`, 3 days overdue, against a 2-day cadence korg INVENTED for it
-- (`cadence_declared: false`). The 7× span gate added in sprint 052 (#1097) to
-- exclude precisely this source now passes it: kyac's median gap stayed at 2
-- while its history span grew from 5 days to 64, and the ratio crossed.
--
-- The gate did not fail. It EXPIRED — a ratio with only one side bounded, and
-- any episodic source that keeps filing occasionally will eventually cross it.
-- The comment above `SOURCE_MIN_SPAN_CADENCES` in korg-core's `repo/reports.rs`
-- called this outcome when it was written: "If episodic sources become common
-- the answer is an explicit 'on-demand' declaration, not a cleverer
-- inference." This column is that declaration.
--
-- DDL ONLY. No backfill and no data movement: one BOOLEAN NOT NULL DEFAULT
-- FALSE, so every existing row keeps exactly the meaning it had — false is
-- "this source has a cadence or is undeclared", which is true of all nine live
-- sources at the time of writing. The declarations for kyac and kfo-soak are
-- data, written after deploy, NOT baked in here: this file must apply to a
-- fresh install with an empty `report_source` table, and a migration that
-- names a production source would be a migration that means different things
-- on different databases.
--
-- ROLLBACK: reverse with `ALTER TABLE report_source DROP COLUMN on_demand`.
-- An older image ignores the column entirely — it does not appear in 0025's
-- INSERT or in any older query — so reverting needs no data move at all and a
-- rolled-back korg simply resumes inferring a cadence for any source that had
-- been declared on-demand. That is the pre-0035 behaviour, correctly, and the
-- only loss is the declaration itself. Read `SELECT source FROM report_source
-- WHERE on_demand` out first if it is ever wanted back; it is not derivable
-- from anything else, because the entire point is that history cannot tell you
-- a source is unscheduled.

-- --------------------------------------------------------------------------
-- 1. `report_source.on_demand`.
--
--    NOT NULL DEFAULT false rather than nullable, and the choice matters. A
--    nullable flag would admit a third state — "nobody has said" — which is
--    exactly what `unrated` already means and what this column exists to
--    distinguish FROM. The declaration is present or absent, and absent is
--    false.
--
--    The contradiction between `on_demand` and a declared `cadence_days` is
--    NOT expressed as a CHECK here, and the reason is the one this schema has
--    given since 0014: korg-core is the single write path both transports
--    share, and a rule that also lived in a constraint is a rule with two
--    authorities that drift. core returns `invalid_input` naming both fields;
--    a CHECK could only return a constraint violation naming neither.
-- --------------------------------------------------------------------------
ALTER TABLE report_source ADD COLUMN IF NOT EXISTS on_demand BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN report_source.on_demand IS
    'On-demand declaration (0035, #2183): this source has NO cadence by design — it files on an event, not a schedule. Freshness is pinned to the `on-demand` literal, inference is skipped, and it never alerts. Mutually exclusive with cadence_days (enforced in korg-core).';

-- --------------------------------------------------------------------------
-- 2. Postcondition.
--
--    Structural only, never counts — a fresh install has no report sources at
--    all (0018/0020/0021/0022/0023/0030/0031/0034 all learned this the same
--    way). Both halves are checked because NOT NULL and the DEFAULT are the
--    whole semantics of §1: a nullable column here would silently reintroduce
--    the third state the comment above rejects.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'report_source' AND column_name = 'on_demand') THEN
        RAISE EXCEPTION 'on_demand postcondition failed: report_source.on_demand is missing';
    END IF;

    IF (SELECT is_nullable FROM information_schema.columns
         WHERE table_name = 'report_source' AND column_name = 'on_demand') <> 'NO' THEN
        RAISE EXCEPTION 'on_demand postcondition failed: report_source.on_demand is nullable';
    END IF;

    IF (SELECT column_default FROM information_schema.columns
         WHERE table_name = 'report_source' AND column_name = 'on_demand') IS NULL THEN
        RAISE EXCEPTION 'on_demand postcondition failed: report_source.on_demand has no default';
    END IF;
END $$;

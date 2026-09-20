-- 0036_src_path_drive_form.sql — WI 2874, proposal korg:2915, slice 1 of
-- program korg:2916.
--
-- `src_path` answers "where is this project's working copy on its development
-- machine" (0019). It has been unable to answer it for cleo, because the
-- canonical form 0019 fixed is `~`-relative and cleo's clones live at
-- `D:\ClaudeWorks\<name>`. Four live projects carry NULL for that reason alone
-- — `kbrickshoot`, `kctrldeck`, `kpidashclient-win`, `krcmd` — and kmuster's
-- `declared.rs` has the consequence written into a doc comment: "korg's field
-- cannot hold a Windows path, so cleo's projects legitimately have none."
--
-- The first consumer that cannot live with that is kctrldeck's Code tab (korg
-- WI 2853): it wants to open any korg project in VS Code with one tap, and a
-- project with no path is a project it cannot launch — including kctrldeck's
-- own repo.
--
-- THE FORM: `/d/ClaudeWorks/kctrldeck` <-> `D:\ClaudeWorks\kctrldeck`. Ken's
-- proposal, and the recommendation in WI 2874, for three reasons that are
-- worth keeping next to the regex:
--
--   * it is the MSYS/git-bash spelling, so an agent meeting it in a field
--     recognises it without a rabbit-hole;
--   * it is unambiguous against `~/` and against the old rejected form — a
--     POSIX absolute path was never valid here, so `/<letter>/` cannot collide
--     with anything a previous write could have stored;
--   * the conversion is mechanical in both directions, which is what lets the
--     connector endpoint emit a host-native `D:\...` without storing one.
--
-- A single LOWERCASE drive letter, deliberately: one spelling per path, so two
-- rows cannot describe the same clone. The rest of the rules are 0019's,
-- unchanged and now shared by both forms — no trailing slash, no whitespace,
-- no parentheses. A path and nothing else.
--
-- What this does NOT widen: `/home/ken/src/tools/korg` is still refused.
-- `^(~|/[a-z])/` requires a separator immediately after the drive letter, so
-- `/home/...` fails on `o` and 0019's "write it relative to home" rule still
-- binds every POSIX path.
--
-- DDL ONLY. The four cleo rows are DATA and are written after deploy, not
-- here: this file must apply to a fresh install with an empty `project` table,
-- and a migration naming production projects would mean different things on
-- different databases (0035's rule, and the reason it is stated there).
--
-- ROLLBACK: reverse by restoring 0019's constraint --
--
--     ALTER TABLE project DROP CONSTRAINT project_src_path_canonical;
--     ALTER TABLE project
--         ADD CONSTRAINT project_src_path_canonical
--         CHECK (src_path IS NULL OR src_path ~ '^~/[^[:space:]()]*[^[:space:]()/]$')
--         NOT VALID;
--
-- — but note that any `/d/...` value written in between then violates it, so
-- the reversal has to clear those rows first or re-add NOT VALID as shown. An
-- older image is unaffected by the widening itself: it reads `src_path` as an
-- opaque string everywhere except the app-side validator, which only ever
-- refuses MORE than the constraint does.
-- ---------------------------------------------------------------------------

-- ---------------------------------------------------------------------------
-- 1. Precheck, so the promotion in step 2 fails legibly or not at all.
--
--    0019 added its constraint `NOT VALID` for one row: `kcard`, whose
--    `src_path` held a sentence of archive history, and whose resolution was a
--    judgement call it correctly refused to guess at inside a migration. That
--    row is resolved — `kcard` is archived and its `src_path` is NULL — and
--    all 62 rows on kubsdb conform to the canonical form (measured 2026-09-20
--    over `GET /api/projects`, which filters nothing). So the constraint can
--    be promoted to a real invariant, which is what 0019 said to do "when the
--    pass that owns it lands".
--
--    Postgres would report a failure here on its own, but as a bare constraint
--    violation naming no row. A database this migration has never seen — a dev
--    copy, an old restore — is exactly where that matters, so name the rows.
-- ---------------------------------------------------------------------------
DO $$
DECLARE
    offenders text;
BEGIN
    SELECT string_agg(format('%s (%L)', name, src_path), ', ' ORDER BY name)
      INTO offenders
      FROM project
     WHERE src_path IS NOT NULL
       AND src_path !~ '^(~|/[a-z])/[^[:space:]()]*[^[:space:]()/]$';

    IF offenders IS NOT NULL THEN
        RAISE EXCEPTION
            'src_path rows do not satisfy the canonical form, so the constraint '
            'cannot be promoted from NOT VALID: %. Resolve each (a path and '
            'nothing else, `~/`-relative or `/<drive>/`-absolute) and re-run.',
            offenders;
    END IF;
END $$;

-- ---------------------------------------------------------------------------
-- 2. The widened constraint, VALIDATED.
--
--    Dropped and re-added rather than altered: Postgres has no
--    `ALTER CONSTRAINT ... CHECK`, and re-adding without `NOT VALID` is what
--    promotes it. Step 1 has already proved the table conforms, so the
--    validation scan finds nothing — it is the declaration that matters, not
--    the scan.
-- ---------------------------------------------------------------------------
ALTER TABLE project DROP CONSTRAINT project_src_path_canonical;

ALTER TABLE project
    ADD CONSTRAINT project_src_path_canonical
    CHECK (src_path IS NULL OR src_path ~ '^(~|/[a-z])/[^[:space:]()]*[^[:space:]()/]$');

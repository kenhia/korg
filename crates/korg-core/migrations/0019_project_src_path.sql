-- 0019_project_src_path.sql — WI #675: rename `project.cn_path` to `src_path`
-- and normalize the values that can be fixed without judgement.
--
-- The rename fixes a semantic ambiguity, not just a name. A bare path is
-- meaningless without a host, and now that development and production are split
-- per project (`machines` vs `deploy_to`), `cn_path` never said which one it
-- meant. `src_path` answers it by construction: **the working copy on the
-- project's development machine.** korg's own row is the worked example —
-- machines ["kai"], deploy_to ["kubsdb"], so src_path describes the kai
-- checkout. The absence of that statement is arguably what let the field drift,
-- so it is written into the schema description in the same change.
--
-- DDL + DATA. Two consequences for the deploy note, and the first is the
-- opposite of 0018's:
--
--   * It DOES create a rollback boundary. An image built before this migration
--     selects `cn_path` and will fail against the renamed column, so rolling
--     back across this deploy is NOT a clean re-tag — it needs the column
--     renamed back (`ALTER TABLE project RENAME COLUMN src_path TO cn_path`)
--     or a dump restore. The rename is trivially reversible; the point is that
--     it is not automatic.
--   * The value normalization is a fix-up, not a content edit, so it runs with
--     the touch trigger disabled (0013, same reasoning as 0016 and 0018).
--
-- Blast radius: 35 projects on production as of 2026-07-31, of which 4 hold a
-- value this file changes and 11 are NULL. The sole writer is update_project;
-- readers are list_projects, the REST /api/projects surface and one display
-- line in Project Details.

-- --------------------------------------------------------------------------
-- 1. The rename.
-- --------------------------------------------------------------------------
ALTER TABLE project RENAME COLUMN cn_path TO src_path;

-- --------------------------------------------------------------------------
-- 2. Normalize the mechanical values.
--
--    Written as general rules keyed on the defect rather than as four UPDATEs
--    keyed on project name. Naming the rows would be shorter and would couple
--    this file to the instant it was written — 0016's rehearsal caught exactly
--    that failure — and these rules are idempotent, so a row that arrives in
--    the same shape between this being written and applied is also fixed.
--
--    Order matters. Absolute→`~` runs first so that the missing-prefix rule
--    cannot mistake `/home/ken/...` for a relative path.
--
--    Observed on production 2026-07-31:
--      korg     /home/ken/src/tools/korg  → absolute        (rule a)
--      gratch   ~/obsidian/gratch/        → trailing slash  (rule b)
--      klams    src/ai/klams              → missing `~/`    (rule c)
--      kpidash  src/kpidash               → missing `~/`    (rule c)
-- --------------------------------------------------------------------------
ALTER TABLE project DISABLE TRIGGER project_touch_updated;

-- (a) Absolute home-directory paths become `~`-relative. Matches any user's
--     home rather than hardcoding `ken`, so a row imported from another
--     machine's checkout normalizes too.
UPDATE project
   SET src_path = regexp_replace(src_path, '^/home/[^/]+/', '~/')
 WHERE src_path ~ '^/home/[^/]+/';

-- (b) Strip trailing slashes. `~/x/` and `~/x` are the same directory, and a
--     field that holds both cannot be compared or joined on.
UPDATE project
   SET src_path = regexp_replace(src_path, '/+$', '')
 WHERE src_path ~ '/+$';

-- (c) Add the missing `~/` prefix. Scoped to values that start with neither
--     `~` nor `/`, so it cannot double-prefix a correct row or mangle an
--     absolute path that rule (a) deliberately left alone.
UPDATE project
   SET src_path = '~/' || src_path
 WHERE src_path IS NOT NULL
   AND src_path !~ '^[~/]';

ALTER TABLE project ENABLE TRIGGER project_touch_updated;

-- --------------------------------------------------------------------------
-- 3. The canonical form, enforced going forward but NOT VALID.
--
--    Canonical `src_path`: `~/`-relative, no trailing slash, no whitespace, no
--    parentheses — a path and nothing else.
--
--    `NOT VALID` is the whole point rather than a hedge. One live row cannot
--    satisfy this yet:
--
--      kcard  ~/.archive-src/kcard (kai; archived 2026-07-09, was ~/src/tools/kcard)
--
--    — a sentence of archive history in a path field. Extracting that prose to
--    somewhere it belongs is a judgement call bundled into the one pass that
--    visits every project's actual source (#758's first run in fix mode), not
--    something to guess at inside a migration.
--
--    So the constraint is added unvalidated: **every INSERT and UPDATE from
--    here on is checked**, which is what stops the field re-drifting, while the
--    one known-bad legacy row is tolerated until the pass that owns it lands.
--    Promoting it is then a one-liner and a real exit criterion for that work:
--
--      ALTER TABLE project VALIDATE CONSTRAINT project_src_path_canonical;
--
--    Note this correctly does NOT flag `feedhub`, whose path
--    (`~/src/ai-agents/harness-eval-runs/run_02/04-kprojects`) is well-formed
--    but points inside an eval-run tree. That is a truth problem, not a form
--    problem, and belongs to the same judgement pass — a constraint that tried
--    to catch it would be guessing.
-- --------------------------------------------------------------------------
ALTER TABLE project
    ADD CONSTRAINT project_src_path_canonical
    CHECK (src_path IS NULL OR src_path ~ '^~/[^[:space:]()]*[^[:space:]()/]$')
    NOT VALID;

-- --------------------------------------------------------------------------
-- 4. Postcondition. Assert and RAISE — rolling the whole file back under
--    sqlx's per-migration transaction — rather than leaving the corpus half
--    normalized.
--
--    Following 0018: assert invariants that hold on ANY snapshot, never counts.
--    A count would fail on a fresh install (zero projects) and on every dev
--    database. What is asserted is exactly what rules (a)-(c) are responsible
--    for, and no more — deliberately NOT the full canonical form, because
--    `kcard` legitimately still violates it and that is what step 3 encodes.
-- --------------------------------------------------------------------------
DO $$
-- `trailing` and `deferred` are Postgres reserved words (TRIM(TRAILING …),
-- SET CONSTRAINTS … DEFERRED), so the locals below are named around them.
DECLARE
    not_tilde     bigint;
    slash_suffix  bigint;
    non_canonical text;
BEGIN
    SELECT count(*) INTO not_tilde
      FROM project WHERE src_path IS NOT NULL AND src_path !~ '^~/';

    SELECT count(*) INTO slash_suffix
      FROM project WHERE src_path IS NOT NULL AND src_path ~ '/$';

    IF not_tilde > 0 OR slash_suffix > 0 THEN
        RAISE EXCEPTION
            'src_path normalization postcondition failed: not_tilde=%, slash_suffix=%',
            not_tilde, slash_suffix;
    END IF;

    -- Report what step 3 knowingly tolerates, so the deferred work is visible
    -- in the deploy log rather than silent. Same shape as 0018 naming the
    -- projects it left uncategorised.
    SELECT string_agg(name, ', ' ORDER BY name) INTO non_canonical
      FROM project
     WHERE src_path IS NOT NULL
       AND src_path !~ '^~/[^[:space:]()]*[^[:space:]()/]$';
    IF non_canonical IS NOT NULL THEN
        RAISE NOTICE
            'src_path rows left non-canonical (constraint added NOT VALID; owned by the project-metadata pass): %',
            non_canonical;
    END IF;
END $$;

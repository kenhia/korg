-- 0018_project_category_vocabulary.sql — WI #678: true up `project.category`
-- to the closed vocabulary korg-core starts enforcing in the same release, so
-- the Work Items rail can color from the category instead of hashing the
-- project *name* (which put 20 of 31 projects within 12 degrees of another
-- while leaving whole arcs of the wheel empty).
--
-- DATA ONLY. `project.category` has existed since 0011 as a nullable TEXT with
-- no constraint, so this file adds no DDL — it is an UPDATE over the project
-- table plus a postcondition. Two consequences worth carrying into the deploy
-- note:
--
--   * It creates NO rollback boundary. Rolling the image back across this
--     deploy stays a clean re-tag, unlike 0017 (handoff), where crossing the
--     migration backwards needs a dump restore.
--   * A wrong value is fixed by another UPDATE, and the nightly dump holds the
--     prior free-text values regardless.
--
-- Ordering matters the way LB-1 had to precede LB-2 (0016): update_project
-- begins validating `category` against PROJECT_CATEGORIES in this same
-- release. Any row still holding a legacy value — the live corpus held `ai` x8,
-- `AI` x1, `tooling` x3, `infra` x2, `fun` x2, NULL x15 — would fail the next
-- update_project on that project, so the true-up must land with enforcement.
-- The postcondition below refuses to leave one behind.
--
-- Blast radius: at most one UPDATE per named project (32 on production as of
-- 2026-07-26). Nothing reads the old values — the sole writer is
-- update_project, the sole reader one display line in Project Details; no query
-- filters, groups or branches on `category`, and no skill reads it. Superseding
-- the free-text values therefore breaks nothing.

-- --------------------------------------------------------------------------
-- 1. Ken's mapping (2026-07-25/26), verbatim and complete: every project that
--    existed when it was written is named, so nothing is defaulted and the
--    postcondition can assert the assignment rather than a fallback.
--
--    It goes in a temp table rather than being spelled twice, so the UPDATE and
--    the postcondition cannot drift from each other.
--
--    Keyed by name because project names are immutable (WI #246) — unlike the
--    node-id list 0016's rehearsal caught as coupled to the instant it was
--    written. A project created after this file converges to NULL, which stays
--    legal (see step 3).
-- --------------------------------------------------------------------------
CREATE TEMP TABLE project_category_map (
    name     TEXT PRIMARY KEY,
    category TEXT NOT NULL
);

INSERT INTO project_category_map (name, category) VALUES
    -- Ops (8)
    ('Agent-Plan',          'Ops'),
    ('claude-cleo',         'Ops'),
    ('gh-kenhia',           'Ops'),
    ('gratch',              'Ops'),
    ('homelab-ai',          'Ops'),
    ('homelab-health',      'Ops'),
    ('k-homelab',           'Ops'),
    ('kmon',                'Ops'),
    -- AI (8)
    ('kagent',              'AI'),
    ('klams',               'AI'),
    ('klams-mind',          'AI'),
    ('kris',                'AI'),
    ('kvllm',               'AI'),
    ('kyac',                'AI'),
    ('trt-llm-explore',     'AI'),
    ('trt-llm-langchain',   'AI'),
    -- Dashboard (5)
    ('apt-temps',           'Dashboard'),
    ('kdeskdash',           'Dashboard'),
    ('korg-dash',           'Dashboard'),
    ('kpidash',             'Dashboard'),
    ('kvscf',               'Dashboard'),
    -- Infrastructure (5)
    ('ansible-k',           'Infrastructure'),
    ('kcard',               'Infrastructure'),
    ('korg',                'Infrastructure'),
    ('kwebi',               'Infrastructure'),
    ('kwi',                 'Infrastructure'),
    -- Fun (3)
    ('hv-simulator',        'Fun'),
    ('kapollo',             'Fun'),
    ('mortars',             'Fun'),
    -- EVAL (2) — harness-evaluation projects that leaked into the real corpus.
    -- Both are archived, so under the no-color-for-inactive rule they render no
    -- color; the category earns its place as a LABEL that makes the leak
    -- findable, and is the worked example of "add a category when needed".
    ('feedhub',             'EVAL'),
    ('loglens',             'EVAL'),
    -- Other (1) — for ACTIVE projects outside the taxonomy.
    ('ATV',                 'Other');

-- --------------------------------------------------------------------------
-- 2. Apply it with the touch trigger disabled.
--
--    `project_touch_updated` (0013) fires BEFORE UPDATE on project, so a bulk
--    category set would bump every project's `updated`. Migration 0016 hit
--    exactly this and disabled the trigger across its fix-up for the same
--    reason: a fix-up is not a content edit, and pretending otherwise is the
--    kind of dishonest provenance that sprint 022 existed to remove.
-- --------------------------------------------------------------------------
ALTER TABLE project DISABLE TRIGGER project_touch_updated;

UPDATE project p
   SET category = m.category
  FROM project_category_map m
 WHERE p.name = m.name
   AND p.category IS DISTINCT FROM m.category;

ALTER TABLE project ENABLE TRIGGER project_touch_updated;

-- --------------------------------------------------------------------------
-- 3. Postcondition. Assert and RAISE — rolling the whole file back under
--    sqlx's per-migration transaction — rather than leaving the corpus half
--    converted.
--
--    Following 0016's rule that *counts* are the rehearsal's job and not
--    hardcoded here: asserting "32 rows" would fail on a fresh install (zero
--    projects) and on any dev database, and would be coupled to the instant the
--    mapping was written. What is asserted instead are the two invariants that
--    hold on any snapshot and that enforcement actually depends on:
--
--      a. every mapped project carries exactly its mapped category; and
--      b. no project carries a category outside the vocabulary.
--
--    (b) is the one that makes app-side validation safe to turn on. A NULL
--    category is deliberately NOT a failure: `create_project` takes only a
--    name, so every project starts uncategorised and a project created between
--    this file being written and it being applied is uncategorised, not broken.
--    It renders with no color and is reported below so it is visible rather
--    than silent.
-- --------------------------------------------------------------------------
DO $$
DECLARE
    mapped_wrong bigint;
    off_vocab    bigint;
    uncategorised text;
BEGIN
    SELECT count(*) INTO mapped_wrong
      FROM project_category_map m
      JOIN project p ON p.name = m.name
     WHERE p.category IS DISTINCT FROM m.category;

    -- Mirrors korg-core's vocab::PROJECT_CATEGORIES. A second copy, but a
    -- point-in-time assertion rather than a live authority — the same shape as
    -- 0016 naming the four registry labels inline.
    SELECT count(*) INTO off_vocab
      FROM project
     WHERE category IS NOT NULL
       AND category NOT IN ('AI', 'Dashboard', 'EVAL', 'Fun', 'Infrastructure', 'Ops', 'Other');

    IF mapped_wrong > 0 OR off_vocab > 0 THEN
        RAISE EXCEPTION
            'project category true-up postcondition failed: mapped_wrong=%, off_vocab=%',
            mapped_wrong, off_vocab;
    END IF;

    SELECT string_agg(name, ', ' ORDER BY name) INTO uncategorised
      FROM project WHERE category IS NULL;
    IF uncategorised IS NOT NULL THEN
        RAISE NOTICE 'projects left uncategorised (legal; they render with no rail color): %',
            uncategorised;
    END IF;
END $$;

DROP TABLE project_category_map;

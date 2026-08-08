-- 0025_time_derived_surfacing.sql — WIs #581 + #950, proposal korg:1079.
--
-- korg's first time-derived state (plan: sprints/051-time-derived-surfacing.md).
-- Two additive changes that answer the same question from opposite ends — "a
-- date crossing now makes something appear":
--
--   * A `schedule` node kind — a work-item template plus a cadence and an
--     anchor. #581, whole: recurring maintenance AND the one-shot.
--   * A `report_source` table — the per-source cadence override and retirement
--     flag behind #950's staleness age-out.
--
-- DDL ONLY, purely additive. No row is written, updated or deleted; a
-- pre-0025 corpus is already in the correct state for both features (no
-- schedules exist, and every report source infers its cadence from history it
-- already has).
--
-- Rollback consequences, section by section:
--
--   * §1/§2 (kind + detail table). An older image's `node_kind_check` has no
--     'schedule', so reverting leaves those rows unreadable-by-constraint if
--     anything rewrites them; the table itself is inert to old code. Reverse
--     with DROP TABLE schedule, then re-add the 0024 constraint. Note the
--     schedule rows themselves would have to go first — they are `node` rows,
--     and dropping them is data loss, which is the one reason a rollback past
--     this migration is a restore rather than a clean re-tag ONCE a schedule
--     has been filed. Until then it is a clean re-tag.
--   * §3 (report_source) is standalone and referenced by nothing. An old image
--     never selects from it and every derivation it feeds falls back to
--     inference-from-history, which is what the pre-0025 behaviour already was.
--     Reverse: DROP TABLE report_source. No data loss beyond the declared
--     overrides themselves.
--
-- What is deliberately NOT here:
--
--   * Any trigger advancing `anchor_at` on work-item completion. That is a
--     lifecycle write rule, and 0023 settled where those live: korg-core, the
--     one path both transports share, never in a trigger that can drift from
--     it.
--   * Any scheduled job. "Due" and "stale" are read-time predicates (D-2) —
--     which is the point, given #950 exists because an unattended scheduled
--     thing died quietly for eleven days.
--   * Any CHECK tying `cadence` to `anchor_at` semantics. A `once` schedule's
--     anchor IS its fire date; the interval is simply zero. One column, one
--     predicate.
--
-- Re-appliable: every section is guarded (IF NOT EXISTS / pg_constraint probe).

-- --------------------------------------------------------------------------
-- 1. `schedule` joins the kind vocabulary.
--
--    The live kind set is 0024's plus 'schedule'. 0024 removed 'topic' and
--    'daily_plan_item'; nothing since has touched this constraint.
-- --------------------------------------------------------------------------
ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN (
        'workitem', 'card', 'link', 'sprint_proposal', 'report',
        'handoff', 'program', 'schedule'
    ));

-- --------------------------------------------------------------------------
-- 2. The detail table (the 0008/0010/0017/0023 pattern).
--
--    A schedule DOES carry a project, on node.project_id — the opposite of
--    0023 §3's rule for programs, and for the opposite reason. A program is the
--    cross-project layer; a schedule is maintenance *of* something, and that
--    something is a project. The quarterly restore drill is korg's.
--
--    `status` is a CHECK rather than an ENUM, per 0010's precedent: korg-core's
--    `vocab` is the authority (WI #526) and the DB is the backstop, so a CHECK
--    that can be extended without a type rewrite is the right backstop.
--
--    `last_wi_id` is the anti-duplicate mechanism and the honesty mechanism at
--    once: while the materialised work item is still outstanding, the schedule
--    is not due again, because the work item IS the surface. ON DELETE SET NULL
--    rather than CASCADE — deleting a work item must not delete the schedule
--    that generated it, it must only forget that generation.
--
--    No FK from the work item back to the schedule: provenance is the
--    generalized `relationship` table with label 'materializes'
--    (schedule -> workitem), registered in korg-core so relate() validates both
--    endpoints. Same reasoning 0008 gave for `covers` and 0023 for `includes`.
--    `last_wi_id` is the *pointer to the newest* one, not the history; the
--    history is the edges.
-- --------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS schedule (
    node_id     BIGINT      PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    title       TEXT        NOT NULL,
    template    TEXT,
    notes       TEXT,
    cadence     TEXT        NOT NULL,
    anchor_mode TEXT        NOT NULL DEFAULT 'completed',
    anchor_at   TIMESTAMPTZ NOT NULL,
    status      TEXT        NOT NULL DEFAULT 'active',
    wi_type     TEXT        NOT NULL DEFAULT 'maintenance',
    wi_tshirt   TEXT        NOT NULL DEFAULT 'Unknown',
    last_wi_id  BIGINT      REFERENCES node(id) ON DELETE SET NULL,
    CONSTRAINT schedule_title_nonempty CHECK (btrim(title) <> ''),
    CONSTRAINT schedule_cadence_check
        CHECK (cadence IN ('once', 'weekly', 'monthly', 'quarterly', 'yearly')),
    CONSTRAINT schedule_anchor_mode_check
        CHECK (anchor_mode IN ('completed', 'created')),
    CONSTRAINT schedule_status_check
        CHECK (status IN ('active', 'paused', 'done'))
);

-- The due scan reads active schedules ordered by when they came due; the
-- partial index keeps paused and finished ones out of it entirely.
CREATE INDEX IF NOT EXISTS schedule_due_idx
    ON schedule (anchor_at) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS schedule_last_wi_idx
    ON schedule (last_wi_id) WHERE last_wi_id IS NOT NULL;

COMMENT ON COLUMN schedule.anchor_at IS
    'The point the cadence interval measures from (#581). Which EVENT advances '
    'it is `anchor_mode`: `created` advances at materialisation, `completed` '
    'when the materialised work item reaches done/closed — a korg-core write '
    'rule, never a trigger. For cadence `once` the interval is zero, so this '
    'column IS the fire date.';
COMMENT ON COLUMN schedule.last_wi_id IS
    'The newest materialisation. While that work item is still outstanding the '
    'schedule is not due again: the work item is the surface. The full history '
    'is the `materializes` edges, not this pointer.';

-- --------------------------------------------------------------------------
-- 3. Per-source reporting declarations (#950).
--
--    NOT a node, deliberately, and this is the same call 0011 made for project
--    metadata: a reporting source has no comments, no edges and no lifecycle —
--    it is a property of a stream of reports, not a thing work happens to.
--    Making it a node would buy an id nothing would ever link to.
--
--    Every column is an OVERRIDE. The default for all of them is NULL/false,
--    meaning "derive it", and the derivation is the median gap between this
--    source's own consecutive report_dates. kmon files daily and its history
--    proves it, so the case that motivated #950 needs no row here at all.
--
--    `retired` is the answer to the WI's open question — how a deliberately
--    stopped source stops nagging without also silencing a broken one. It is an
--    explicit declaration, so "we turned this off" stays distinguishable from
--    "this died", which silence alone can never do.
--
--    No FK to `report.source`: a source can be declared before its first report
--    (that is how a new daily source skips the `unrated` state on day one), and
--    a source's rows can be pruned without dropping its declaration.
-- --------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS report_source (
    source       TEXT        PRIMARY KEY,
    cadence_days INTEGER,
    grace_days   INTEGER,
    retired      BOOLEAN     NOT NULL DEFAULT FALSE,
    note         TEXT,
    updated      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT report_source_nonempty      CHECK (btrim(source) <> ''),
    CONSTRAINT report_source_cadence_check CHECK (cadence_days IS NULL OR cadence_days > 0),
    CONSTRAINT report_source_grace_check   CHECK (grace_days   IS NULL OR grace_days  >= 0)
);

COMMENT ON TABLE report_source IS
    'Per-source overrides for staleness age-out (#950). Absent row or NULL '
    'column means "derive from report history" — the median gap between '
    'consecutive report_dates. `retired` stops a deliberately-ended source '
    'nagging without silencing a broken one.';

-- --------------------------------------------------------------------------
-- 4. Postcondition.
--
--    Structural invariants only, never counts — a count assertion fails on a
--    fresh install (0018/0020/0021/0022/0023 all learned this).
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'schedule') THEN
        RAISE EXCEPTION 'time-derived surfacing postcondition failed: schedule table missing';
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'report_source') THEN
        RAISE EXCEPTION 'time-derived surfacing postcondition failed: report_source table missing';
    END IF;

    -- The kind constraint must admit 'schedule', or §2's table is unfillable.
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'node_kind_check'
           AND pg_get_constraintdef(oid) LIKE '%schedule%'
    ) THEN
        RAISE EXCEPTION 'time-derived surfacing postcondition failed: node_kind_check does not admit schedule';
    END IF;
END $$;

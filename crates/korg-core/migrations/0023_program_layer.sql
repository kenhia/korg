-- 0023_program_layer.sql — WIs #968 + #969, proposal korg:972, kfdc Phase 0.
--
-- Two additive data-model changes, bundled because they touch the same
-- node/edge stack (plan: sprints/044-program-layer.md):
--
--   * A `program` node kind — the multi-project layer. A program `includes`
--     proposals, ordered. Proposals stay strictly single-project (0022); the
--     cross-project story lives one level up, which is what the residual 0022
--     reported (13 proposals, 4 of them live) has been waiting for.
--   * `awaiting_since` / `awaiting_note` on `node` — "this moves only when Ken
--     acts", expressible at last.
--
-- DDL ONLY, purely additive. Rollback consequences, section by section:
--
--   * §1/§2 (kind + detail table). An older image's `node_kind_check` has no
--     'program', so reverting means the rows become unreadable-by-constraint if
--     anything rewrites them; the table itself is inert to old code. Reverse with
--     DROP TABLE program, then re-add the 0017 constraint.
--   * §3 (no-project CHECK) is the one that bites in the other direction: an
--     image built before this can try to file a program under a project and gets
--     a raw violation surfaced as `internal` rather than the `invalid_input`
--     core returns. Reverse: ALTER TABLE node DROP CONSTRAINT
--     node_program_has_no_project.
--   * §4 (relationship.rank) is nullable with no default — every existing edge
--     reads identically before and after, and an old image ignores the column.
--   * §5 (awaiting columns) likewise: NULL is "not awaiting", so the pre-0023
--     corpus is already in the correct state and old code never sees them.
--
-- What is deliberately NOT here: any constraint on which projects a program's
-- slices span (a program over one project's proposals is legitimate — this
-- repo's own agent-surface program is exactly that), and any clearing rule for
-- the awaiting marker. Lifecycle clearing (D-7) is a write rule, and LB-2
-- settled where those live: in korg-core, the one path both transports share,
-- never in a trigger that can drift from it.
--
-- Re-appliable: every section is guarded (IF NOT EXISTS / pg_constraint probe).

-- --------------------------------------------------------------------------
-- 1. `program` joins the kind vocabulary.
--
--    The live kind set is 0017's plus 'program'. Nothing between 0017 and 0022
--    touched this constraint.
-- --------------------------------------------------------------------------
ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN (
        'workitem', 'card', 'link', 'sprint_proposal', 'report',
        'topic', 'daily_plan_item', 'handoff', 'program'
    ));

-- --------------------------------------------------------------------------
-- 2. The detail table (the 0008/0010/0017 pattern).
--
--    No FK to proposals: membership is the generalized `relationship` table
--    with label 'includes' (program -> sprint_proposal), registered in
--    korg-core so relate() validates both endpoints. Same reasoning 0008 gave
--    for `covers` — no new join table — and the same reason 0017 gave for
--    handoffs: the edge is the membership, and it can be reordered, removed and
--    audited with the tools that already exist.
--
--    `status` is a CHECK rather than an ENUM: 0008 used an enum for proposals,
--    0010 moved to a CHECK for reports, and korg-core's `vocab` is the authority
--    either way (WI #526) — the DB is the backstop. A CHECK is extensible
--    without a type rewrite.
--
--    `aim` is the program's routing contract, the way a proposal's `summary` is
--    (0021), and `notes` is the unbounded analysis. No length CHECK here: 0021
--    put the proposal's 500-char cap in the DB, and that cap is enforced by core
--    for programs too, but a program's aim has no queue row to keep lean.
-- --------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS program (
    node_id BIGINT  PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    title   TEXT    NOT NULL,
    aim     TEXT    NOT NULL,
    notes   TEXT,
    status  TEXT    NOT NULL DEFAULT 'active',
    rank    NUMERIC NOT NULL DEFAULT 0,
    pinned  BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT program_title_nonempty CHECK (btrim(title) <> ''),
    CONSTRAINT program_aim_nonempty   CHECK (btrim(aim)   <> ''),
    CONSTRAINT program_status_check   CHECK (status IN ('active', 'holding', 'done'))
);

CREATE INDEX IF NOT EXISTS program_order_idx  ON program (pinned DESC, rank ASC);
CREATE INDEX IF NOT EXISTS program_status_idx ON program (status);

-- --------------------------------------------------------------------------
-- 3. A program carries NO project (D-6) — the mirror of 0022 §2.
--
--    0022 made every proposal carry a project. This is the symmetric rule one
--    level up, and it is the whole point of the layer: a program filed under a
--    single project rebuilds the exact lie proposals were just cured of.
--    Proposal 818 is the worked example — work in kagent, klams-mind and korg,
--    filed "korg", because the writer had to pick one.
--
--    A program's span is DERIVED from its slices' projects, so the fact has one
--    home and cannot go stale when a slice is added. Valid immediately: there
--    are no programs yet.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'node_program_has_no_project'
    ) THEN
        ALTER TABLE node
            ADD CONSTRAINT node_program_has_no_project
            CHECK (kind <> 'program' OR project_id IS NULL);
    END IF;
END $$;

COMMENT ON CONSTRAINT node_program_has_no_project ON node IS
    'A program is the CROSS-project layer (#968): filing one under a single '
    'project rebuilds the mis-routing 0022 cured for proposals. Its span is '
    'derived from the projects of the proposals it includes.';

-- --------------------------------------------------------------------------
-- 4. Slice order lives on the edge (D-2).
--
--    Position is a property of the containment, not of the proposal: a proposal
--    included by two programs has two positions, and only an edge column holds
--    both. Same drag-orderable NUMERIC idiom as card.rank and
--    sprint_proposal.rank.
--
--    Nullable with no default, so all ~450 existing `covers` edges read exactly
--    as before. Readers order by `rank NULLS LAST, right_id`, which keeps an
--    unranked edge stable rather than random.
-- --------------------------------------------------------------------------
ALTER TABLE relationship ADD COLUMN IF NOT EXISTS rank NUMERIC;

COMMENT ON COLUMN relationship.rank IS
    'Optional position of the right endpoint within the left one (#968). Used '
    'by the `includes` label to order a program''s slices; NULL on every other '
    'edge, which sorts last.';

-- --------------------------------------------------------------------------
-- 5. The awaiting-Ken marker (D-3), on `node` beside `archived`.
--
--    0001's stated design is that cross-cutting attributes live on `node` "so
--    they are shared by every kind" — and a work item, a proposal AND a program
--    can all be waiting on Ken.
--
--    Deliberately NOT a reserved tag, which was the cheaper guess #969 offered.
--    Tags are written wholesale (`UPDATE node SET tags = $2`), so an agent
--    editing tags for an unrelated reason silently clears the marker; 76% of
--    nodes carry tags, so that is a hot path, and the failure is invisible. The
--    corpus already shows the tag approach not converging: `decision` (1 node)
--    and `needs-decision` (1 node), two spellings of one concept.
--
--    A timestamp rather than a boolean because the lane's value is "this has
--    been waiting nine days" — free here, an extra column with a flag.
-- --------------------------------------------------------------------------
ALTER TABLE node ADD COLUMN IF NOT EXISTS awaiting_since TIMESTAMPTZ;
ALTER TABLE node ADD COLUMN IF NOT EXISTS awaiting_note  TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'node_awaiting_note_needs_since'
    ) THEN
        ALTER TABLE node
            ADD CONSTRAINT node_awaiting_note_needs_since
            CHECK (awaiting_note IS NULL OR awaiting_since IS NOT NULL);
    END IF;
END $$;

-- Partial: the lane is a handful of rows against 884 nodes, and it is read on
-- every board render.
CREATE INDEX IF NOT EXISTS node_awaiting_idx
    ON node (awaiting_since) WHERE awaiting_since IS NOT NULL;

COMMENT ON COLUMN node.awaiting_since IS
    'NULL = not awaiting. Non-null = this node moves only when Ken acts, and '
    'when the ask was raised (#969). Re-asserting the marker preserves this '
    'timestamp: the age is the point.';
COMMENT ON COLUMN node.awaiting_note IS
    'What is being asked of Ken. Only meaningful with awaiting_since.';

-- --------------------------------------------------------------------------
-- 6. Postcondition.
--
--    Structural invariants only, never counts — a count assertion fails on a
--    fresh install (0018/0020/0021/0022 all learned this).
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'program') THEN
        RAISE EXCEPTION 'program layer postcondition failed: program table missing';
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'relationship' AND column_name = 'rank') THEN
        RAISE EXCEPTION 'program layer postcondition failed: relationship.rank missing';
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name = 'node' AND column_name = 'awaiting_since') THEN
        RAISE EXCEPTION 'program layer postcondition failed: node.awaiting_since missing';
    END IF;

    IF EXISTS (SELECT 1 FROM node WHERE kind = 'program' AND project_id IS NOT NULL) THEN
        RAISE EXCEPTION 'program layer postcondition failed: a program carries a project';
    END IF;

    IF EXISTS (SELECT 1 FROM node WHERE awaiting_note IS NOT NULL
                                    AND awaiting_since IS NULL) THEN
        RAISE EXCEPTION 'program layer postcondition failed: awaiting_note without awaiting_since';
    END IF;
END $$;

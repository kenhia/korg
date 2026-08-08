-- 0026_transition_log.sql — WI #977, proposal korg:1076.
--
-- korg's first record of *when a thing changed state* (plan:
-- sprints/053-board-events-and-deconfliction.md).
--
-- The board has had no event feed since #970 asked for one, and sprint 045
-- deferred it (D-7) rather than fake it: the only available substitute was
-- `node.updated`, which advances on any edit, so a "recently shipped" feed built
-- from it would date a proposal by its last tag edit and present that as a ship
-- date. On a surface whose whole promise is deterministic rendering (kfdc
-- standing decision #1: no LLM in the render path), a plausible-looking wrong
-- date is worse than an absent panel.
--
-- **What this table is for, stated as a rule**: korg records the events that
-- have no honest timestamp anywhere else. A node's creation has `node.created`;
-- a report landing has `report_date`; a handoff being written has its own
-- `node.created`. A *status change* had nothing — and it is the one that says
-- "shipped". So the log covers transitions and nothing else, and a ticker
-- renders creations and reports from the columns korg already had.
--
-- DDL ONLY, purely additive. No row is written, updated or deleted. There is
-- deliberately **no backfill**: the history does not exist, and inventing one
-- from `node.updated` is the exact lie this table was built to stop telling. A
-- pre-0026 corpus therefore starts with an empty feed and fills forward, which
-- is the honest state.
--
-- Rollback consequences: reverse with DROP TABLE transition. An old image never
-- selects from it and never writes to it, so rolling the image back across this
-- deploy is a **clean re-tag** — the running code simply stops appending. The
-- only loss is the accumulated event history itself, which nothing else derives
-- from and which no other column can reconstruct. Keeping the table across a
-- rollback is therefore strictly better than dropping it: it is inert to old
-- code and still correct when the new image returns.
--
-- What is deliberately NOT here:
--
--   * Any trigger writing this table. 0023 settled where lifecycle rules live —
--     korg-core, the one path both transports share — and a trigger would drift
--     from it. The three update paths that hook `settle_awaiting` hook this too.
--   * Any `kind` column. The node's kind is one join away and denormalising it
--     creates a second place for it to be wrong.
--   * Any actor/origin column. korg has no authenticated writer to record; the
--     `relationship.origin` precedent is self-reported and unverified, and a
--     self-reported actor on an event log reads as provenance while being
--     nothing of the sort. When korg grows a real writer identity this table
--     takes a column; until then it stays honest about what it knows.
--
-- Re-appliable: every statement is guarded (IF NOT EXISTS).

-- --------------------------------------------------------------------------
-- 1. The log.
--
--    Not a node, for the reason 0011 and 0025 §3 gave for project metadata and
--    report sources: an event has no comments, no edges and no lifecycle of its
--    own. It is a fact *about* a node. Making it a node would buy an id nothing
--    would ever link to, and would put a million-row corpus in the same table
--    as the work.
--
--    `from_status` is NOT NULL and constrained to differ from `to_status`,
--    which is the invariant the whole feed rests on: **a write that does not
--    change the status must not produce an event.** korg's own post-deploy
--    check re-PATCHes a status to the value it already has, and agents re-set
--    statuses routinely; a log that recorded those would reproduce
--    `node.updated`'s failure one table over.
--
--    ON DELETE CASCADE: deleting a node deletes its history. The alternative is
--    orphan events naming an id that no longer resolves, which a renderer
--    cannot show and a reader cannot interpret.
-- --------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS transition (
    id          BIGSERIAL   PRIMARY KEY,
    node_id     BIGINT      NOT NULL REFERENCES node(id) ON DELETE CASCADE,
    from_status TEXT        NOT NULL,
    to_status   TEXT        NOT NULL,
    at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT transition_actually_changed CHECK (from_status <> to_status)
);

-- The feed: newest first, global. `id` breaks ties because two transitions in
-- one transaction share `now()` to the microsecond, and a feed that reorders
-- between refreshes is a feed nobody trusts.
CREATE INDEX IF NOT EXISTS transition_feed_idx ON transition (at DESC, id DESC);

-- One node's history — "when did this actually ship?" — which is the per-row
-- question the feed's global ordering cannot answer.
CREATE INDEX IF NOT EXISTS transition_node_idx ON transition (node_id, at DESC);

COMMENT ON TABLE transition IS
    'Append-only status-transition log (#977). Written by korg-core''s update '
    'paths ONLY, never by a trigger, and only when the status actually changed. '
    'This is the board ticker''s substrate: `node.updated` cannot serve it '
    'because it advances on any edit, so a proposal edited for its tags would '
    'date as newly shipped.';
COMMENT ON COLUMN transition.at IS
    'Postgres''s clock at the moment of the change — the same clock '
    'BoardRollup.generated reads, so an age computed against it is right.';

-- --------------------------------------------------------------------------
-- 2. Postcondition.
--
--    Structural only, never counts: a fresh install has no rows and every
--    migration from 0018 on that asserted a count learned this the hard way.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'transition') THEN
        RAISE EXCEPTION 'transition log postcondition failed: transition table missing';
    END IF;

    -- The no-op guard is the table's reason for being trustworthy; a future
    -- edit dropping it would let re-PATCHes back into the feed silently.
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'transition_actually_changed'
    ) THEN
        RAISE EXCEPTION 'transition log postcondition failed: no-op guard missing';
    END IF;
END $$;

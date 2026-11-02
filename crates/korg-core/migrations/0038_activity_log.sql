-- 0038_activity_log.sql — WI 2936, proposal korg:4314 (sprint 096).
--
-- Ken runs several agents against korg at once and loses track of what
-- changed where. 0026's `transition` table records status changes; this is
-- its sibling for every OTHER change an agent makes to a node: created,
-- edited, a comment added/edited/deleted, an edge related/unrelated, an
-- attachment attached/detached. `GET /api/activity` reads the two together,
-- and the web Activity page renders it.
--
-- WHY A SIBLING, NOT A GENERALISED `transition`: two readers consume
-- `transition` today — the board ticker (`list_transitions`) and the
-- work-item flow series (`flow.rs`) — and both read it as "status changes and
-- nothing else". Widening it would mean changing both readers in the same
-- deploy and re-checking 0026's `transition_actually_changed` invariant for
-- rows that carry no status. A sibling table leaves both readers and the
-- invariant untouched, and the union on read costs one `UNION ALL`. Chosen on
-- migration risk, as the 2936 thread asked.
--
-- 0026's rules, inherited unchanged:
--
--   * Written by korg-core's write paths ONLY, never by a trigger: 0023
--     settled that lifecycle rules live in korg-core, the one path both
--     transports share.
--   * The no-op guard. A write that changes nothing logs nothing — the update
--     paths fingerprint the node before and after and record `edited` only
--     when the two differ. Without it every re-PATCH would land here and the
--     log would reproduce `node.updated`'s failure one table over.
--   * NO BACKFILL. The history does not exist; inventing it from
--     `node.updated` is the lie 0026 was built to stop telling. A pre-0038
--     corpus starts with an empty log and fills forward.
--   * No `kind` column (one join away) and no actor column (korg has no
--     authenticated writer; 0026 declined one, and WI 4242's header would not
--     tell two Claude sessions apart anyway).
--   * Proposal and program context is NOT copied onto the row. It is resolved
--     at read time through `covers` and `includes`, so there is no second
--     place for it to be wrong.
--
-- Not a node, for 0026's reason: an event has no comments, edges or lifecycle.
--
-- `node_id` CASCADEs: deleting a node deletes its history, as with
-- `transition`. `comment_id` and `other_node_id` carry NO foreign key on
-- purpose: `comment_deleted` must outlive its comment, and `unrelated` /
-- `detached` name a node that may since have gone. The reader LEFT JOINs them.
--
-- DDL ONLY, purely additive. No row is written, updated or deleted.
--
-- ROLLBACK: `DROP TABLE activity`. An older image never reads or writes it, so
-- re-tagging across this deploy is clean; keeping the table across a rollback
-- loses only the events the old image does not write.
--
-- Re-appliable: every statement is guarded.

CREATE TABLE IF NOT EXISTS activity (
    id            BIGSERIAL   PRIMARY KEY,
    node_id       BIGINT      NOT NULL REFERENCES node(id) ON DELETE CASCADE,
    action        TEXT        NOT NULL,
    -- The comment, for the three comment actions.
    comment_id    BIGINT      NULL,
    -- The other end, for `related`/`unrelated` (the edge's right side) and
    -- `attached`/`detached` (the attachment node).
    other_node_id BIGINT      NULL,
    -- The relationship label, for `related`/`unrelated`.
    label         TEXT        NULL,
    at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- `status_changed` is deliberately absent: it is `transition`'s, and the
    -- reader names it. A status change landing here would be counted twice.
    CONSTRAINT activity_action_known CHECK (action IN (
        'created', 'edited',
        'comment_added', 'comment_edited', 'comment_deleted',
        'related', 'unrelated',
        'attached', 'detached'
    )),
    CONSTRAINT activity_comment_shape CHECK (
        (action LIKE 'comment\_%') = (comment_id IS NOT NULL)
    ),
    CONSTRAINT activity_edge_shape CHECK (
        (action IN ('related', 'unrelated', 'attached', 'detached'))
            = (other_node_id IS NOT NULL)
    )
);

-- The feed: newest first, global; `id` breaks the `now()` tie within one
-- transaction, as in 0026.
CREATE INDEX IF NOT EXISTS activity_feed_idx ON activity (at DESC, id DESC);
CREATE INDEX IF NOT EXISTS activity_node_idx ON activity (node_id, at DESC);

COMMENT ON TABLE activity IS
    'Append-only change log (WI 2936), sibling of `transition`: every node '
    'change that is not a status change. Written by korg-core only, never by a '
    'trigger, and never for a write that changed nothing. No backfill.';

-- Postcondition: structural only. A fresh install has no rows.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'activity') THEN
        RAISE EXCEPTION 'activity log postcondition failed: activity table missing';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint
                    WHERE conname = 'activity_action_known') THEN
        RAISE EXCEPTION 'activity log postcondition failed: action vocabulary missing';
    END IF;
END $$;

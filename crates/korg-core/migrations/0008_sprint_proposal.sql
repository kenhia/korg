-- 0008_sprint_proposal.sql — agent-planning proposed sprints.
--
-- A sprint_proposal is a node like any other: it bundles a short pitch
-- (title + summary) with the set of work items it covers via the existing
-- generalized `relationship` table (label 'covers') — no new join table.
--
-- Ordering mirrors `card.rank` (NUMERIC, drag-orderable) rather than a
-- literal priority field: both Ken (drag in the web UI) and agents (MCP)
-- reorder with roughly equal weight. `pinned` proposals always sort first.

ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check
    CHECK (kind IN ('workitem', 'card', 'link', 'slot', 'sprint_proposal'));

CREATE TYPE sprint_proposal_status AS ENUM (
    'proposed',
    'active',
    'done',
    'declined'
);

CREATE TABLE sprint_proposal (
    node_id BIGINT                 PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    title   TEXT                   NOT NULL,
    summary TEXT                   NOT NULL,
    status  sprint_proposal_status NOT NULL DEFAULT 'proposed',
    rank    NUMERIC                NOT NULL,
    pinned  BOOLEAN                NOT NULL DEFAULT FALSE,
    CONSTRAINT sprint_proposal_title_nonempty CHECK (btrim(title) <> '')
);

CREATE INDEX sprint_proposal_order_idx ON sprint_proposal (pinned DESC, rank ASC);
CREATE INDEX sprint_proposal_status_idx ON sprint_proposal (status);

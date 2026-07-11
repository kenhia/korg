-- 0012_daily_planning.sql — replace generated timebox slots with source-linked daily planning.
-- Existing slots are intentionally disposable; deleting their nodes first lets
-- node-scoped comments and generalized relationships cascade safely.

DELETE FROM node WHERE kind = 'slot';
DROP TABLE slot;
DROP TABLE slot_template;

ALTER TABLE node DROP CONSTRAINT node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN (
        'workitem', 'card', 'link', 'sprint_proposal', 'report',
        'topic', 'daily_plan_item'
    ));

CREATE TABLE topic (
    node_id     BIGINT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    CONSTRAINT topic_name_nonempty CHECK (btrim(name) <> '')
);

CREATE INDEX topic_name_search_idx ON topic (lower(name));

CREATE TABLE daily_plan_item (
    node_id        BIGINT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    plan_date      DATE NOT NULL,
    position       INT NOT NULL CHECK (position >= 0),
    display        TEXT NOT NULL,
    source_node_id BIGINT NOT NULL REFERENCES node(id) ON DELETE RESTRICT,
    completed_at   TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT daily_plan_display_nonempty CHECK (btrim(display) <> ''),
    CONSTRAINT daily_plan_date_position_unique
        UNIQUE (plan_date, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX daily_plan_date_position_idx ON daily_plan_item (plan_date, position);
CREATE INDEX daily_plan_source_idx ON daily_plan_item (source_node_id);
CREATE INDEX daily_plan_history_idx ON daily_plan_item (plan_date, completed_at);

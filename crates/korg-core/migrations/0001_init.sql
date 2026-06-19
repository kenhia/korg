-- 0001_init.sql — korg initial schema.
--
-- Unifies kwi (work items) and kcard (kanban cards) onto a single
-- typed-node + generalized-edges model:
--
--   * One surrogate identity space: node(id). Every work item and card IS a
--     node. Future kinds (calendar slots, reading-list URLs) slot in as new
--     `kind` values + detail tables without disturbing this core.
--   * Cross-cutting attributes (project, category, tags, archived, timestamps)
--     live on `node` so they are shared by every kind.
--   * Kind-specific attributes live in detail tables keyed by node_id.
--   * `relationship` is a generalized many-to-many edge over node ids, so any
--     kind can link to any other (work item <-> card today; slots/URLs later).
--   * Work items keep a stable, user-facing serial `wi_number` (referenced by
--     external project docs) that is NOT the primary key.

-- ---------------------------------------------------------------------------
-- Taxonomy
-- ---------------------------------------------------------------------------

-- Unified project taxonomy: kwi projects (repo-derived) and kcard projects
-- merge here by name. kwi-only fields (gh_repo, cn_path) are optional so
-- kcard-origin projects fit without them.
CREATE TABLE project (
    id          BIGSERIAL   PRIMARY KEY,
    name        TEXT        NOT NULL UNIQUE,
    gh_repo     TEXT,
    cn_path     TEXT,
    description TEXT,
    created     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_name_nonempty CHECK (btrim(name) <> '')
);

-- Areas remain project-scoped (a kwi work-item concept).
CREATE TABLE area (
    id          BIGSERIAL PRIMARY KEY,
    project_id  BIGINT    NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    name        TEXT      NOT NULL,
    description TEXT,
    UNIQUE (project_id, name)
);

-- ---------------------------------------------------------------------------
-- Node: the shared identity + cross-cutting attributes
-- ---------------------------------------------------------------------------

CREATE TABLE node (
    id         BIGSERIAL   PRIMARY KEY,
    kind       TEXT        NOT NULL CHECK (kind IN ('workitem', 'card')),
    project_id BIGINT      REFERENCES project(id) ON DELETE SET NULL,
    category   TEXT,
    tags       TEXT[]      NOT NULL DEFAULT '{}',
    archived   BOOLEAN     NOT NULL DEFAULT FALSE,
    created    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT node_tags_no_empty CHECK (array_position(tags, '') IS NULL)
);

CREATE INDEX node_kind_idx     ON node (kind);
CREATE INDEX node_project_idx  ON node (project_id);
CREATE INDEX node_category_idx ON node (category);
CREATE INDEX node_tags_gin_idx ON node USING GIN (tags);

-- ---------------------------------------------------------------------------
-- Work item detail (kwi)
-- ---------------------------------------------------------------------------

-- User-facing, searchable work-item number. Stays serial across the app's
-- lifetime (import seeds it from kwi ids; future items continue at max+1).
CREATE SEQUENCE workitem_wi_number_seq AS BIGINT;

CREATE TABLE workitem (
    node_id        BIGINT  PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    wi_number      BIGINT  NOT NULL UNIQUE DEFAULT nextval('workitem_wi_number_seq'),
    area_id        BIGINT  REFERENCES area(id) ON DELETE SET NULL,
    wi_type        TEXT    NOT NULL,
    wi_status      TEXT    NOT NULL,
    wi_tshirt      TEXT    NOT NULL DEFAULT 'Unknown'
                           CHECK (wi_tshirt IN ('XS','S','M','L','XL','Huge','Unknown')),
    sprint         TEXT,
    title          TEXT    NOT NULL,
    content        TEXT    NOT NULL,
    details        TEXT,
    parent_node_id BIGINT  REFERENCES node(id) ON DELETE SET NULL
);

ALTER SEQUENCE workitem_wi_number_seq OWNED BY workitem.wi_number;

CREATE INDEX workitem_area_idx   ON workitem (area_id);
CREATE INDEX workitem_parent_idx ON workitem (parent_node_id);
CREATE INDEX workitem_status_idx ON workitem (wi_status);
CREATE INDEX workitem_type_idx   ON workitem (wi_type);

-- ---------------------------------------------------------------------------
-- Card detail (kcard)
-- ---------------------------------------------------------------------------

CREATE TYPE card_status AS ENUM (
    'Backlog',
    'Research',
    'OnDeck',
    'Active',
    'Done',
    'Cut'
);

CREATE TABLE card (
    node_id     BIGINT      PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    status      card_status NOT NULL DEFAULT 'Backlog',
    title       TEXT        NOT NULL,
    description TEXT        NOT NULL DEFAULT '',
    rank        NUMERIC     NOT NULL,
    CONSTRAINT card_title_nonempty CHECK (btrim(title) <> '')
);

CREATE INDEX card_status_rank_idx ON card (status, rank ASC);

CREATE TABLE comment (
    id           BIGSERIAL   PRIMARY KEY,
    card_node_id BIGINT      NOT NULL REFERENCES node(id) ON DELETE CASCADE,
    body         TEXT        NOT NULL,
    created      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT comment_body_nonempty CHECK (btrim(body) <> '')
);

CREATE INDEX comment_card_created_idx ON comment (card_node_id, created ASC);

-- ---------------------------------------------------------------------------
-- Generalized relationships (kwi `related`, expanded to all node kinds)
-- ---------------------------------------------------------------------------

CREATE TABLE relationship (
    id           BIGSERIAL PRIMARY KEY,
    left_id      BIGINT    NOT NULL REFERENCES node(id) ON DELETE CASCADE,
    right_id     BIGINT    NOT NULL REFERENCES node(id) ON DELETE CASCADE,
    relationship TEXT      NOT NULL
);

CREATE INDEX relationship_left_idx  ON relationship (left_id);
CREATE INDEX relationship_right_idx ON relationship (right_id);

-- ---------------------------------------------------------------------------
-- updated-at maintenance (advances only on UPDATE; explicit values on INSERT
-- are preserved, which keeps the import faithful to source timestamps).
-- ---------------------------------------------------------------------------

CREATE FUNCTION touch_updated() RETURNS TRIGGER AS $$
BEGIN
    NEW.updated := now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER node_touch_updated
    BEFORE UPDATE ON node
    FOR EACH ROW EXECUTE FUNCTION touch_updated();

CREATE TRIGGER comment_touch_updated
    BEFORE UPDATE ON comment
    FOR EACH ROW EXECUTE FUNCTION touch_updated();

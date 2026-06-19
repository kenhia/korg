-- 0002_link.sql — add the reading-list "link" node kind.
--
-- A link is a captured URL to read later. It is a first-class node, so it
-- shares the cross-cutting attributes on `node` (project, category, tags,
-- archived) and can participate in generalized `relationship` edges with work
-- items and cards.

-- Allow the new kind. The original inline column check is named
-- `node_kind_check` by Postgres convention.
ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN ('workitem', 'card', 'link'));

CREATE TABLE link (
    node_id BIGINT  PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    url     TEXT    NOT NULL,
    title   TEXT,
    read    BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT link_url_nonempty CHECK (btrim(url) <> '')
);

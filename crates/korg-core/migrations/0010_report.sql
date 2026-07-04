-- 0010_report.sql — daily reports (kmon et al.) as first-class nodes.
--
-- A report is a node like any other (0008 pattern): kind-specific detail table,
-- findings linked through the existing generalized `relationship` table (label
-- 'finding', report -> workitem), comments already node-generic (0007).
-- UNIQUE (source, report_date): one report per source per day — a same-day
-- re-run REPLACES content via upsert but keeps the node_id, so links and
-- comments survive.

ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check
    CHECK (kind IN ('workitem', 'card', 'link', 'slot', 'sprint_proposal', 'report'));

CREATE TABLE report (
    node_id     BIGINT  PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    source      TEXT    NOT NULL,
    report_date DATE    NOT NULL,
    status      TEXT    NOT NULL CHECK (status IN ('ok', 'attention', 'problem')),
    summary     TEXT    NOT NULL,
    body        TEXT    NOT NULL,
    model       TEXT,
    escalated   BOOLEAN NOT NULL DEFAULT FALSE,
    CONSTRAINT report_source_nonempty  CHECK (btrim(source) <> ''),
    CONSTRAINT report_summary_nonempty CHECK (btrim(summary) <> ''),
    CONSTRAINT report_one_per_day UNIQUE (source, report_date)
);

CREATE INDEX report_date_idx ON report (report_date DESC);

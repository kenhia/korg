-- 0003_slot.sql — calendar timebox slots with an editable weekly template.
--
-- A slot is a duration-only timebox on a specific date (no fixed time-of-day),
-- into which a small goal can be placed. Slots are nodes, so they can be linked
-- to the work item / card they advance via generalized relationships.
--
-- The weekly cadence lives in `slot_template`, seeded with Ken's schedule but
-- fully editable (durations/counts can change as free time changes).

ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN ('workitem', 'card', 'link', 'slot'));

-- Recurring weekly template. dow: 0=Sunday .. 6=Saturday (Postgres DOW).
CREATE TABLE slot_template (
    id               BIGSERIAL PRIMARY KEY,
    dow              SMALLINT  NOT NULL CHECK (dow BETWEEN 0 AND 6),
    position         INT       NOT NULL,
    duration_minutes INT       NOT NULL CHECK (duration_minutes > 0),
    label            TEXT,
    UNIQUE (dow, position)
);

-- Materialized timeboxes on a date.
CREATE TABLE slot (
    node_id          BIGINT  PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    slot_date        DATE    NOT NULL,
    duration_minutes INT     NOT NULL CHECK (duration_minutes > 0),
    label            TEXT,
    goal             TEXT,
    template_id      BIGINT  REFERENCES slot_template(id) ON DELETE SET NULL,
    position         INT     NOT NULL DEFAULT 0
);

CREATE INDEX slot_date_idx ON slot (slot_date, position);

-- Default weekly template (Ken's schedule):
--   Mon-Fri: 30 + 60
--   Sat:     120 + 120 + 30
--   Sun:     60 + 30 + 30
INSERT INTO slot_template (dow, position, duration_minutes) VALUES
    (1, 0, 30), (1, 1, 60),
    (2, 0, 30), (2, 1, 60),
    (3, 0, 30), (3, 1, 60),
    (4, 0, 30), (4, 1, 60),
    (5, 0, 30), (5, 1, 60),
    (6, 0, 120), (6, 1, 120), (6, 2, 30),
    (0, 0, 60), (0, 1, 30), (0, 2, 30);

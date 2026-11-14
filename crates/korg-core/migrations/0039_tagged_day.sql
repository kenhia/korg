-- 0039_tagged_day.sql — WI 4664, proposal korg:4667 (sprint 101).
--
-- Ken's days off and special days, for kpidash's time and calendar card
-- (program korg:4669). korg holds the list because Ken reaches korg from every
-- machine, so "ask an agent to tag the 27th" needs no new tool. The publisher
-- (`calfeed/`, a kai user timer) merges these rows with rule-computed US
-- federal holidays into kdashdata's `kdash:calendar` record.
--
-- A TABLE, not a node kind. A tag has no comments, no edges, no lifecycle and
-- no title worth searching — 0025 §3's argument for report sources and 0037's
-- for machines, unchanged. Removal is a DELETE, because a day that is no longer
-- off is not an archived day off; it is simply not a fact any more.
--
-- One row per tag, so a date that is both off and a birthday is two rows; the
-- publisher folds a date's rows into the feed's one entry. `yearly` makes a
-- birthday a single row: it repeats on `day`'s month and day in every year
-- from `day`'s own year on (a 29 February repeats on the 28th in a common
-- year). Holidays are NOT stored: they are computed by rule, so there is no
-- data to refresh each January.
--
-- Bounds mirror the feed contract (kdashdata CD-30): a name is 1..31
-- characters and a short label 1..12, so a stored row can never produce an
-- entry the panel's reader would skip. `off` rows carry neither — a day off is
-- a flag, and the feed's `short` for it belongs to whatever else is on the date.
--
-- DDL ONLY. Ken's own days are DATA, written after deploy through the
-- `add_tagged_day` MCP tool. They never appear in this repo.
--
-- ROLLBACK: `DROP TABLE tagged_day`. An older image never reads it; keeping it
-- across a rollback is harmless and saves re-seeding.
--
-- Re-appliable: every statement is guarded.

CREATE TABLE IF NOT EXISTS tagged_day (
    id BIGSERIAL PRIMARY KEY,
    -- The date, America/Los_Angeles. For a `yearly` row, its month and day
    -- repeat every year from this year on.
    day DATE NOT NULL,
    yearly BOOLEAN NOT NULL DEFAULT false,
    kind TEXT NOT NULL
        CONSTRAINT tagged_day_kind_check CHECK (kind IN ('off', 'special')),
    -- A special day's name; NULL on an `off` row and required on a `special` one.
    label TEXT NULL
        CONSTRAINT tagged_day_label_len CHECK (label IS NULL OR char_length(label) BETWEEN 1 AND 31),
    short TEXT NULL
        CONSTRAINT tagged_day_short_len CHECK (short IS NULL OR char_length(short) BETWEEN 1 AND 12),
    created TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT tagged_day_label_by_kind CHECK (
        (kind = 'special' AND label IS NOT NULL)
        OR (kind = 'off' AND label IS NULL AND short IS NULL)
    )
);

-- The same tag twice is one tag. `COALESCE` because an `off` row's label is
-- NULL, and NULLs never collide in a plain unique index.
CREATE UNIQUE INDEX IF NOT EXISTS tagged_day_unique
    ON tagged_day (kind, day, yearly, COALESCE(label, ''));

COMMENT ON TABLE tagged_day IS
    'Ken''s days off and special days (WI 4664), published with computed '
    'holidays as kdash:calendar. One row per tag; yearly rows repeat on their '
    'month-day from their own year on. Holidays are computed, never stored.';

-- Postcondition: structural only. A fresh install has no rows.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'tagged_day') THEN
        RAISE EXCEPTION 'tagged_day postcondition failed: table missing';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_indexes
                    WHERE indexname = 'tagged_day_unique') THEN
        RAISE EXCEPTION 'tagged_day postcondition failed: unique index missing';
    END IF;
END $$;

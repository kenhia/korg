-- 0005_slot_unique.sql — make slot generation idempotent. (WI #82)
--
-- `generate_slots` previously inserted unconditionally, so re-running it for an
-- overlapping date range duplicated every slot. A slot is uniquely identified
-- by its date and position-within-the-day (positions come from the weekly
-- template, which is itself UNIQUE (dow, position)). We dedupe any rows that
-- already slipped through, then enforce that natural key so duplicates are
-- impossible at the database level — independent of the application code.

-- Drop duplicates, keeping one row per (slot_date, position): prefer a row that
-- already carries a goal, then the lowest node_id. Deleting the node cascades
-- to the slot via slot.node_id ... ON DELETE CASCADE.
DELETE FROM node n
USING (
    SELECT node_id
    FROM (
        SELECT node_id,
               row_number() OVER (
                   PARTITION BY slot_date, position
                   ORDER BY (goal IS NOT NULL) DESC, node_id
               ) AS rn
        FROM slot
    ) ranked
    WHERE rn > 1
) dup
WHERE n.id = dup.node_id;

ALTER TABLE slot ADD CONSTRAINT slot_date_position_uniq UNIQUE (slot_date, position);

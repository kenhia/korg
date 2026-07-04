-- 0009_identity.sql — one number: a work item's node id IS its wi_number.
--
-- Docs and humans reference work items as #N (wi_number); MCP relationships and
-- comments key on node.id. Two numbers for one thing was a standing papercut.
-- After this migration: for every workitem, node.id = wi_number, and creation
-- keeps them equal forever (wi_number is set from node.id; its old sequence is
-- gone). Existing NON-workitem nodes that squat on a wi_number-valued id are
-- renumbered above the current max — nothing references them by number in docs.
--
-- Mechanics: FKs to node(id) are captured from the catalog (with their exact
-- definitions), dropped, the remap applied, then re-added — no constraint names
-- or ON DELETE semantics are hardcoded. The id rewrite is two-phase through a
-- +10M offset so no transient PK collision is possible regardless of overlap
-- between old and target ids. Runs in the migration's transaction.

DO $$
DECLARE
    fk  RECORD;
    off CONSTANT BIGINT := 10000000;
BEGIN
    -- sanity: the offset region must be free
    IF EXISTS (SELECT 1 FROM node WHERE id > off) THEN
        RAISE EXCEPTION 'node ids above %, offset trick unsafe — pick a bigger offset', off;
    END IF;

    CREATE TEMP TABLE fk_defs ON COMMIT DROP AS
        SELECT conrelid::regclass::text AS tbl,
               conname,
               (SELECT attname FROM pg_attribute
                 WHERE attrelid = conrelid AND attnum = conkey[1]) AS col,
               pg_get_constraintdef(oid) AS def
        FROM pg_constraint
        WHERE confrelid = 'node'::regclass AND contype = 'f';

    CREATE TEMP TABLE remap (old_id BIGINT PRIMARY KEY, new_id BIGINT UNIQUE) ON COMMIT DROP;

    -- squatters: non-workitem nodes sitting on an id some workitem needs
    INSERT INTO remap (old_id, new_id)
    SELECT n.id,
           GREATEST((SELECT COALESCE(MAX(id), 0) FROM node),
                    (SELECT COALESCE(MAX(wi_number), 0) FROM workitem))
           + row_number() OVER (ORDER BY n.id)
    FROM node n
    WHERE n.kind <> 'workitem'
      AND n.id IN (SELECT wi_number FROM workitem);

    -- workitems whose node id isn't already their wi_number
    INSERT INTO remap (old_id, new_id)
    SELECT w.node_id, w.wi_number FROM workitem w WHERE w.node_id <> w.wi_number;

    FOR fk IN SELECT * FROM fk_defs LOOP
        EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I', fk.tbl, fk.conname);
    END LOOP;

    -- phase A: move every remapped node (and every reference) into the offset
    -- region; phase B: drop the offset. Each is one statement per table, so
    -- uniqueness can never transiently collide.
    UPDATE node n SET id = r.new_id + off FROM remap r WHERE n.id = r.old_id;
    FOR fk IN SELECT * FROM fk_defs LOOP
        EXECUTE format(
            'UPDATE %s t SET %I = r.new_id + %s FROM remap r WHERE t.%I = r.old_id',
            fk.tbl, fk.col, off, fk.col);
    END LOOP;

    UPDATE node SET id = id - off WHERE id > off;
    FOR fk IN SELECT * FROM fk_defs LOOP
        EXECUTE format('UPDATE %s SET %I = %I - %s WHERE %I > %s',
                       fk.tbl, fk.col, fk.col, off, fk.col, off);
    END LOOP;

    FOR fk IN SELECT * FROM fk_defs LOOP
        EXECUTE format('ALTER TABLE %s ADD CONSTRAINT %I %s', fk.tbl, fk.conname, fk.def);
    END LOOP;

    PERFORM setval(pg_get_serial_sequence('node', 'id'),
                   GREATEST((SELECT MAX(id) FROM node), 1));
END $$;

-- Postconditions: every workitem's node id equals its wi_number …
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM workitem WHERE node_id <> wi_number) THEN
        RAISE EXCEPTION 'identity migration failed: node_id <> wi_number remains';
    END IF;
END $$;

-- … and they stay equal: wi_number is now assigned from node.id at insert.
ALTER TABLE workitem ALTER COLUMN wi_number DROP DEFAULT;
DROP SEQUENCE IF EXISTS workitem_wi_number_seq;

-- 0015_node_sequence_fresh_install.sql — let node #1 exist on a fresh install.
-- (WI #552, REVIEW.md F-08)
--
-- 0009_identity ends with
--
--     PERFORM setval(pg_get_serial_sequence('node', 'id'),
--                    GREATEST((SELECT MAX(id) FROM node), 1));
--
-- which is right on a populated database and wrong on an empty one. `MAX(id)`
-- is NULL there, `GREATEST` ignores NULLs and yields 1, and the two-argument
-- `setval` sets is_called = true — so the first `nextval` returns 2. On a
-- fresh install node #1, and therefore work item #1, can never exist. It is
-- invisible in production (data was always present) and merely confusing in
-- tests, until something reasonably assumes wi_numbers start at 1.
--
-- 0009 is not edited: sqlx checksums applied migrations, so changing it would
-- break every existing deployment's migration check. This runs after it and
-- corrects only the empty-database case.
--
-- Idempotent and safe to re-run: on a non-empty `node` it does nothing at all,
-- which is the half that would hurt if it were wrong.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM node) THEN
        -- Three-argument setval with is_called = false: the *next* nextval
        -- returns 1 rather than skipping it.
        PERFORM setval(pg_get_serial_sequence('node', 'id'), 1, false);
    END IF;
END $$;

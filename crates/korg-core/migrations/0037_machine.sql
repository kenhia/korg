-- 0037_machine.sql — WI 2919, proposal korg:4314 (sprint 096).
--
-- The connector (`GET /api/connectors/projects`, WI 2874) promises an
-- ABSOLUTE, host-native path, so a `~/`-relative `src_path` has to be
-- expanded against its host's home directory. korg had nowhere to record one —
-- `project.machines` is a `TEXT[]` of bare names (0011) — so sprint 086 shipped
-- a documented constant, `POSIX_HOME = "/home/ken"`. Ken's ruling (WI 2919,
-- comment 4278): the constant "feels like a hack waiting to break", and a
-- small per-machine table is cheap insurance. kimac is the first host it is
-- actually wrong for: its home is `/Users/ken`.
--
-- Deliberately SMALL. This is a home-directory table, not the "should korg
-- model machines" substrate the WI's body asks about:
--
--   * `project.machines` stays `TEXT[]`. No foreign key to this table — a
--     project may name a host nobody has described yet, and refusing that
--     write would turn a missing fact into a broken one.
--   * Two columns. OS flavour, deploy-target-ness and tailnet names are not
--     here; add them when a consumer needs one, not before.
--   * Not a node, for the reason 0025 §3 gave for report sources: a machine
--     row has no comments, no edges and no lifecycle of its own.
--
-- `home` NULL means "this host does not expand `~`" — the Windows case. Those
-- hosts store their clones in 0036's drive-root form, which the connector
-- converts without a home at all, so a NULL home is a statement, not a gap.
-- A host with NO ROW is the gap: the connector reports a `~/` path on it as
-- unlocated rather than guessing.
--
-- DDL ONLY. The fleet's rows are DATA and are written after deploy through
-- `PUT /api/machines/:name`, not here: this file must apply to a fresh
-- install, and a migration naming production hosts would mean different
-- things on different databases (0035's rule, restated in 0036).
--
-- ROLLBACK: `DROP TABLE machine`. An older image never reads it and goes back
-- to its constant, so re-tagging across this deploy is clean; keeping the
-- table across a rollback is harmless and saves re-seeding.
--
-- Re-appliable: every statement is guarded.

CREATE TABLE IF NOT EXISTS machine (
    -- A `project.machines` entry, spelled the same way: `kai`, `kubs0`, `cleo`.
    name TEXT PRIMARY KEY
        CONSTRAINT machine_name_canonical CHECK (name ~ '^[a-z0-9][a-z0-9-]*$'),
    -- Absolute, no trailing slash: `/home/ken`, `/Users/ken`. Exactly what
    -- `~` expands to on that host, so `home || '/' || rest` is the whole rule.
    home TEXT NULL
        CONSTRAINT machine_home_absolute CHECK (
            home IS NULL OR home ~ '^/[^[:space:]]*[^[:space:]/]$'
        )
);

COMMENT ON TABLE machine IS
    'Per-host home directory (WI 2919), for expanding a project''s `~/` '
    'src_path. NULL home = the host does not expand `~` (Windows: drive-root '
    'paths, 0036). No row = unknown, and the connector says so.';

-- Postcondition: structural only. A fresh install has no rows.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'machine') THEN
        RAISE EXCEPTION 'machine postcondition failed: machine table missing';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint
                    WHERE conname = 'machine_home_absolute') THEN
        RAISE EXCEPTION 'machine postcondition failed: home constraint missing';
    END IF;
END $$;

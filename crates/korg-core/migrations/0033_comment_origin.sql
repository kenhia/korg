-- 0033_comment_origin.sql — WI #1879, proposal korg:1944.
--
-- `list_comments` returns {id, node_id, body, created, updated} — no author,
-- no origin. karc's `ship` reads a proposal's comments looking for the
-- overseer's `overseer: cleared to ship`; it can prove the comment exists and
-- when, but not who filed it. korg is no-auth HTTP on the fleet, so a real
-- author column is not on offer, and this migration does not pretend
-- otherwise.
--
-- WHY THIS IS D-17's COLUMN, NOT A NEW CONVENTION. `relationship.origin`
-- (0016 §4, D-17) is already exactly this: TEXT, nullable, no default,
-- self-reported by the writer and unverified — the web client sends `web`, a
-- skill sends its name, and NULL honestly means "predates provenance" rather
-- than "anonymous". Comments get the same column with the same semantics and
-- the same spelling. A second provenance convention across two detail tables
-- would put that ambiguity into the surface agents read, which is the mistake
-- 0032 refused for `starred`/`pinned`.
--
-- WHAT IT IS NOT. Not a control. An unverified string cannot stop a leg from
-- writing `origin: overseer` on its own clearance, and nothing here tries to:
-- karc closes that hole structurally (PD-9 — a clearance must postdate the
-- leg's last turn end). This is the audit aid beside it, so a human reading
-- the thread sees `overseer` vs `overseen-sprint` at a glance. 0026 §"Any
-- actor/origin column" declined exactly this for the transition log and named
-- the reason: there is no authenticated writer. That reasoning stands — which
-- is why this is self-reported and says so everywhere it surfaces.
--
-- DDL ONLY, and NO BACKFILL — the two are the same decision here. Every
-- existing comment predates the write path, and NULL states exactly that.
-- Stamping the existing rows with a guessed origin would manufacture
-- provenance that was never recorded, and a fabricated audit trail is worse
-- than an absent one. This is 0032's "DDL only" case: the honest starting value
-- for every existing row is the one the column already has.
--
-- NO INDEX. Nothing filters on it. Comments are read node-scoped
-- (`WHERE node_id = $1`) and origin is rendered, never queried — an index
-- would cost writes to speed up a query that does not exist. Same call 0032
-- made for `project.starred`.
--
-- Rollback consequences: none beyond the column. An older image's comment
-- SELECTs name their columns explicitly and never mention `origin`, so it
-- reads and writes this table unchanged with the column present — the value
-- simply stops being surfaced and stops being settable, and any origin already
-- stamped survives the downgrade untouched for a later re-upgrade. Reverse
-- with `ALTER TABLE comment DROP COLUMN origin`, which loses the stamps and
-- nothing else. No constraint, no index, no default to restore.
--
-- Re-appliable: `ADD COLUMN IF NOT EXISTS` is a no-op on second run.

-- --------------------------------------------------------------------------
-- 1. `comment.origin`.
--
--    Nullable with no default, mirroring `relationship.origin` exactly. The
--    write path (add_comment/update_comment) treats an absent origin as NULL
--    rather than substituting a caller-derived guess.
-- --------------------------------------------------------------------------
ALTER TABLE comment ADD COLUMN IF NOT EXISTS origin TEXT;

COMMENT ON COLUMN comment.origin IS
    'Self-reported, unverified provenance of the writer, exactly as '
    'relationship.origin (D-17): the web client sends ''web'', a skill sends '
    'its own name. NULL means the comment predates provenance or the writer '
    'declined to identify itself — never that it is anonymous in any enforced '
    'sense. korg is no-auth HTTP on the fleet; this is an audit aid, not a '
    'control.';

-- --------------------------------------------------------------------------
-- 2. Postcondition.
--
--    Structural only. It must never count rows: a fresh install has no
--    comments, and on an existing database every row is NULL by construction
--    rather than by anything this migration decided.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
         WHERE table_name = 'comment' AND column_name = 'origin'
    ) THEN
        RAISE EXCEPTION 'origin postcondition failed: comment.origin is absent';
    END IF;
END $$;

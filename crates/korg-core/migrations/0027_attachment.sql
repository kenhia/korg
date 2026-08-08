-- 0027_attachment.sql — images as first-class nodes (sprint 056, proposal
-- korg:1081, WIs #582 and #1119; design record: the handoff on the "Images in
-- korg" program, korg:1128).
--
-- An attachment is a node like any other — the 0008/0010/0017/0023/0025 pattern
-- — carrying the metadata for one uploaded image. The bytes live on disk under
-- KORG_IMG_ROOT (`/datastore/korg/images` on kubsdb), one directory per
-- attachment; korg-img owns that layout and this table never names a path,
-- because a path stored in two places is a path that can disagree with itself.
--
-- The node's `id` IS the image's identity: the display id `img-<hex>` is the
-- hex of this node id (handoff D3). That is why there is no id column here.
--
-- **There is deliberately no `state` column.** The handoff (D5) describes an
-- attachment as `pending` until it is linked to its owner on save. Storing that
-- as a column would put the same fact in two places — the column and the
-- `has_attachment` edge — and the way they drift is the dangerous way round: an
-- edge written through the generic `relate()` path would leave the column
-- saying `pending`, and the sweeper would delete a *linked* image. So `pending`
-- means "no has_attachment edge", `linked` means "has one", both are derived on
-- read, and the sweeper's predicate is edge-less AND older than its cutoff.
-- The observable lifecycle is exactly the one D5 specifies; it just cannot lie.
--
-- **Owners are nodes, not comments.** D2 says "owner (WI or comment)", but a
-- korg comment is not a node — it is a row in `comment` with a `node_id`
-- pointing at the node it hangs off (0001/0007) — and `relationship` can only
-- reference nodes. An image pasted into a comment is therefore owned by the
-- node that comment is on, and the comment body carries the placement token.
-- This keeps existence edge-derived, which is D2's actual requirement.
--
-- DDL ONLY, purely additive: one widened CHECK and two new tables. No row is
-- written, updated or deleted, and nothing that existed before this migration
-- is read differently after it.
--
-- Rollback consequences: a **clean re-tag**. An old image knows nothing about
-- either table and never selects from them, so rolling back across this deploy
-- leaves them inert; only the `node_kind_check` widening is visible to it, and
-- a widened CHECK constrains an old writer no differently because that writer
-- never emits 'attachment'. Reverse fully with DROP TABLE attachment_variant,
-- DROP TABLE attachment, and restoring the 0025 constraint — but only after
-- deleting any attachment nodes, and note that dropping the tables strands the
-- blobs on disk: the store is not transactional with Postgres, and the purge
-- runbook (slice 3) is what reconciles the two.
--
-- Re-appliable: every statement is guarded (IF NOT EXISTS / pg_constraint probe).

-- --------------------------------------------------------------------------
-- 1. `attachment` joins the kind vocabulary.
--
--    The live kind set is 0025's plus 'attachment'. Widening this constraint
--    obliges korg-core's `vocab::NODE_KINDS` to grow with it — the schema suite
--    pins the two together, and the node-preview fence then asks for a preview
--    arm, which is the chain sprint 054 built precisely so a new kind cannot be
--    half-added.
-- --------------------------------------------------------------------------
ALTER TABLE node DROP CONSTRAINT IF EXISTS node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN (
        'workitem', 'card', 'link', 'sprint_proposal', 'report',
        'handoff', 'program', 'schedule', 'attachment'
    ));

-- --------------------------------------------------------------------------
-- 2. The detail table.
--
--    `filename` is what the uploader called it — kept for the attachment list
--    and for a human reading the store, never used to address a blob.
--
--    `mime` is SNIFFED from the bytes at upload, not taken from the client's
--    Content-Type, and it is also how a path is rebuilt (korg-img's
--    `ext_for_mime`). A declared type is a claim; the decoder has an opinion
--    either way.
--
--    `content_hash` is recorded and deliberately NOT acted upon (handoff D4).
--    It makes "have we seen this before?" answerable later; deduplicating
--    storage on it now would mean one blob with several owners, and the
--    sensitive-image purge runbook (D9) could no longer promise that removing
--    an attachment removed its bytes.
--
--    An attachment carries `node.project_id` like any node, inherited from its
--    owner at upload where there is one. It is metadata for filtering, not
--    identity: an image's real anchor is the has_attachment edge.
-- --------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS attachment (
    node_id      BIGINT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    filename     TEXT   NOT NULL,
    mime         TEXT   NOT NULL,
    byte_size    BIGINT NOT NULL,
    width        INTEGER NOT NULL,
    height       INTEGER NOT NULL,
    content_hash TEXT   NOT NULL,
    CONSTRAINT attachment_filename_nonempty CHECK (btrim(filename) <> ''),
    CONSTRAINT attachment_mime_nonempty     CHECK (btrim(mime)     <> ''),
    CONSTRAINT attachment_size_positive     CHECK (byte_size > 0),
    CONSTRAINT attachment_dimensions_positive CHECK (width > 0 AND height > 0)
);

-- "Have we seen these bytes before?" — the query the hash exists to make
-- possible. Not unique: identical uploads are separate attachments by design.
CREATE INDEX IF NOT EXISTS attachment_hash_idx ON attachment (content_hash);

-- --------------------------------------------------------------------------
-- 3. The generated variants.
--
--    A table rather than JSONB or a column pair per variant: `/api/img/stats`
--    is then one sum() (kmon's growth milestones, slice 4, read it), and a
--    third variant is a row rather than a migration. Two rows per attachment is
--    a rounding error next to the blobs they describe.
--
--    `variant` is bare TEXT with korg-img as the authority, matching 0010's
--    precedent and WI #526's rule: korg-core validates, the DB backstops.
-- --------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS attachment_variant (
    node_id   BIGINT  NOT NULL REFERENCES attachment(node_id) ON DELETE CASCADE,
    variant   TEXT    NOT NULL,
    mime      TEXT    NOT NULL,
    width     INTEGER NOT NULL,
    height    INTEGER NOT NULL,
    byte_size BIGINT  NOT NULL,
    PRIMARY KEY (node_id, variant),
    CONSTRAINT attachment_variant_nonempty CHECK (btrim(variant) <> ''),
    CONSTRAINT attachment_variant_size_positive CHECK (byte_size > 0),
    CONSTRAINT attachment_variant_dimensions_positive CHECK (width > 0 AND height > 0)
);

COMMENT ON TABLE attachment IS
    'One uploaded image (#582/#1119). The blob lives on disk under KORG_IMG_ROOT '
    'in a directory named for this row''s display id (img-<hex of node_id>); this '
    'table never stores a path. `pending` vs `linked` is DERIVED from the presence '
    'of a has_attachment edge, never stored — see the migration header.';
COMMENT ON COLUMN attachment.content_hash IS
    'SHA-256 of the original bytes, recorded for future dedup DETECTION only. '
    'korg never shares a blob between attachments: the purge runbook has to be '
    'able to promise that removing an attachment removed its bytes.';
COMMENT ON TABLE attachment_variant IS
    'The eagerly-generated sizes (thumb ~400px, agent <=1568px — Anthropic''s '
    'vision ceiling). korg does no on-demand resizing and has no cache layer.';

-- --------------------------------------------------------------------------
-- 4. Postconditions.
--
--    Structural only, never counts: a fresh install has no attachments, and
--    every migration since 0018 that asserted a count learned this the hard way.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'attachment') THEN
        RAISE EXCEPTION 'attachment postcondition failed: attachment table missing';
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.tables
                    WHERE table_name = 'attachment_variant') THEN
        RAISE EXCEPTION 'attachment postcondition failed: attachment_variant table missing';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'node_kind_check'
           AND pg_get_constraintdef(oid) LIKE '%attachment%'
    ) THEN
        RAISE EXCEPTION 'attachment postcondition failed: node_kind_check does not admit attachment';
    END IF;
END $$;

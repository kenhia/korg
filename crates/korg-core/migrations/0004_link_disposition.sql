-- 0004_link_disposition.sql — reading-list dispositions.
--
-- A link's disposition tracks what Ken decided about a captured URL. Hard
-- delete is intentionally deferred (soft-delete via node.archived + CLI later);
-- Summarize / Vault-Save are agent actions performed outside korg (over MCP),
-- and the resulting state is recorded here.

CREATE TYPE link_disposition AS ENUM (
    'Unread',
    'Done',
    'Revisit',
    'Summarized',
    'VaultSaved'
);

ALTER TABLE link
    ADD COLUMN disposition link_disposition NOT NULL DEFAULT 'Unread';

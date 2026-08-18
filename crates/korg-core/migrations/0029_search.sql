-- 0029_search.sql — WI #1177, proposal korg:1395 (sprint 066).
--
-- korg-native full-text search (plan: sprints/066-korg-native-full-text-search.md).
-- One generated `search_tsv` column per searchable detail table, each backed by
-- a GIN index. Together they are korg's search corpus: every node kind that
-- carries prose, plus one document per comment — comments are where the real
-- payload lives (#1177), so indexing titles alone was the measured-worse option.
--
-- **Why generated columns and not an index table.** #1177 budgeted for an
-- external tantivy segment and therefore for a whole subsystem around it:
-- refresh-on-write, a `get_board` change detector, a delete-and-rebuild
-- recovery story, an `indexed_at` staleness field on every hit. A GENERATED
-- ALWAYS ... STORED column is maintained by Postgres inside the writing
-- transaction, so none of that exists here. Search cannot be stale, cannot be
-- half-rebuilt, and cannot disagree with the row it describes. The alternative
-- shape — one `search_doc` table fed by triggers — buys a single index at the
-- cost of ~10 tables x 3 operations of trigger code, which is exactly the class
-- of thing that goes wrong silently. This one cannot lie.
--
-- **Why the locator is inside the vector.** `WI-836` and `korg:1395` are how
-- Ken and agents actually name work, so an identifier has to be a searchable
-- term, not just a display string. Since the 0009 identity migration a work
-- item's node id IS its wi_number, so `comment` can derive both spellings from
-- its own `node_id` without joining: a comment on WI-836 is reachable by
-- `WI-836`. For a comment whose parent is not a work item the `WI-<id>` token
-- names a locator that does not exist — harmless, because the id space is
-- shared and no other node can claim it.
--
-- **Why `||`/`coalesce` and not `concat_ws`.** A generation expression must be
-- IMMUTABLE. `concat_ws` and `date::text` are only STABLE (they consult
-- type-output settings), and Postgres refuses them here. That is also why
-- `report` indexes its `source` rather than its `report_date` — the date is a
-- filterable column, not search text.
--
-- Weights: `A` = locator + title, `B` = body. Read side ranks with
-- `ts_rank(tsv, q, 1|32)`; the `1` (divide by 1 + log(length)) is Postgres's
-- stand-in for BM25's length normalisation and is what makes the AND->OR
-- relaxation usable — see the sprint record for the measured sweep.
--
-- DDL ONLY, purely additive. No row is written, updated or deleted: every
-- column is derived from data already present, so a pre-0029 corpus is in the
-- correct state the moment the column materialises.
--
-- Rollback consequences: a **clean re-tag**. Nothing writes these columns and
-- nothing else references them, so reverting is `DROP INDEX` + `DROP COLUMN`
-- for the nine pairs below and the corpus is untouched. An older image is
-- indifferent to them while they exist — every INSERT in korg names its columns
-- explicitly, and a generated column cannot be written to in any case.
--
-- Cost note: the columns roughly double the stored prose (a tsvector of a
-- document is the same order of size as the document). On the 2026-08-18
-- corpus — 1,312 nodes and 783 comments — that is a few megabytes, and it buys
-- 403 ms -> 8 ms on a full-corpus query.

-- --------------------------------------------------------------------------
-- §1 Node-kind detail tables
-- --------------------------------------------------------------------------

ALTER TABLE workitem ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'WI-' || wi_number || ' korg:' || node_id || ' ' || title), 'A') ||
    setweight(to_tsvector('english', coalesce(content, '') || ' ' || coalesce(details, '')), 'B')
) STORED;

ALTER TABLE sprint_proposal ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'korg:' || node_id || ' ' || title), 'A') ||
    setweight(to_tsvector('english', coalesce(summary, '') || ' ' || coalesce(notes, '')), 'B')
) STORED;

ALTER TABLE card ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'korg:' || node_id || ' ' || title), 'A') ||
    setweight(to_tsvector('english', description), 'B')
) STORED;

ALTER TABLE handoff ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'korg:' || node_id || ' ' || title), 'A') ||
    setweight(to_tsvector('english', coalesce(summary, '') || ' ' || coalesce(body, '')), 'B')
) STORED;

ALTER TABLE program ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'korg:' || node_id || ' ' || title), 'A') ||
    setweight(to_tsvector('english', coalesce(aim, '') || ' ' || coalesce(notes, '')), 'B')
) STORED;

ALTER TABLE report ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'korg:' || node_id || ' ' || source), 'A') ||
    setweight(to_tsvector('english', coalesce(summary, '') || ' ' || coalesce(body, '')), 'B')
) STORED;

ALTER TABLE schedule ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'korg:' || node_id || ' ' || title), 'A') ||
    setweight(to_tsvector('english', coalesce(template, '') || ' ' || coalesce(notes, '')), 'B')
) STORED;

ALTER TABLE link ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'korg:' || node_id || ' ' || coalesce(title, '')), 'A') ||
    setweight(to_tsvector('english', url), 'B')
) STORED;

-- --------------------------------------------------------------------------
-- §2 Comments — one document each, tied to the parent's locator
-- --------------------------------------------------------------------------

ALTER TABLE comment ADD COLUMN search_tsv tsvector GENERATED ALWAYS AS (
    setweight(to_tsvector('english', 'WI-' || node_id || ' korg:' || node_id), 'A') ||
    setweight(to_tsvector('english', body), 'B')
) STORED;

-- --------------------------------------------------------------------------
-- §3 Indexes
-- --------------------------------------------------------------------------

CREATE INDEX workitem_search_idx        ON workitem        USING GIN (search_tsv);
CREATE INDEX sprint_proposal_search_idx ON sprint_proposal USING GIN (search_tsv);
CREATE INDEX card_search_idx            ON card            USING GIN (search_tsv);
CREATE INDEX handoff_search_idx         ON handoff         USING GIN (search_tsv);
CREATE INDEX program_search_idx         ON program         USING GIN (search_tsv);
CREATE INDEX report_search_idx          ON report          USING GIN (search_tsv);
CREATE INDEX schedule_search_idx        ON schedule        USING GIN (search_tsv);
CREATE INDEX link_search_idx            ON link            USING GIN (search_tsv);
CREATE INDEX comment_search_idx         ON comment         USING GIN (search_tsv);

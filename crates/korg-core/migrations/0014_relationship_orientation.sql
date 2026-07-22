-- 0014_relationship_orientation.sql — make `covers` / `finding` direction mean
-- something, and forbid self-edges (WIs #531, #532).
--
-- Sprint 008 made relate() directed, but create_proposal and upsert_report
-- kept inserting (least(id), greatest(id)) — so for every covers/finding edge
-- the stored orientation recorded node-id ordering, not semantics, and
-- `neighbors.direction` was noise. The writers now insert semantically
-- (proposal -> work item, report -> work item); this backfills what they left
-- behind.
--
-- Orientation is recoverable from endpoint kinds: exactly one end of a covers
-- edge is a sprint_proposal, exactly one end of a finding edge is a report.
-- Flipping is loss-free (it rewrites left/right on an existing row) and cannot
-- collide with relationship_unique from 0006: a collision would require the
-- reverse edge to already exist, i.e. the same unordered pair twice under one
-- label, which the same unique constraint has always forbidden.

-- --------------------------------------------------------------------------
-- 1. covers: put the sprint_proposal on the left.
-- --------------------------------------------------------------------------
UPDATE relationship r
SET    left_id = r.right_id, right_id = r.left_id
FROM   node l, node rt
WHERE  l.id = r.left_id AND rt.id = r.right_id
  AND  r.relationship = 'covers'
  AND  rt.kind = 'sprint_proposal' AND l.kind <> 'sprint_proposal';

-- --------------------------------------------------------------------------
-- 2. finding: put the report on the left.
-- --------------------------------------------------------------------------
UPDATE relationship r
SET    left_id = r.right_id, right_id = r.left_id
FROM   node l, node rt
WHERE  l.id = r.left_id AND rt.id = r.right_id
  AND  r.relationship = 'finding'
  AND  rt.kind = 'report' AND l.kind <> 'report';

-- --------------------------------------------------------------------------
-- 3. Legacy covers edges (pre-0008): the bundle is a *work item* titled
--    'Sprint: …', because sprint_proposal did not exist yet. Best-effort and
--    deliberately not asserted — a database whose legacy edges don't match
--    this shape is left alone rather than blocked from starting. Only rows
--    where exactly one endpoint looks like the bundle are touched.
-- --------------------------------------------------------------------------
UPDATE relationship r
SET    left_id = r.right_id, right_id = r.left_id
FROM   workitem lw, workitem rw
WHERE  lw.node_id = r.left_id AND rw.node_id = r.right_id
  AND  r.relationship = 'covers'
  AND  rw.title LIKE 'Sprint:%' AND lw.title NOT LIKE 'Sprint:%';

-- --------------------------------------------------------------------------
-- 4. Postcondition. Asserts only what is structurally guaranteed: no covers
--    edge may have a proposal on the right, and no finding edge a report on
--    the right. (Legacy work-item bundles from step 3 are outside this claim
--    by construction — see docs/api.md.)
-- --------------------------------------------------------------------------
DO $$
DECLARE
    bad_covers  bigint;
    bad_finding bigint;
BEGIN
    SELECT count(*) INTO bad_covers
      FROM relationship r JOIN node rt ON rt.id = r.right_id
     WHERE r.relationship = 'covers' AND rt.kind = 'sprint_proposal';

    SELECT count(*) INTO bad_finding
      FROM relationship r JOIN node rt ON rt.id = r.right_id
     WHERE r.relationship = 'finding' AND rt.kind = 'report';

    IF bad_covers > 0 OR bad_finding > 0 THEN
        RAISE EXCEPTION
            'orientation backfill failed: % covers edge(s) still point at a proposal, % finding edge(s) at a report',
            bad_covers, bad_finding;
    END IF;
END $$;

-- --------------------------------------------------------------------------
-- 5. Self-edges are meaningless for every registry label and actively harmful
--    for depends_on (a node blocking itself never reaches the frontier).
--    Verified zero in production before adding the constraint; the delete is
--    a safety net for any other database.
-- --------------------------------------------------------------------------
DELETE FROM relationship WHERE left_id = right_id;

ALTER TABLE relationship
    ADD CONSTRAINT relationship_no_self_edge CHECK (left_id <> right_id);

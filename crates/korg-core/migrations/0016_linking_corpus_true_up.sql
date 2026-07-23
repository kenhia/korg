-- 0016_linking_corpus_true_up.sql — LB-1: make the live corpus conform to the
-- label registry before LB-2 constrains writes (sprint 022, proposal korg:596,
-- review decisions D-13..D-18 resolved with Ken 2026-07-23).
--
-- One rehearsed pass of data fix-ups plus the provenance/index schema. Like
-- 0014, it ends by asserting its own postcondition and RAISE-ing — which rolls
-- the whole file back under sqlx's per-migration transaction — rather than
-- leaving the corpus half-converted. Every delta was enumerated in advance and
-- rehearsed against a restored nightly dump (sprints/022-.../README.md,
-- docs/operations.md "Rehearsing a data migration").
--
-- Blast radius, verified read-only against production 2026-07-23: ≤32 rows
-- touched; no code or skill reads the retired labels; the five converted
-- bundles surface as archived done proposals hidden by default filters; all
-- depends_on edges are byte-identical (plan-status / refill-queue unaffected).

-- --------------------------------------------------------------------------
-- 1. Off-registry labels are synonyms, not extensions (D-14, D-16).
--    7 `related` + 1 `follows_from` (474->472) collapse into `related-to`.
--    Live-verified: zero of these pairs collide with an existing `related-to`
--    in either orientation, so relationship_pair_label_unique (0006) cannot
--    trip — and if it somehow did, the whole migration would roll back.
--    `related-to` is undirected (D-1): the stored orientation is meaningless
--    and is left exactly as it was, with no canonicalization.
-- --------------------------------------------------------------------------
UPDATE relationship
   SET relationship = 'related-to'
 WHERE relationship IN ('related', 'follows_from');

-- --------------------------------------------------------------------------
-- 2. `part_of` re-invented the built-in subtask field (D-15). The three edges
--    are child -> parent (400/401/402 -> 277) with the child on the left; set
--    the child's parent_node_id from the edge's right end, then drop the edges.
--    parent_node_id is NULL on these WIs today, so this is a clean set. (The
--    workitem table has no updated trigger, so no timestamp is disturbed.)
-- --------------------------------------------------------------------------
UPDATE workitem c
   SET parent_node_id = r.right_id
  FROM relationship r
 WHERE r.relationship = 'part_of'
   AND c.node_id = r.left_id;

DELETE FROM relationship WHERE relationship = 'part_of';

-- --------------------------------------------------------------------------
-- 3. The five pre-0008 `Sprint: …` bundle work items (#108–112) are, in truth,
--    sprint proposals from before the sprint_proposal node kind existed
--    (D-13). Write that history: create one archived done sprint_proposal per
--    bundle — project/timestamps/tags/category carried from the source WI's
--    node, title minus the "Sprint: " prefix, summary = the bundle's own
--    content — and re-point that bundle's `covers` edges from bundle -> member
--    onto proposal -> member. The source WIs stay archived in place; nothing is
--    deleted. Afterwards `covers` is exactly sprint_proposal -> workitem
--    corpus-wide, retiring the "one legacy shape" caveat for good.
-- --------------------------------------------------------------------------
DO $$
DECLARE
    rec      RECORD;
    new_node BIGINT;
BEGIN
    FOR rec IN
        SELECT n.id AS wi_node, n.project_id, n.category, n.tags,
               n.created, n.updated, w.wi_number, w.title AS wi_title, w.content
          FROM node n
          JOIN workitem w ON w.node_id = n.id
         WHERE w.wi_number IN (108, 109, 110, 111, 112)
         ORDER BY w.wi_number
    LOOP
        -- INSERT preserves explicit created/updated (the touch trigger is
        -- BEFORE UPDATE only), so the proposal inherits the bundle's timeline.
        INSERT INTO node (kind, project_id, category, tags, archived, created, updated)
        VALUES ('sprint_proposal', rec.project_id, rec.category, rec.tags,
                TRUE, rec.created, rec.updated)
        RETURNING id INTO new_node;

        INSERT INTO sprint_proposal (node_id, title, summary, status, rank, pinned)
        VALUES (new_node,
                regexp_replace(rec.wi_title, '^Sprint:\s*', ''),
                rec.content,
                'done',
                rec.wi_number,   -- stable, irrelevant once archived + done
                FALSE);

        UPDATE relationship
           SET left_id = new_node
         WHERE left_id = rec.wi_node
           AND relationship = 'covers';
    END LOOP;
END $$;

-- --------------------------------------------------------------------------
-- 4. Provenance columns + label index (D-17 schema half, F-25). Both columns
--    are nullable with no default: NULL honestly means "predates provenance";
--    there is no backfill lie. LB-2 (korg:597) adds the write path that stamps
--    created/origin on NEW edges — and its relate() ON CONFLICT no-op MUST
--    preserve the original created/origin rather than overwriting them.
--    The label column is named `relationship`; nothing indexed it before.
-- --------------------------------------------------------------------------
ALTER TABLE relationship
    ADD COLUMN created TIMESTAMPTZ,
    ADD COLUMN origin  TEXT;

CREATE INDEX relationship_label_idx ON relationship (relationship);

-- --------------------------------------------------------------------------
-- 5. Backfill project on the project-less proposals from the *rule* behind
--    D-18, not a snapshot's node-id list. Rehearsal caught the trap: the
--    2026-07-23 nightly dump held 11 project-less proposals where production
--    already held 7 — four non-terminal ones (kapollo/hv-simulator) had been
--    triaged in the intervening hours. Any hardcoded id list is therefore
--    coupled to the instant it was written. The rule is not: a proposal takes
--    the unanimous project of the WIs it covers; #175 is the lone ambiguous
--    case (kdeskdash 5 vs klams 4) and is Ken's call: kdeskdash. This converges
--    any snapshot to the same honest end-state.
--
--    The touch-updated trigger is disabled across the backfill so these frozen
--    records keep their real `updated` — a project fix-up is not a content
--    edit, and the sprint's whole point is honest provenance. The one genuine
--    cross-project edge (proposal #485 kdeskdash -> WI #484 k-homelab) already
--    has a project and is left untouched.
-- --------------------------------------------------------------------------
ALTER TABLE node DISABLE TRIGGER node_touch_updated;

-- The one ambiguous case, decided by hand.
UPDATE node
   SET project_id = (SELECT id FROM project WHERE name = 'kdeskdash')
 WHERE id = 175 AND kind = 'sprint_proposal' AND project_id IS NULL;

-- Everyone else: the unanimous project of the WIs they cover. A proposal that
-- covers WIs spanning two projects, or covers nothing, is left for a human and
-- caught by the postcondition rather than guessed at.
UPDATE node p
   SET project_id = u.pid
  FROM (
      SELECT cov.left_id AS prop, min(cn.project_id) AS pid
        FROM relationship cov
        JOIN node cn ON cn.id = cov.right_id
       WHERE cov.relationship = 'covers'
       GROUP BY cov.left_id
      HAVING count(DISTINCT cn.project_id) = 1
         AND min(cn.project_id) IS NOT NULL
  ) u
 WHERE p.id = u.prop
   AND p.kind = 'sprint_proposal'
   AND p.project_id IS NULL;

ALTER TABLE node ENABLE TRIGGER node_touch_updated;

-- --------------------------------------------------------------------------
-- 6. Postcondition. Assert the whole end-state and RAISE (rolling back the
--    entire file) if any part missed — refuse to half-apply. Per-label and
--    per-kind *counts* are the rehearsal's job (production drifts, so they are
--    re-baselined, not hardcoded); this block asserts the structural
--    invariants LB-2 will then rely on.
-- --------------------------------------------------------------------------
DO $$
DECLARE
    off_registry bigint;
    covers_bad_l bigint;
    covers_bad_r bigint;
    finding_bad  bigint;
    rev_dup      bigint;
    projectless  bigint;
    parent_wrong bigint;
    partof_left  bigint;
    prov_dirty   bigint;
BEGIN
    -- Every label is now one of the four registry labels.
    SELECT count(*) INTO off_registry
      FROM relationship
     WHERE relationship NOT IN ('covers', 'finding', 'depends_on', 'related-to');

    -- covers is exactly sprint_proposal -> workitem.
    SELECT count(*) INTO covers_bad_l
      FROM relationship r JOIN node l ON l.id = r.left_id
     WHERE r.relationship = 'covers' AND l.kind <> 'sprint_proposal';
    SELECT count(*) INTO covers_bad_r
      FROM relationship r JOIN node rt ON rt.id = r.right_id
     WHERE r.relationship = 'covers' AND rt.kind <> 'workitem';

    -- finding is exactly report -> workitem.
    SELECT count(*) INTO finding_bad
      FROM relationship r
      JOIN node l  ON l.id  = r.left_id
      JOIN node rt ON rt.id = r.right_id
     WHERE r.relationship = 'finding' AND (l.kind <> 'report' OR rt.kind <> 'workitem');

    -- No undirected pair stored in both orientations under related-to.
    SELECT count(*) INTO rev_dup
      FROM relationship a JOIN relationship b
        ON a.relationship = 'related-to' AND b.relationship = 'related-to'
       AND a.left_id = b.right_id AND a.right_id = b.left_id AND a.id < b.id;

    -- Every proposal has a project.
    SELECT count(*) INTO projectless
      FROM sprint_proposal sp JOIN node n ON n.id = sp.node_id
     WHERE n.project_id IS NULL;

    -- The three part_of children now point at #277, and no part_of edge remains.
    SELECT count(*) INTO parent_wrong
      FROM workitem
     WHERE wi_number IN (400, 401, 402)
       AND parent_node_id IS DISTINCT FROM (SELECT node_id FROM workitem WHERE wi_number = 277);
    SELECT count(*) INTO partof_left
      FROM relationship WHERE relationship = 'part_of';

    -- Provenance columns exist and are NULL on every pre-existing edge.
    SELECT count(*) INTO prov_dirty
      FROM relationship WHERE created IS NOT NULL OR origin IS NOT NULL;

    IF off_registry > 0 OR covers_bad_l > 0 OR covers_bad_r > 0 OR finding_bad > 0
       OR rev_dup > 0 OR projectless > 0 OR parent_wrong > 0 OR partof_left > 0
       OR prov_dirty > 0 THEN
        RAISE EXCEPTION
            'LB-1 corpus true-up postcondition failed: off_registry=%, covers_bad_l=%, covers_bad_r=%, finding_bad=%, rev_dup=%, projectless=%, parent_wrong=%, partof_left=%, prov_dirty=%',
            off_registry, covers_bad_l, covers_bad_r, finding_bad, rev_dup,
            projectless, parent_wrong, partof_left, prov_dirty;
    END IF;
END $$;

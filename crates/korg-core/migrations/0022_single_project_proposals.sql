-- 0022_single_project_proposals.sql — WI #967, proposal korg:971, kfdc Phase 0.
--
-- "One proposal, one project" was a convention the refill-queue and start-sprint
-- skills happened to follow. Sprint 043 makes it a rule: `create_proposal`
-- refuses without a project, and a `covers` edge whose two ends are in different
-- projects is refused on both write paths. This file is the data half — the
-- corpus those writes will now be measured against.
--
-- DATA + DDL. Rollback consequences:
--
--   * The four project moves in §1 are not self-reversing. They are recorded
--     here by rule and by name, so undoing one is a hand-written UPDATE; an old
--     image reads them as ordinary rows either way, because a proposal's project
--     has been a plain column since 0011.
--   * The §2 CHECK is the direction that bites. An image built before this can
--     still try to create a project-less proposal (its core has no such check)
--     and would get a raw constraint violation surfaced as `internal` instead of
--     the `invalid_input` sprint 043's core returns. Reversing is
--     `ALTER TABLE node DROP CONSTRAINT node_sprint_proposal_has_project`.
--
-- What is deliberately NOT here: a constraint on cross-project `covers`. It
-- would have to be a trigger (the two projects live in two rows of `node`), and
-- 0016/LB-2 settled that question — edge rules live in core, the one write path
-- both transports share, never in a trigger that can drift from it. Thirteen
-- legacy proposals also still violate the rule and cannot be resolved
-- mechanically (§3), so the constraint could only ever be NOT VALID anyway.
--
-- Re-appliable: §1's WHERE clause is false once it has run, and §2 is guarded.

-- --------------------------------------------------------------------------
-- 1. Backfill the mechanically-resolvable mis-files.
--
--    0016 §5 ran this rule for proposals with NO project. It could not touch a
--    proposal that had the *wrong* project, which is the class left: five
--    pre-project-model proposals filed under the retired `Agent-Plan` umbrella,
--    and one kprojects proposal filed under `claude-cleo`.
--
--    Same rule as 0016, same reason — derived from structure, not from a
--    snapshot's id list, so any snapshot converges to the same end state: a
--    proposal whose covered work items are unanimously in one project, and that
--    project is not the proposal's, moves to it. A proposal spanning two
--    projects is left for a human; guessing which half is "really" the sprint
--    is exactly the judgement a migration must not make.
--
--    Dry-run against production 2026-08-05 — four rows, every target active:
--
--      prop  title                            from         -> to
--      599   korg dogfood fixes               Agent-Plan   -> korg
--      601   kapollo core UX                  Agent-Plan   -> kapollo
--      602   hv-simulator dashboard pass      Agent-Plan   -> hv-simulator
--      747   kprojects installable anywhere   claude-cleo  -> kprojects
--
--    The touch trigger is disabled across the move, as 0016 did: these are
--    closed records (three archived-done, one declined) and a mis-file
--    correction is not a content edit. The sprint's whole point is honest
--    provenance; stamping `updated` would spend it.
-- --------------------------------------------------------------------------
ALTER TABLE node DISABLE TRIGGER node_touch_updated;

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
   AND p.project_id IS DISTINCT FROM u.pid;

ALTER TABLE node ENABLE TRIGGER node_touch_updated;

-- --------------------------------------------------------------------------
-- 2. Every proposal has a project, as a constraint rather than a habit.
--
--    `node.project_id` is shared across every kind and stays nullable — a card
--    or a reading link legitimately has no project — so the rule is expressed
--    per-kind. VALID immediately: 0016 §5 drove project-less proposals to zero
--    and its own postcondition asserts it, so there is nothing to tolerate.
--
--    Belt and braces, not the primary gate. Core returns `invalid_input` naming
--    both selectors (the same division of labour 0021 §3 used for the summary
--    cap); this constraint is what makes the invariant true of the *database*
--    rather than of whichever code last wrote to it.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'node_sprint_proposal_has_project'
    ) THEN
        ALTER TABLE node
            ADD CONSTRAINT node_sprint_proposal_has_project
            CHECK (kind <> 'sprint_proposal' OR project_id IS NOT NULL);
    END IF;
END $$;

COMMENT ON CONSTRAINT node_sprint_proposal_has_project ON node IS
    'A sprint proposal is a single-project bundle: it is what tells a session '
    'which repo to branch in, and what its covered work items are validated '
    'against (#967). Other kinds may be unfiled.';

-- --------------------------------------------------------------------------
-- 3. Postcondition.
--
--    Structural invariants only, never counts — a count assertion fails on a
--    fresh install with zero proposals (0018/0020/0021).
--
--    The residual is reported, not asserted. Thirteen proposals genuinely
--    cover work in two or more projects; four of them are LIVE queue rows
--    (818, 819, 820, 822 as of 2026-08-05). They are Ken's to split or to hand
--    to the program layer once it exists — this migration surfaces the number
--    rather than pretending the corpus is clean or guessing at a fix.
-- --------------------------------------------------------------------------
DO $$
DECLARE
    projectless bigint;
    residual    bigint;
    live        bigint;
BEGIN
    SELECT count(*) INTO projectless
      FROM sprint_proposal sp JOIN node n ON n.id = sp.node_id
     WHERE n.project_id IS NULL;

    IF projectless > 0 THEN
        RAISE EXCEPTION
            'single-project proposals postcondition failed: % proposal(s) still have no project',
            projectless;
    END IF;

    SELECT count(DISTINCT r.left_id) INTO residual
      FROM relationship r
      JOIN node pn ON pn.id = r.left_id
      JOIN node wn ON wn.id = r.right_id
     WHERE r.relationship = 'covers'
       AND pn.project_id IS NOT NULL
       AND wn.project_id IS NOT NULL
       AND pn.project_id <> wn.project_id;

    SELECT count(DISTINCT r.left_id) INTO live
      FROM relationship r
      JOIN node pn ON pn.id = r.left_id
      JOIN node wn ON wn.id = r.right_id
      JOIN sprint_proposal sp ON sp.node_id = r.left_id
     WHERE r.relationship = 'covers'
       AND pn.project_id IS NOT NULL
       AND wn.project_id IS NOT NULL
       AND pn.project_id <> wn.project_id
       AND sp.status IN ('proposed', 'active')
       AND NOT pn.archived;

    IF residual > 0 THEN
        RAISE NOTICE
            'single-project rule now enforced on writes; % legacy proposal(s) still '
            'cover work across projects, % of them live in the queue. They span two or '
            'more projects, so no rule resolves them — split them or wait for the '
            'program layer (#968).', residual, live;
    END IF;
END $$;

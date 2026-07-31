-- 0020_project_notes_and_routing_description.sql — WI #828, implementing the
-- gate-1 field review recorded on WI #757.
--
-- The review's verdict: the schema is mostly innocent, the read surface is
-- guilty. Nine fields all earn their storage; none earns unconditional
-- broadcast. Two defects were real, and this file fixes the storage half of
-- both while #828 fixes the surface half.
--
--   1. `description` had no contract, so the corpus drifted into two
--      incompatible shapes in one column — routing one-liners (60-120 chars)
--      and 300-500 char operational briefs. A cap alone would destroy the
--      briefs; a column alone would leave the drift. So: `notes` for the long
--      form, and `description` becomes a capped routing contract.
--   2. `status` carried two values with zero rows and no distinct semantics.
--
-- DDL + DATA. Rollback consequences, both milder than 0019's:
--
--   * Adding a nullable column is NOT a boundary in the dangerous direction —
--     an image built before this ignores `notes` happily. What an old image
--     WOULD notice is the six rows whose `description` moved: it would render
--     them blank. Nothing breaks; some prose goes missing until re-applied.
--     Reversing is `UPDATE project SET description = notes, notes = NULL` over
--     those rows plus dropping the constraint.
--   * Narrowing `PROJECT_STATUSES` is app-level only. No CHECK is added on
--     `status`, matching how 0018 handled `category` — the vocabulary lives in
--     `vocab.rs`, and this file only asserts the corpus already agrees with it.
--
-- Blast radius: 35 projects on production as of 2026-07-31 — 6 descriptions
-- move, 0 status values change, 1 column added.

-- --------------------------------------------------------------------------
-- 1. `notes` — the home the long form never had.
--
--    Named `notes`, not `summary`: a "summary" longer than the "description"
--    is a naming trap, and the review rejected it on exactly that ground.
--    Unbounded on purpose. Returned by get_project and REST only — never by
--    the lean list, never by the roster — so its length costs nothing to the
--    consumers this whole program is about.
--
--    Explicitly NOT policed by the drift check (#758). The drift-sensitive
--    facts — paths, repos, hosts — live in the structured fields, which is
--    where assertions can be mechanical. Notes are best-effort context.
-- --------------------------------------------------------------------------
ALTER TABLE project ADD COLUMN notes TEXT;

COMMENT ON COLUMN project.notes IS
    'Long-form operational context: deploy topology, build commands, house '
    'conventions. Unbounded. Returned by get_project and REST, never by the '
    'lean list_projects or the MCP instructions roster. Not asserted by the '
    'project-metadata drift check.';

-- --------------------------------------------------------------------------
-- 2. Move the over-length descriptions. MOVE, never truncate.
--
--    Truncating would produce a plausible-looking routing line that nobody
--    wrote and nothing verified — the same failure mode 0019 refused for
--    `kcard`'s path. The data pass (#758's first run) writes the real routing
--    lines; until then these rows read as null, which is honest.
--
--    Six rows on production, not the four the review predicted:
--      klams-mind 528, kprojects 437, agent-skills 333, klams-view 311,
--      kyac 190, homelab-health 183.
--
--    `homelab-health` is worth calling out: the review named it "the corpus's
--    best row", the unprompted worked example of the very contract being
--    defined — and at 183 chars it fails that contract's cap. It trims to ~130
--    without losing either clause, so the cap demands concision rather than
--    forbidding the form. Its original text lands in `notes` and is the model
--    the backfill should start from.
--
--    Trigger disabled: a fix-up is not a content edit (0013, and the same
--    reasoning as 0016, 0018 and 0019).
-- --------------------------------------------------------------------------
ALTER TABLE project DISABLE TRIGGER project_touch_updated;

UPDATE project
   SET notes       = description,
       description = NULL
 WHERE description IS NOT NULL
   AND char_length(description) > 160;

-- --------------------------------------------------------------------------
-- 2b. Converge `status` onto the two-value vocabulary.
--
--     `vocab.rs` narrows PROJECT_STATUSES to (active, archived) in this same
--     release, and update_project validates against it — so a row still holding
--     `maintenance` or `inactive` would fail its next update. 0018 recorded
--     this ordering lesson for `category`; it applies verbatim here, and the
--     true-up must land WITH the enforcement rather than be assumed.
--
--     Production happened to be clean when this was written, because Ken had
--     just moved the last three `inactive` projects to `archived` by hand. That
--     is exactly why converting rather than only asserting matters: a rehearsal
--     against the morning's nightly dump — taken hours before that hand-fix —
--     failed this file's postcondition with off_vocab=3. Any database restored
--     from an older backup, or any dev copy, would have done the same.
--
--     The mapping preserves each value's ACTUAL prior behaviour rather than
--     collapsing both to one side:
--
--       maintenance -> active    It was rail-visible by default (WI #246: the
--                                rail showed active + maintenance), i.e. a live
--                                project in a quiet phase, and a legitimate
--                                target for new work.
--       inactive    -> archived  It was rail-hidden, and Ken's 2026-07-31
--                                ruling is explicit that formerly-inactive
--                                projects are not targets for new work items.
--                                That is what `archived` now means.
--
--     Still inside the trigger-disabled window: this is a vocabulary fix-up,
--     not someone editing a project.
-- --------------------------------------------------------------------------
UPDATE project SET status = 'active'   WHERE status = 'maintenance';
UPDATE project SET status = 'archived' WHERE status = 'inactive';

ALTER TABLE project ENABLE TRIGGER project_touch_updated;

-- --------------------------------------------------------------------------
-- 3. `description` as a capped routing contract.
--
--    160 characters. The contract itself (one line, task-shaped first clause,
--    a "Not X — that's <project>" boundary where a sibling plausibly claims the
--    same work) is prose and lives in the schema descriptions where writers
--    read it; only the length is mechanically enforceable, so only the length
--    is a constraint.
--
--    VALID immediately, unlike 0019's — step 2 guarantees no row exceeds it,
--    and NULL passes. Nothing is being tolerated here, so nothing needs
--    deferring.
-- --------------------------------------------------------------------------
ALTER TABLE project
    ADD CONSTRAINT project_description_routing_line
    CHECK (description IS NULL OR char_length(description) <= 160);

COMMENT ON COLUMN project.description IS
    'Routing contract, one line, <=160 chars. First clause says what work '
    'belongs here, task-shaped ("harness conventions and sprint layout", not '
    '"a repository containing..."). Where a sibling project plausibly claims '
    'the same work, add the boundary: "Not X - that''s <project>." Written for '
    'an agent with zero prior context deciding where a work item goes. No '
    'paths, repos, hosts or build commands - those live in the structured '
    'fields and notes.';

-- --------------------------------------------------------------------------
-- 4. Postcondition.
--
--    Following 0018 and 0019: assert invariants that hold on ANY snapshot,
--    never counts — a count fails on a fresh install with zero projects.
--
--    The status assertion is the one that matters. `vocab.rs` narrows
--    PROJECT_STATUSES to (active, archived) in this same release, so a row
--    still holding `maintenance` or `inactive` would fail its next
--    update_project — the ordering lesson 0018 recorded, applied again. The
--    corpus was already clean when this was written; this refuses to ship if
--    it stops being.
-- --------------------------------------------------------------------------
DO $$
DECLARE
    off_vocab   bigint;
    over_cap    bigint;
    moved       text;
BEGIN
    -- Mirrors korg-core's vocab::PROJECT_STATUSES. A second copy, but a
    -- point-in-time assertion rather than a live authority — same shape as
    -- 0018 naming the category vocabulary inline.
    SELECT count(*) INTO off_vocab
      FROM project WHERE status NOT IN ('active', 'archived');

    SELECT count(*) INTO over_cap
      FROM project WHERE description IS NOT NULL AND char_length(description) > 160;

    IF off_vocab > 0 OR over_cap > 0 THEN
        RAISE EXCEPTION
            'project notes/description postcondition failed: off_vocab=%, over_cap=%',
            off_vocab, over_cap;
    END IF;

    -- Name what moved, so the transient is visible in the deploy log rather
    -- than silent. These rows now read as null until the data pass writes
    -- their routing lines; their prose is intact in `notes`.
    SELECT string_agg(name, ', ' ORDER BY name) INTO moved
      FROM project WHERE notes IS NOT NULL AND description IS NULL;
    IF moved IS NOT NULL THEN
        RAISE NOTICE
            'description moved to notes (awaiting routing lines from the project-metadata pass): %',
            moved;
    END IF;
END $$;

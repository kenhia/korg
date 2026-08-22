-- 0031_parked_proposals_and_programs.sql — WIs #1534 + #1535, proposal
-- korg:1546, slice 1 of program korg:1549 (the kfdc half is #1536/#1540).
--
-- `parked` reaches two more node kinds. #810 settled the semantics for work
-- items and this does not reopen them: parked is **live, not terminal** — it
-- stays in the default read and sorts below a divider, because the point of the
-- status is keeping the work in view. korg:1478 sat `active` for a month with a
-- comment asking people not to pick it up, and korg:1480 sat `holding`, which
-- is true about the motion and wrong about the reason.
--
-- DDL ONLY, no data touched: `parked` is a value nothing has yet, so there is
-- nothing to backfill and no row changes meaning. That is the difference from
-- 0030, which had to correct rows the old default had mislabelled.
--
-- §1 IS A TYPE CONVERSION, NOT A WIDEN, and it is the decision worth recording.
-- `sprint_proposal.status` has been a real PG enum since 0008, so admitting a
-- fifth value was `ALTER TYPE ... ADD VALUE` — one line, and one line with a
-- footnote: `ADD VALUE` carries transaction restrictions (a migration that
-- wanted to *use* the value afterwards would need `sqlx::migrate!`'s
-- `-- no-transaction` escape hatch), so even the small diff is not quite the
-- free change it looks like.
--
-- Converting to TEXT + CHECK was chosen instead, because it is the shape korg
-- already committed to everywhere else and it costs almost nothing here:
--
--   * `program.status` is TEXT + CHECK (0023/0030), and the two kinds now grow
--     the same way. A sixth status is one widen for both, not a widen and an
--     ALTER TYPE with different transaction rules.
--   * Enforcement lands where #526 says it belongs: korg-core's `vocab` is the
--     authority and the DB is the backstop. An enum is a second authority that
--     can only be widened by a migration.
--   * Every read site already casts `status::text` (they had to), so the code
--     diff is a single write site losing its `::sprint_proposal_status` cast.
--
-- Rollback consequences, section by section:
--
--   * §1 (enum -> TEXT + CHECK). An older image reads `status::text` exactly as
--     it does now — the casts it already carries are what make this invisible
--     to it — but its one write site casts to `::sprint_proposal_status`, and
--     that type is gone. So reverting the CODE without reverting the SCHEMA
--     breaks every proposal status write. Reverse in this order: move every
--     `parked` row off `parked` FIRST (see below), recreate the type, then
--     `ALTER COLUMN status TYPE sprint_proposal_status USING status::…`, then
--     re-add 0008's default.
--   * §2 (widened CHECK). Same shape 0030 §1 described: an older image's
--     `program_status_check` has no 'parked', so reverting while parked rows
--     exist makes them unreadable-by-constraint the moment anything rewrites
--     one. Reverse by moving every parked row off 'parked' first, then
--     re-adding 0030's constraint.
--
-- **Where a parked row goes when it is un-parked is NOT derivable, in either
-- direction.** `parked` is a declaration, and the status it replaced is not
-- recoverable from the row: a proposal parked out of `proposed` and one parked
-- out of `active` are indistinguishable afterwards. The `transition` rows (0026)
-- are the record of what each row was — reverse from those, not by picking a
-- plausible default. This is the same rule the code enforces: korg never guesses
-- an un-park target, and neither should a rollback.
--
-- Re-appliable: every statement is unconditional-but-idempotent. §1's USING
-- clause is a no-op on a column that is already TEXT, and both CHECKs are
-- dropped-then-added.

-- --------------------------------------------------------------------------
-- 1. `sprint_proposal.status` becomes TEXT + CHECK, admitting `parked`.
--
--    Ordered so the column is never briefly unconstrained in a way that could
--    admit a bad write: the default is dropped (it is typed as the enum and
--    would block the conversion), the type is converted, the default is
--    restored as a string literal, and the CHECK lands before the transaction
--    commits. `sprint_proposal_status_idx` (0008) is rebuilt automatically by
--    the type change; the 0029 `search_tsv` generated column does not reference
--    `status`, so it is untouched.
--
--    The vocabulary is spelled here in korg-core's order (`vocab::
--    PROPOSAL_STATUSES`), with `parked` last because it is not a lifecycle
--    stage — it is a sideways move reachable from either live status.
-- --------------------------------------------------------------------------
ALTER TABLE sprint_proposal ALTER COLUMN status DROP DEFAULT;

ALTER TABLE sprint_proposal
    ALTER COLUMN status TYPE TEXT USING status::text;

ALTER TABLE sprint_proposal ALTER COLUMN status SET DEFAULT 'proposed';

ALTER TABLE sprint_proposal DROP CONSTRAINT IF EXISTS sprint_proposal_status_check;
ALTER TABLE sprint_proposal
    ADD CONSTRAINT sprint_proposal_status_check
    CHECK (status IN ('proposed', 'active', 'done', 'declined', 'parked'));

-- The type has exactly one dependant and it has just been converted, so this
-- cannot silently leave a column behind. IF EXISTS keeps the file re-appliable.
DROP TYPE IF EXISTS sprint_proposal_status;

-- --------------------------------------------------------------------------
-- 2. `parked` joins the program status vocabulary.
--
--    A CHECK widen, 0030 §1 exactly. Ordered as the lifecycle reads with the
--    off-lifecycle value last: queued -> active <-> holding -> done, parked.
--
--    No default moves. A program is still born `queued` — `parked` is declared
--    by an operator and korg never sets it on its own, so it is not reachable
--    from an INSERT that omits the column, and a default that admitted it would
--    contradict that.
-- --------------------------------------------------------------------------
ALTER TABLE program DROP CONSTRAINT IF EXISTS program_status_check;
ALTER TABLE program
    ADD CONSTRAINT program_status_check
    CHECK (status IN ('queued', 'active', 'holding', 'done', 'parked'));

-- --------------------------------------------------------------------------
-- 3. Postcondition.
--
--    Structural only, never counts — a fresh install has no proposals and no
--    programs at all (0018/0020/0021/0022/0023/0030 all learned this).
--
--    The constraint clauses inspect the definition, as 0030's do. What makes
--    that sufficient here is the pair of clauses above it: the enum is gone and
--    the column is TEXT, so there is no second authority left that could reject
--    a value this CHECK admits. Whether korg's vocabulary and this constraint
--    agree is korg-core's fence to keep, not a migration's.
-- --------------------------------------------------------------------------
DO $$
BEGIN
    IF (SELECT data_type FROM information_schema.columns
         WHERE table_name = 'sprint_proposal' AND column_name = 'status') <> 'text' THEN
        RAISE EXCEPTION 'parked postcondition failed: sprint_proposal.status is not TEXT';
    END IF;

    IF EXISTS (SELECT 1 FROM pg_type WHERE typname = 'sprint_proposal_status') THEN
        RAISE EXCEPTION 'parked postcondition failed: sprint_proposal_status type survives';
    END IF;

    IF (SELECT column_default FROM information_schema.columns
         WHERE table_name = 'sprint_proposal' AND column_name = 'status') NOT LIKE '%proposed%' THEN
        RAISE EXCEPTION 'parked postcondition failed: proposal default was not restored';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'sprint_proposal_status_check'
           AND pg_get_constraintdef(oid) LIKE '%parked%'
    ) THEN
        RAISE EXCEPTION 'parked postcondition failed: proposal CHECK does not admit parked';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'program_status_check'
           AND pg_get_constraintdef(oid) LIKE '%parked%'
    ) THEN
        RAISE EXCEPTION 'parked postcondition failed: program CHECK does not admit parked';
    END IF;
END $$;

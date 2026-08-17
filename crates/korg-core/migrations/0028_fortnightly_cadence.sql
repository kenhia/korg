-- 0028_fortnightly_cadence.sql — WI #1113, proposal korg:1393 (sprint 063).
--
-- One value onto `schedule.cadence`: `fortnightly`.
--
-- 0025 shipped the cadence set #581 wrote before anyone had filed a real
-- schedule: once, weekly, monthly, quarterly, yearly. On the feature's FIRST
-- live use — four schedules filed 2026-08-08 — one of the two recurring
-- requests was "every two weeks, firing Saturday noon", and it had no honest
-- spelling. A 50% miss rate on the only sample that existed.
--
-- **Why neither workaround was acceptable, and why it is structural.** The set
-- mixes two families. `weekly` is the only WEEK-based recurring value, and week
-- intervals are the ones that preserve a weekday; `monthly`/`quarterly`/
-- `yearly` advance by calendar month and drift off it (2026-08-08 is a
-- Saturday, 2026-09-08 is a Tuesday). So "every N weeks on <weekday>" could
-- only be approximated by `weekly` — which is what schedule 1112 was filed as,
-- with a note saying so. `fortnightly` is the missing member of the week
-- family, not a special case bolted on.
--
-- DDL ONLY, purely additive: one widened CHECK. No row is written, updated or
-- deleted — including schedule 1112, whose flip to `fortnightly` is an explicit
-- update through the API after this deploy, not a data migration hidden in a
-- DDL file. (Migrating it here would also be a lie about the header.)
--
-- Rollback consequences: a **clean re-tag** as long as no schedule has been
-- filed `fortnightly` yet. The old image's `CASE` has no `fortnightly` arm, so
-- such a row would compute a NULL `due_at` and fail to deserialize — every
-- schedule read errors, not just that row's. If a fortnightly schedule exists,
-- roll it back to `weekly` BEFORE rolling the image back:
--
--     UPDATE schedule SET cadence = 'weekly' WHERE cadence = 'fortnightly';
--
-- Reversing the constraint itself is the same two statements below with the
-- 0025 list. Doing so with a `fortnightly` row present fails the constraint,
-- which is the safe direction to fail in.
--
-- Re-appliable: the constraint is dropped IF EXISTS before it is added.

ALTER TABLE schedule DROP CONSTRAINT IF EXISTS schedule_cadence_check;

ALTER TABLE schedule ADD CONSTRAINT schedule_cadence_check
    CHECK (cadence IN ('once', 'weekly', 'fortnightly', 'monthly', 'quarterly', 'yearly'));

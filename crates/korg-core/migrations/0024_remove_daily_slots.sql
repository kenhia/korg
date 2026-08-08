-- 0024_remove_daily_slots.sql — WI #965, proposal korg:1075: remove the daily
-- "slots" planning feature whole — the daily plan, its History, and the topics
-- infrastructure that fed it. Ken's call (2026-08-04, scope corrected
-- 2026-08-07): the slots are an unwanted extra surface, and tools like
-- `search_topics` were an active trap for agents. 0012 built this feature on
-- the grave of 0003's timebox slots; this file closes the line. Cards and
-- reading-list links are untouched — what they lose is only the ability to be
-- planned onto a day.
--
-- DDL + DATA. Rollback consequences — this is the destructive direction, the
-- opposite of 0023's:
--
--   * The DELETEs discard real usage data: every daily-plan item ever planned
--     (the topic corpus was a single archived probe). Node-scoped comments and
--     generalized relationships cascade with the nodes, the same way 0012
--     disposed of slots, so `neighbors` cannot dangle afterwards. None of it
--     is recoverable from the schema — reversing this migration means a dump
--     restore. Count the rows before deploying; the sprint record keeps the
--     number.
--   * An image built before this selects from `daily_plan_item` and `topic`
--     and would fail against the dropped tables, so rolling back across this
--     deploy is NOT a clean re-tag — it needs the pre-deploy dump.

-- Plan items before topics: daily_plan_item.source_node_id is ON DELETE
-- RESTRICT and a topic can be a plan source, so the referencing kind goes
-- first. Comments and relationships cascade with each node.
DELETE FROM node WHERE kind = 'daily_plan_item';
DELETE FROM node WHERE kind = 'topic';

DROP TABLE daily_plan_item;
DROP TABLE topic;

-- The 0023 list minus the two retired kinds.
ALTER TABLE node DROP CONSTRAINT node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN (
        'workitem', 'card', 'link', 'sprint_proposal', 'report',
        'handoff', 'program'
    ));

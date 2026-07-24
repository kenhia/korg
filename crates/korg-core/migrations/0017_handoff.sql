-- 0017_handoff.sql — handoffs as first-class nodes (sprint 025, proposal
-- korg:614; plan sprints/planning/2026-07-21-handoff-node-plan.md).
--
-- A handoff is a node like any other (the 0008/0010 pattern): a kind-specific
-- detail table, attached to the work it describes through the generalized
-- `relationship` table (label 'has_handoff', subject -> handoff, registered in
-- korg-core so LB-2's relate() enforces its endpoints). No dedicated FKs to work
-- items or proposals — the same handoff can describe a proposal, several work
-- items, a report, or another handoff. Discovery is relationship-driven, so no
-- index in v1 (a bounded list op waits on a demonstrated need).
--
-- Read-path only additions live in Rust (related_context gains a handoff title
-- join, LB-3); this migration is purely additive — one widened check, one table
-- — so a re-tag fully reverts it if needed.

-- The live kind set is 0012's (which dropped 'slot' and added 'topic' /
-- 'daily_plan_item') plus 'handoff' — not 0010's list.
ALTER TABLE node DROP CONSTRAINT node_kind_check;
ALTER TABLE node
    ADD CONSTRAINT node_kind_check CHECK (kind IN (
        'workitem', 'card', 'link', 'sprint_proposal', 'report',
        'topic', 'daily_plan_item', 'handoff'
    ));

CREATE TABLE handoff (
    node_id BIGINT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
    title   TEXT   NOT NULL,
    summary TEXT   NOT NULL,
    body    TEXT   NOT NULL,   -- Markdown; carries the full state
    CONSTRAINT handoff_title_nonempty   CHECK (btrim(title)   <> ''),
    CONSTRAINT handoff_summary_nonempty CHECK (btrim(summary) <> '')
);

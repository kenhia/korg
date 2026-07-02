-- Comments have always stored against node(id), not card(node_id) — the column
-- name was misleading. Comments are node-scoped (work items, cards, any kind).
-- Rename the column and its index to match reality. Non-destructive.

ALTER TABLE comment RENAME COLUMN card_node_id TO node_id;
ALTER INDEX comment_card_created_idx RENAME TO comment_node_created_idx;

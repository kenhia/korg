-- WI #84 — make relationships idempotent and symmetric.
-- Relationships are undirected (neighbors reads both ends), so canonicalize each
-- edge to left_id < right_id, collapse duplicates, and enforce one edge per
-- (pair, label).

-- 1. Canonicalize orientation: store the lower node id on the left.
UPDATE relationship
   SET left_id = right_id, right_id = left_id
 WHERE left_id > right_id;

-- 2. Drop duplicate edges (same pair + label), keeping the lowest id.
DELETE FROM relationship r
 USING relationship keep
 WHERE r.left_id = keep.left_id
   AND r.right_id = keep.right_id
   AND r.relationship = keep.relationship
   AND r.id > keep.id;

-- 3. Enforce uniqueness per canonical pair + label.
ALTER TABLE relationship
  ADD CONSTRAINT relationship_pair_label_unique
  UNIQUE (left_id, right_id, relationship);

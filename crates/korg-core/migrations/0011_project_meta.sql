-- WI #246 — project metadata: lifecycle status, machine assignment,
-- deploy targets, and category. Defaults keep every existing project
-- 'active' with empty machine lists.
ALTER TABLE project
  ADD COLUMN status    TEXT   NOT NULL DEFAULT 'active',
  ADD COLUMN machines  TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN deploy_to TEXT[] NOT NULL DEFAULT '{}',
  ADD COLUMN category  TEXT;

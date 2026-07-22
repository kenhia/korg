-- 0013_project_touch.sql — advance project.updated on write (WI #529).
--
-- The touch_updated() trigger from 0001 was only ever attached to `node` and
-- `comment`, and update_project never set `updated` itself, so the column sat
-- frozen at creation time. Latent today (ProjectRow doesn't expose timestamps)
-- but a booby trap for anything that starts sorting projects by recency.

CREATE TRIGGER project_touch_updated
    BEFORE UPDATE ON project
    FOR EACH ROW EXECUTE FUNCTION touch_updated();

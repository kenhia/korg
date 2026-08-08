import { test, expect } from "@playwright/test";

// Sprint 055, from #870's node-shape audit punch list.
//
// The audit found three separate items (#1104, #1106, #1107) that were one
// mismatch: `add_comment`/`list_comments` take any node id and `node.tags`/
// `created`/`updated` exist for every kind, but only per-kind pages rendered
// them. `NodePreview` is now where the node-level columns live, so a kind gets
// comments and timestamps by existing rather than by being wired up.
//
// These specs assert that through two kinds that had *no* comment surface at
// all before this sprint — a sprint proposal and a link — because those are the
// ones where the generic renderer is the only thing standing between the data
// and the reader.

test("a sprint proposal's comments and timestamps are reachable from Planning", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = `e2e-prop-comment-${stamp}`;
  const title = `commented proposal ${stamp}`;
  const note = `premise check ${stamp}`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  await request.post("/api/proposals", {
    data: {
      title,
      summary: "a proposal an agent left a note on",
      project_id: pid,
      tags: ["e2e", "audit-870"],
    },
  });

  await page.goto("/planning");
  const card = page.locator(`text=${title}`).first();
  await expect(card).toBeVisible();

  // #1106, the row half: tags render on the card, and so does "when was this
  // queued?" — the queue is ordered by rank, so nothing else says how long a
  // proposal has been sitting there.
  await expect(page.getByText("#audit-870").first()).toBeVisible();
  await expect(page.getByTestId("proposal-queued").first()).toContainText("queued");

  // The title opens the proposal's own preview (sprint 054's affordance).
  await card.click();
  const panel = page.getByTestId("node-preview-panel");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(title);

  // #1106, the detail half: the timestamps the payload always carried.
  await expect(panel.getByTestId("node-preview-stamps")).toContainText("created");

  // #1104 — the gap the audit called the largest real one. Agents comment on
  // proposals routinely and nothing in the web app could show it.
  await expect(panel.getByTestId("comment-list")).toContainText("No comments.");
  await panel.getByTestId("comment-input").fill(note);
  await panel.getByTestId("comment-input").press("Control+Enter");
  await expect(panel.getByTestId("comment-list")).toContainText(note);

  // It is a real node comment, not panel-local state: it survives a reload.
  await page.reload();
  await page.locator(`text=${title}`).first().click();
  await expect(
    page.getByTestId("node-preview-panel").getByTestId("comment-list"),
  ).toContainText(note);
});

test("a reading-list link has a comment surface", async ({ page, request }) => {
  const stamp = Date.now();
  const linkTitle = `e2e link ${stamp}`;
  const note = `skimmed, the useful part is section 3 (${stamp})`;

  await request.post("/api/links", {
    data: { url: `https://example.invalid/${stamp}`, title: linkTitle },
  });

  await page.goto("/reading-list");
  const row = page.locator("li").filter({ hasText: linkTitle });
  await expect(row).toBeVisible();

  // #1107 — the id is the affordance, deliberately not the title: on a reading
  // list the title follows the link, which is the one thing the page is for.
  await row.getByTestId("link-open-detail").click();
  const panel = page.getByTestId("node-preview-panel");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(linkTitle);

  await panel.getByTestId("comment-input").fill(note);
  await panel.getByTestId("comment-input").press("Control+Enter");
  await expect(panel.getByTestId("comment-list")).toContainText(note);
});

import { test, expect } from "@playwright/test";

// Sprint 003: comments are node-scoped and surface on the work-item detail view
// (no need to enter edit mode). Add → visible → delete.

test("add and remove comments on a work item", async ({ page }) => {
  const project = `e2e-wicomment-${Date.now()}`;
  const title = `commented item ${Date.now()}`;
  const note = `agent note ${Date.now()}`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  // Create a work item.
  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByPlaceholder("Content (markdown)").fill("body");
  await page.getByRole("button", { name: "Save" }).first().click();
  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();

  // Open the detail view — the comments section is visible without editing.
  await page.getByRole("row", { name: new RegExp(title) }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await expect(page.getByTestId("comment-list")).toContainText("No comments.");

  // Add a comment (Ctrl-Enter posts).
  await page.getByTestId("comment-input").fill(note);
  await page.getByTestId("comment-input").press("Control+Enter");
  await expect(page.getByTestId("comment-list")).toContainText(note);

  // #1651 — a comment row has to be a card on whatever surface it lands on.
  // The original bug was not subtlety, it was identity: the row painted
  // `--color-bg`, which IS the page tone, so on every container that sets no
  // background of its own the block had no edge at all. Comparing the row's
  // computed background against the page's is what catches that coming back;
  // asserting a literal colour would only pin today's palette.
  const row = page.getByTestId("comment-list").locator("li").first();
  const tones = await row.evaluate((el) => ({
    row: getComputedStyle(el).backgroundColor,
    page: getComputedStyle(document.body).backgroundColor,
  }));
  expect(tones.row).not.toBe(tones.page);

  // Delete it.
  // Two presses: this is irreversible, so it confirms (WI #549). The button's
  // accessible name changes to "Confirm: …" once armed.
  await page.getByRole("button", { name: "Delete comment" }).click();
  await page.getByRole("button", { name: "Confirm: Delete comment" }).click();
  await expect(page.getByTestId("comment-list")).toContainText("No comments.");
});

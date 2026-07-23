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

  // Delete it.
  // Two presses: this is irreversible, so it confirms (WI #549). The button's
  // accessible name changes to "Confirm: …" once armed.
  await page.getByRole("button", { name: "Delete comment" }).click();
  await page.getByRole("button", { name: "Confirm: Delete comment" }).click();
  await expect(page.getByTestId("comment-list")).toContainText("No comments.");
});

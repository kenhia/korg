import { test, expect } from "@playwright/test";

// Create a project + work item, then open its detail and verify the kwi-style
// fields render (Size/Status/Sprint meta + markdown content).

test("create a project and a work item, view detail fields", async ({ page }) => {
  const project = `e2e-${Date.now()}`;
  const title = `spec item ${Date.now()}`;

  await page.goto("/work-items");

  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByPlaceholder("Content (markdown)").fill("**bold** body");
  await page.getByRole("button", { name: "Save" }).first().click();

  const row = page.getByRole("row", { name: new RegExp(title) });
  await expect(row).toBeVisible();

  // Open detail and verify the tuned fields + markdown.
  await row.click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await expect(page.getByText("Size", { exact: true })).toBeVisible();
  await expect(page.getByText("Status", { exact: true })).toBeVisible();
  await expect(page.getByText("Sprint", { exact: true })).toBeVisible();
  await expect(page.locator("strong", { hasText: "bold" })).toBeVisible();

  // Back returns to the list.
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();
});

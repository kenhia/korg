import { test, expect } from "@playwright/test";

// Adding an area via the project rail diamond, then using it in the new-item
// form's Area selector.

test("add an area to a project via the rail", async ({ page }) => {
  const project = `e2e-area-${Date.now()}`;
  const area = `area-${Date.now()}`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  // Click the diamond for this project.
  await page.getByRole("button", { name: `Add area to ${project}` }).click();
  await page.getByPlaceholder("area name").fill(area);
  await page.getByRole("button", { name: "Create" }).click();

  // The new area is selectable in the new-item form.
  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await expect(page.locator("option", { hasText: area })).toBeAttached();
});

import { test, expect } from "@playwright/test";

// Exercises the create-project + create-work-item flow end to end against the
// live API. Uses a unique project name so repeated runs stay independent.

test("create a project and a work item", async ({ page }) => {
  const project = `e2e-${Date.now()}`;
  const title = `spec item ${Date.now()}`;

  await page.goto("/work-items");

  // Create a project via the rail.
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  // Add a work item to it.
  await page.getByRole("button", { name: "+ Work item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByPlaceholder("Content").fill("created by e2e");
  await page.getByRole("button", { name: "Create" }).click();

  // It shows up in the list.
  await expect(page.getByText(title).first()).toBeVisible();
});

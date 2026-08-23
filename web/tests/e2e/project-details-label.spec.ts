import { test, expect } from "@playwright/test";

// WI #1601 — which project am I looking at?
//
// The rail is long enough now that selecting a project low in a category
// scrolls the selection out of view by the time you are reading its work items,
// and the page said nothing about it: the name lived inside the collapsed
// Project Details block, which is exactly where you cannot see it.

test("the selected project's name sits on the Project Details bar", async ({ page }) => {
  const stamp = Date.now();
  const mine = `e2e-label-${stamp}`;

  await page.goto("/work-items");

  // Nothing is selected yet — the bar belongs to a project, so it is absent
  // under "All projects" rather than showing an empty label.
  await expect(page.getByTestId("project-details-name")).toHaveCount(0);

  await page.getByPlaceholder("new project…").fill(mine);
  await page.getByPlaceholder("new project…").press("Enter");
  await page.getByRole("button", { name: mine, exact: true }).click();

  // The label reads while the section is SHUT — that is the whole ask. The
  // `<dl>` inside carries the same name as the project's record; this is the
  // page's own answer to "where am I", and only it survives the collapse.
  const label = page.getByTestId("project-details-name");
  await expect(label).toHaveText(mine);
  await expect(label).toBeVisible();

  // Switching projects moves it, so it cannot go stale on the previous choice.
  const other = `e2e-label-other-${stamp}`;
  await page.getByPlaceholder("new project…").fill(other);
  await page.getByPlaceholder("new project…").press("Enter");
  await page.getByRole("button", { name: other, exact: true }).click();
  await expect(page.getByTestId("project-details-name")).toHaveText(other);
});

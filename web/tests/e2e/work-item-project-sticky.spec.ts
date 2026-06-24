import { test, expect } from "@playwright/test";

// WI #83 — the selected project must stay selected when navigating away to
// another page and back to Work Items. Regression: it reset to the most-recent
// (last-added) project instead of the one the user had chosen.

test("selected project is sticky across navigation away and back", async ({ page }) => {
  const stamp = Date.now();
  const mine = `e2e-sticky-${stamp}`;
  // A second, later-added project. Without the fix the page snaps to this one
  // (recent-project) after navigation, instead of staying on `mine`.
  const other = `e2e-other-${stamp}`;

  await page.goto("/work-items");

  await page.getByPlaceholder("new project…").fill(mine);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: mine, exact: true })).toBeVisible();

  await page.getByPlaceholder("new project…").fill(other);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: other, exact: true })).toBeVisible();

  // Explicitly select MY project (not the most-recently-added `other`).
  await page.getByRole("button", { name: mine, exact: true }).click();
  const mineBtn = page.getByRole("button", { name: mine, exact: true });
  await expect(mineBtn).toHaveAttribute("aria-current", "true");

  // Navigate away to Cards, then back to Work Items.
  await page.goto("/cards");
  await expect(page).toHaveURL(/\/cards/);
  await page.goto("/work-items");

  // The selection must still be MY project, not `other`.
  await expect(page.getByRole("button", { name: mine, exact: true })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await expect(page.getByRole("button", { name: other, exact: true })).toHaveAttribute(
    "aria-current",
    "false",
  );
});

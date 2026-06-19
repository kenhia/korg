import { test, expect } from "@playwright/test";

// The Cut column autohides to a narrow strip and expands on click.

test("cut bucket autohides and expands", async ({ page }) => {
  const title = `cut ${Date.now()}`;

  await page.goto("/cards");
  // Seed a card and move it to Cut via the list view (deterministic).
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");
  await page.getByRole("button", { name: "List" }).click();
  const row = page.getByRole("row", { name: new RegExp(title) });
  await row.locator("select").selectOption("Cut");
  await page.getByRole("button", { name: "Board" }).click();

  // Collapsed: toggle shows "CUT" and the card is not visible.
  const toggle = page.getByTestId("cut-toggle");
  await expect(toggle).toContainText("CUT");
  await expect(page.getByTestId("col-Cut").getByText(title)).toBeHidden();

  // Expand: card becomes visible.
  await toggle.click();
  await expect(page.getByTestId("col-Cut").getByText(title)).toBeVisible();
});

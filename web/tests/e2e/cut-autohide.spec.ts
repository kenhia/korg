import { test, expect } from "@playwright/test";

// The Cut column autohides to a narrow strip and expands on click.

test("cut bucket autohides and expands", async ({ page }) => {
  const title = `cut ${Date.now()}`;

  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  // Move it to Cut via the edit modal (status is edited by opening a card).
  await page.getByTestId("col-Backlog").getByText(title).click();
  await expect(page.getByTestId("card-modal")).toBeVisible();
  await page.getByTestId("card-modal").getByRole("combobox").first().selectOption("Cut");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByTestId("card-modal")).toBeHidden();

  // Collapsed: toggle shows "CUT" and the card is not visible.
  const toggle = page.getByTestId("cut-toggle");
  await expect(toggle).toContainText("CUT");
  await expect(page.getByTestId("col-Cut").getByText(title)).toBeHidden();

  // Expand: card becomes visible.
  await toggle.click();
  await expect(page.getByTestId("col-Cut").getByText(title)).toBeVisible();
});

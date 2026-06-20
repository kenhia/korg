import { test, expect } from "@playwright/test";

// Card filters (search) narrow the board, and the edit modal supports
// project/category/comments.

test("card filter search + comments", async ({ page }) => {
  const uniq = `zzfilter${Date.now()}`;
  await page.goto("/cards");

  await page.getByPlaceholder("New card title…").fill(uniq);
  await page.getByPlaceholder("New card title…").press("Enter");
  await expect(page.getByTestId("col-Backlog").getByText(uniq)).toBeVisible();

  // Search filter narrows to just this card.
  await page.getByTestId("filter-search").fill(uniq);
  await expect(page.getByTestId("col-Backlog").getByText(uniq)).toBeVisible();
  await page.getByRole("button", { name: "Reset filters" }).click();
  await expect(page.getByTestId("filter-search")).toHaveValue("");

  // Open it, set project/category, add a comment.
  await page.getByTestId("col-Backlog").getByText(uniq).click();
  await expect(page.getByTestId("card-modal")).toBeVisible();
  await page.getByTestId("comment-input").fill("a note");
  await page.getByTestId("comment-input").press("Enter");
  await expect(page.getByTestId("comment-list").getByText("a note")).toBeVisible();
});

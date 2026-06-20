import { test, expect } from "@playwright/test";

// URLs in a card's description or comments surface as clickable launch
// links at the bottom of the edit panel (open in a new browser tab).

test("card edit panel surfaces clickable launch links for URLs", async ({ page }) => {
  const title = `links ${Date.now()}`;
  const descUrl = `https://example.com/desc-${Date.now()}`;
  const commentUrl = `https://example.org/note-${Date.now()}`;

  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  // Open the card and put a URL in the description.
  await page.getByTestId("col-Backlog").getByText(title).click();
  await expect(page.getByTestId("card-modal")).toBeVisible();
  await page.getByPlaceholder("Description (markdown)").fill(`see ${descUrl} for details`);

  const links = page.getByTestId("launch-links");
  const descLink = links.getByRole("link", { name: descUrl });
  await expect(descLink).toBeVisible();
  await expect(descLink).toHaveAttribute("href", descUrl);
  await expect(descLink).toHaveAttribute("target", "_blank");

  // A URL added via a comment also appears.
  await page.getByTestId("comment-input").fill(`ref ${commentUrl}`);
  await page.getByTestId("comment-input").press("Control+Enter");
  await expect(links.getByRole("link", { name: commentUrl })).toHaveAttribute("href", commentUrl);
});

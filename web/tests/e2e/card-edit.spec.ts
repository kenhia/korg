import { test, expect } from "@playwright/test";

// Editing a card and clicking Close with unsaved changes prompts before
// discarding; saving persists.
//
// The close control is `dialog-close` since sprint 019: the card editor is now
// built on the shared <Dialog> primitive, which owns the close button (and,
// more to the point, the focus trap and focus restore the hand-rolled overlay
// never had).

test("card edit prompts on dirty close, saves cleanly", async ({ page }) => {
  const title = `edit ${Date.now()}`;
  const edited = `${title} EDITED`;

  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  // Open the card.
  await page.getByTestId("col-Backlog").getByText(title).click();
  await expect(page.getByTestId("card-modal")).toBeVisible();

  // Make it dirty, then try to close -> discard prompt appears.
  await page.getByTestId("edit-title").fill(edited);
  await page.getByTestId("dialog-close").click();
  await expect(page.getByTestId("discard-prompt")).toBeVisible();

  // Keep editing, then Save instead.
  await page.getByRole("button", { name: "Keep editing" }).click();
  await expect(page.getByTestId("discard-prompt")).toBeHidden();
  await page.getByRole("button", { name: "Save" }).click();

  // Modal closed and the new title shows on the board.
  await expect(page.getByTestId("card-modal")).toBeHidden();
  await expect(page.getByText(edited)).toBeVisible();

  // Closing without changes does not prompt.
  await page.getByText(edited).click();
  await expect(page.getByTestId("card-modal")).toBeVisible();
  await page.getByTestId("dialog-close").click();
  await expect(page.getByTestId("card-modal")).toBeHidden();
});

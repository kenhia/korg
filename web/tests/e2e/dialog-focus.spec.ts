import { test, expect } from "@playwright/test";

// WI #548: the three things NodePreview had none of, and the cards modal had
// one of — focus trap, focus restore, Escape.
//
// Built on native `<dialog>`, so these assert the element is actually driven
// with `showModal()`. That distinction is the whole point: a `<dialog open>`
// looks identical on screen and provides none of the behaviour below.

test("the card editor is a modal dialog that restores focus on Escape", async ({
  page,
}) => {
  const title = `dialog probe ${Date.now()}`;
  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  // Focus the opener before activating, so "restore" has something to restore
  // to. A mouse click on a child element never focuses the tile, which makes
  // the restore assertion meaningless rather than wrong.
  const opener = page
    .locator('[data-testid^="card-"]')
    .filter({ hasText: title })
    .first();
  await opener.focus();
  await page.keyboard.press("Enter");

  const dialog = page.getByTestId("dialog");
  await expect(dialog).toBeVisible();

  // `showModal()` puts it in the top layer and marks the element open. The
  // plain `open` attribute would render but give no trap and no Escape.
  expect(await dialog.evaluate((d: HTMLDialogElement) => d.open)).toBe(true);
  expect(await dialog.evaluate((d) => d.matches(":modal"))).toBe(true);

  // Focus moved inside.
  expect(
    await dialog.evaluate((d) => d.contains(document.activeElement)),
  ).toBe(true);

  // Escape closes (the element's `cancel` event) and focus returns to the tile
  // that opened it — the behaviour `<dialog>` gives for free and the hand-built
  // overlay never had.
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(await opener.evaluate((el) => el === document.activeElement)).toBe(true);
});

test("the dialog close button dismisses it", async ({ page }) => {
  const title = `dialog close ${Date.now()}`;
  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  await page.getByTestId("col-Backlog").getByText(title).click();
  await expect(page.getByTestId("card-modal")).toBeVisible();

  await page.getByTestId("dialog-close").click();
  await expect(page.getByTestId("dialog")).toBeHidden();
});

test("a board tile activates with Space as well as Enter", async ({ page }) => {
  const title = `space probe ${Date.now()}`;
  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  // Note the tile is addressed by test id, not by role: `svelte-dnd-action`
  // rewrites its children to role="listitem" (the zone itself becomes
  // role="list"), so a role="button" set here does not survive to the DOM.
  // Reorderable-list semantics are arguably the right ones; what matters for
  // this WI is that the tile is focusable and activates from the keyboard.
  const tile = page
    .locator('[data-testid^="card-"]')
    .filter({ hasText: title })
    .first();

  await tile.focus();
  await page.keyboard.press(" ");
  await expect(page.getByTestId("dialog")).toBeVisible();
});

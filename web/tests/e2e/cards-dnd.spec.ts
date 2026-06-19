import { test, expect } from "@playwright/test";

// Proves the core kanban interaction: dragging a card to another column moves
// it there (status change persisted). svelte-dnd-action is pointer-driven, so
// we drive a stepped manual drag.

test("drag a card from Backlog to Active", async ({ page }) => {
  const title = `drag ${Date.now()}`;

  await page.goto("/cards");
  // Create a fresh card (lands in Backlog at top).
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  const card = page.locator('[data-testid^="card-"]', { hasText: title });
  await expect(card).toBeVisible();
  await card.scrollIntoViewIfNeeded();

  // It starts in Backlog.
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();

  const target = page.getByTestId("col-Active");
  const tbox = await target.boundingBox();
  const sbox = await card.boundingBox();
  if (!tbox || !sbox) throw new Error("missing bounding boxes");

  // Stepped pointer drag so svelte-dnd-action registers the movement, with
  // short settles so the library's rAF keeps up under load.
  await page.mouse.move(sbox.x + sbox.width / 2, sbox.y + sbox.height / 2);
  await page.mouse.down();
  await page.mouse.move(sbox.x + sbox.width / 2, sbox.y + sbox.height / 2 + 8, { steps: 5 });
  await page.waitForTimeout(120);
  await page.mouse.move(tbox.x + tbox.width / 2, tbox.y + 24, { steps: 20 });
  await page.waitForTimeout(120);
  await page.mouse.move(tbox.x + tbox.width / 2, tbox.y + 28, { steps: 5 });
  await page.waitForTimeout(120);
  await page.mouse.up();

  // The card now lives in Active...
  await expect(page.getByTestId("col-Active").getByText(title)).toBeVisible({ timeout: 10000 });
  // ...and the move persisted: reload and it is still in Active.
  await page.reload();
  await expect(page.getByTestId("col-Active").getByText(title)).toBeVisible();
});

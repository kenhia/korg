import { test, expect } from "@playwright/test";

// Dropping a card onto a timebox schedules it as a reference: a chip appears on
// the slot and the card stays in its original column (does NOT move buckets).

test("drop a card onto a slot to schedule it", async ({ page }) => {
  const title = `sched ${Date.now()}`;

  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  const card = page.locator('[data-testid^="card-"]', { hasText: title });
  await expect(card).toBeVisible();
  await card.scrollIntoViewIfNeeded();
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();

  const slot = page.locator('[data-testid^="slot-"]').first();
  await expect(slot).toBeVisible();
  await slot.scrollIntoViewIfNeeded();

  const sbox = await card.boundingBox();
  const dbox = await slot.boundingBox();
  if (!sbox || !dbox) throw new Error("missing boxes");

  await page.mouse.move(sbox.x + sbox.width / 2, sbox.y + sbox.height / 2);
  await page.mouse.down();
  await page.mouse.move(sbox.x + sbox.width / 2, sbox.y + sbox.height / 2 - 8, { steps: 5 });
  await page.waitForTimeout(120);
  await page.mouse.move(dbox.x + dbox.width / 2, dbox.y + dbox.height / 2, { steps: 20 });
  await page.waitForTimeout(120);
  await page.mouse.move(dbox.x + dbox.width / 2, dbox.y + dbox.height / 2 + 2, { steps: 4 });
  await page.waitForTimeout(120);
  await page.mouse.up();

  // A scheduled chip with the card title appears on a slot...
  await expect(
    page.locator('[data-testid^="sched-"]', { hasText: title }),
  ).toBeVisible({ timeout: 10000 });
  // ...and the card is still in Backlog (scheduling is a reference, not a move).
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();
});

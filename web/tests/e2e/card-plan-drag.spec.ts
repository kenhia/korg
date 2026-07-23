import { test, expect } from "@playwright/test";

// Renamed from slot-schedule.spec.ts in sprint 020. Slots were removed in
// migration 0012; this has tested card → daily-plan dragging ever since, under
// a name that described nothing in the codebase.

function isoDate(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

test("drag a card into today's daily plan without moving its board status", async ({
  page,
}) => {
  const title = `planned card ${Date.now()}`;

  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  const card = page.locator('[data-testid^="card-"]', { hasText: title });
  await expect(card).toBeVisible();
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();

  const day = page.getByTestId(`card-plan-day-${isoDate(new Date())}`);
  await expect(day).toBeVisible();

  // Drive the HTML5 drag-and-drop contract directly rather than via
  // `locator.dragTo`. `dragTo` moves a real mouse, so it needs the source and
  // the target on screen at the same time — and it silently drops nothing when
  // they are not. This suite runs against a persistent database, so once the
  // Backlog column has a few days of cards in it the new card sits far below
  // the plan grid, `scrollIntoViewIfNeeded` on the source pushes the target out
  // of view, and the drag lands nowhere. That failure looked like a product bug
  // and was not one: it reproduced 4 runs out of 5 on a populated board and
  // passed every time on an empty one.
  //
  // Sharing one DataTransfer between `dragstart` and `drop` is what the app
  // actually depends on — `dragCard` writes the node id into it and `dropCard`
  // reads it back — so this exercises the real contract, viewport included or
  // not.
  const dataTransfer = await page.evaluateHandle(() => new DataTransfer());
  await card
    .getByRole("button", { name: `Plan ${title}` })
    .dispatchEvent("dragstart", { dataTransfer });
  await day.dispatchEvent("drop", { dataTransfer });

  await expect(day.getByText(title)).toBeVisible({ timeout: 10000 });
  // The whole point of the feature: planning a card does not move it on the
  // board.
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();
});

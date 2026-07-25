import { test, expect } from "@playwright/test";

// Proves the core kanban interaction: dragging a card to another column moves
// it there (status change persisted). svelte-dnd-action is pointer-driven, so
// we drive a stepped manual drag.
//
// WI #580 — two independent causes, both confirmed by measurement.
//
// 1. **The drag became a race against auto-scroll.** `page.mouse` moves a real
//    pointer, so the grab point and the drop point must be on screen *at the
//    same time*. The test used to create its card through the UI, and `add()`
//    posts no rank — so the card takes the server default `rank: 0`, ties with
//    every other card, and sorts LAST on the node_id tie-break: the bottom of
//    Backlog. Scrolling it into view then leaves the target column's drop zone
//    (content-height `min-h-[3rem]`, so it sits at the *top* of the board) far
//    above the viewport. Measured on a populated board — 176 cards in Backlog,
//    720px viewport: grab y=651, target y=**-11709**, page scrolled to 12133.
//
//    That does not fail outright, which is exactly why it read as "flaky":
//    svelte-dnd-action auto-scrolls toward a pointer held at the viewport edge,
//    so the drop sometimes arrives after the board has scrolled back into reach
//    and sometimes does not, depending on how the library's rAF lines up with
//    the fixed waits below and on machine load. A coin toss, not a bug that
//    reproduces on demand.
//
//    Seeding the card with a low rank puts it at the top of Backlog, level with
//    the target column, so both ends are reachable with no scrolling and no race
//    — regardless of how many cards the database holds. Clamping the drop point
//    into the target box was the other candidate and does not work here: when
//    the page is scrolled down that box is entirely above the viewport, so there
//    is no reachable point to clamp to.
//
// 2. **The titles collided.** `drag ${Date.now()}` is unique only if no two runs
//    share a millisecond, and this WI's acceptance command (`--repeat-each=5`)
//    runs its copies in parallel. Two identically-named cards make the `hasText`
//    locator ambiguous and it fails before the drag is even attempted — 2 of 3
//    parallel repeats died this way, while the same three run sequentially all
//    passed. The worker and repeat indices remove it.
//
// The geometry assertion below stays as a tripwire: if some future board state
// puts the ends out of reach again, this fails immediately and says so, rather
// than timing out twenty lines later on a card that never moved.
//
// Not used: svelte-dnd-action's keyboard drag interface (the WI's other option).
// The tile deliberately consumes Enter and Space in the capture phase to open
// the edit modal — see the `tile` snippet in src/routes/cards/+page.svelte — so
// keyboard lifting is unavailable by design, and taking it would mean regressing
// that decision to serve a test. The board view has no search filter to narrow
// itself with either; that panel renders only in the table view. Card creation
// through the UI is covered by six other specs, so seeding this one over the API
// costs no coverage.

test("drag a card from Backlog to Active", async ({ page, request }, testInfo) => {
  // Unique per parallel repeat, not merely per millisecond — see (2) above.
  const title = `drag ${Date.now()}-${testInfo.workerIndex}-${testInfo.repeatEachIndex}`;

  // Seeded at the top of Backlog — see (1) above.
  const created = await request.post("/api/cards", { data: { title, rank: -1 } });
  expect(created.ok()).toBeTruthy();

  await page.goto("/cards");

  const card = page.locator('[data-testid^="card-"]', { hasText: title });
  await expect(card).toBeVisible();

  // It starts in Backlog.
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();

  const target = page.getByTestId("col-Active");
  const tbox = await target.boundingBox();
  const sbox = await card.boundingBox();
  if (!tbox || !sbox) throw new Error("missing bounding boxes");

  const viewport = page.viewportSize();
  if (viewport) {
    const dropY = tbox.y + 24;
    const grabY = sbox.y + sbox.height / 2;
    const bothOnScreen =
      dropY >= 0 && dropY <= viewport.height && grabY >= 0 && grabY <= viewport.height;
    expect(
      bothOnScreen,
      `drag ends are not both on screen (grab y=${grabY}, drop y=${dropY}, viewport ${viewport.height}px) — ` +
        `the board outgrew what one pointer drag can span`,
    ).toBe(true);
  }

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

import { test, expect } from "@playwright/test";

// WI #583 — the Work Items filter dropdowns used to close only by clicking
// their own summary again, so all five could sit open at once. They now
// dismiss the way every other dropdown does.

test("filter dropdowns dismiss on outside click, Escape, and when another opens", async ({
  page,
}) => {
  await page.goto("/work-items");

  const types = page.getByTestId("filter-types");
  const statuses = page.getByTestId("filter-statuses");

  await types.locator("summary").click();
  await expect(types).toHaveJSProperty("open", true);

  // Ticking a box inside the panel is not an outside click — the panel stays
  // up so several options can be toggled in one go.
  await types.getByRole("checkbox").first().click();
  await expect(types).toHaveJSProperty("open", true);

  // Opening another filter closes this one: its summary is outside us.
  await statuses.locator("summary").click();
  await expect(types).toHaveJSProperty("open", false);
  await expect(statuses).toHaveJSProperty("open", true);

  // Escape closes and hands focus back to the summary it came from.
  await page.keyboard.press("Escape");
  await expect(statuses).toHaveJSProperty("open", false);
  await expect(statuses.locator("summary")).toBeFocused();

  // A click on unrelated page chrome closes it too.
  await statuses.locator("summary").click();
  await expect(statuses).toHaveJSProperty("open", true);
  await page.getByPlaceholder("search…").click();
  await expect(statuses).toHaveJSProperty("open", false);

  // The summary's own click is preventDefault'd so that state stays the single
  // source of truth — which would silently cost keyboard operation if Enter and
  // Space did not dispatch a click on a focused <summary>. They do; this is the
  // guard on that assumption.
  await statuses.locator("summary").focus();
  await page.keyboard.press("Enter");
  await expect(statuses).toHaveJSProperty("open", true);
  await page.keyboard.press("Enter");
  await expect(statuses).toHaveJSProperty("open", false);
});

import { test, expect } from "@playwright/test";

// Inline code in a work item's markdown must render WITHOUT literal backticks
// (the Tailwind Typography plugin injects them via code::before/::after by
// default; we override that). Headings get an accent color.

test("work item inline code renders without backtick pseudo-content", async ({ page }) => {
  const project = `code-${Date.now()}`;
  const title = `code item ${Date.now()}`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByPlaceholder("Content (markdown)").fill("has `inline` code");
  await page.getByRole("button", { name: "Save" }).first().click();

  const row = page.getByRole("row", { name: new RegExp(title) });
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();

  const code = page.locator(".prose code", { hasText: "inline" });
  await expect(code).toBeVisible();

  // No backtick injected by ::before / ::after pseudo-elements.
  const before = await code.evaluate((el) => getComputedStyle(el, "::before").content);
  const after = await code.evaluate((el) => getComputedStyle(el, "::after").content);
  expect(before === "none" || before === '""' || before === "normal").toBe(true);
  expect(after === "none" || after === '""' || after === "normal").toBe(true);
});

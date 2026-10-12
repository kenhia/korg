import { test, expect } from "@playwright/test";

// #3761: the new-comment box defaults to 5 lines, and Expand grows the SAME
// textarea to 50 — only its height changes, so typed text survives the
// round-trip. A successful post drops it back to 5 lines.

test("comment box expands and collapses without losing text", async ({ page }) => {
  const project = `e2e-cexpand-${Date.now()}`;
  const title = `expandable item ${Date.now()}`;
  const draft = `line one ${Date.now()}\nline two`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByPlaceholder("Content (markdown)").fill("body");
  await page.getByRole("button", { name: "Save" }).first().click();
  await page.getByRole("row", { name: new RegExp(title) }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();

  const input = page.getByTestId("comment-input");
  await expect(input).toHaveAttribute("rows", "5");
  await input.fill(draft);
  const small = (await input.boundingBox())!.height;

  await page.getByRole("button", { name: "Expand comment box" }).click();
  await expect(input).toHaveAttribute("rows", "50");
  await expect(input).toHaveValue(draft);
  expect((await input.boundingBox())!.height).toBeGreaterThan(small * 5);

  await page.getByRole("button", { name: "Collapse comment box" }).click();
  await expect(input).toHaveAttribute("rows", "5");
  await expect(input).toHaveValue(draft);

  // Expanded, then posted: the box clears and goes back to 5 lines.
  await page.getByRole("button", { name: "Expand comment box" }).click();
  await input.press("Control+Enter");
  await expect(page.getByTestId("comment-list")).toContainText("line two");
  await expect(input).toHaveValue("");
  await expect(input).toHaveAttribute("rows", "5");
});

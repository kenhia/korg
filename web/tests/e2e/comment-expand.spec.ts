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

// #3793: the in-place edit box gets the same 5 ↔ 50 toggle, keeps its buffer
// across it, and opens collapsed again after a save.
test("comment edit box expands and collapses without losing text", async ({ page }) => {
  const project = `e2e-ceditexp-${Date.now()}`;
  const title = `editable expand item ${Date.now()}`;

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

  await page.getByTestId("comment-input").fill("original");
  await page.getByTestId("comment-input").press("Control+Enter");
  await expect(page.getByTestId("comment-list")).toContainText("original");

  await page.getByRole("button", { name: "Edit comment" }).click();
  const edit = page.getByTestId("comment-edit-input");
  await expect(edit).toHaveAttribute("rows", "5");
  await edit.fill("edited line one\nedited line two");
  const small = (await edit.boundingBox())!.height;

  await page.getByRole("button", { name: "Expand edit box" }).click();
  await expect(edit).toHaveAttribute("rows", "50");
  await expect(edit).toHaveValue("edited line one\nedited line two");
  expect((await edit.boundingBox())!.height).toBeGreaterThan(small * 5);

  await page.getByRole("button", { name: "Collapse edit box" }).click();
  await expect(edit).toHaveAttribute("rows", "5");

  // Expanded, then saved: reopening the editor starts collapsed.
  await page.getByRole("button", { name: "Expand edit box" }).click();
  await edit.press("Control+Enter");
  await expect(page.getByTestId("comment-list")).toContainText("edited line two");
  await page.getByRole("button", { name: "Edit comment" }).click();
  await expect(page.getByTestId("comment-edit-input")).toHaveAttribute("rows", "5");
});

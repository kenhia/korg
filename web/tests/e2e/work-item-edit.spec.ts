import { test, expect } from "@playwright/test";

// Edit a work item, archive it, and add/remove a relationship.

test("edit, archive, and relate a work item", async ({ page }) => {
  const project = `e2e-rel-${Date.now()}`;
  const a = `item A ${Date.now()}`;
  const b = `item B ${Date.now()}`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  // Two items to relate.
  for (const t of [a, b]) {
    await page.getByRole("button", { name: "+ New Work Item" }).click();
    await page.getByPlaceholder("Title").fill(t);
    await page.getByPlaceholder("Content (markdown)").fill("body");
    await page.getByRole("button", { name: "Save" }).first().click();
    await expect(page.getByRole("row", { name: new RegExp(t) })).toBeVisible();
  }

  // Open A; capture B's id from its row (the first cell).
  const bRow = page.getByRole("row", { name: new RegExp(b) });
  const bId = (await bRow.locator("td").first().innerText()).trim();

  await page.getByRole("row", { name: new RegExp(a) }).click();
  await expect(page.getByRole("heading", { name: a })).toBeVisible();

  // Edit: change status to resolved.
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("combobox").nth(1).selectOption("resolved"); // Status select
  await page.getByRole("button", { name: "Save" }).first().click();
  await expect(page.getByText("resolved", { exact: true }).first()).toBeVisible();

  // Add a relationship A -> B.
  await page.getByRole("button", { name: "+ Add" }).click();
  await page.getByPlaceholder("label").fill("blocks");
  await page.getByPlaceholder("42").fill(bId);
  await page.getByPlaceholder("42").press("Enter");
  await expect(page.getByText("blocks", { exact: true })).toBeVisible();
  await expect(page.getByText(new RegExp(`#${bId} `))).toBeVisible();

  // Remove it.
  await page.getByRole("button", { name: "Remove" }).click();
  await expect(page.getByText("No relationships.")).toBeVisible();

  // Archive.
  await page.getByRole("button", { name: "Archive" }).click();
  await expect(page.getByText("Archived", { exact: true })).toBeVisible();
});

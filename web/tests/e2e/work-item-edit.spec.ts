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

  // Add a relationship A -> B. The vocabulary is closed (LB-2, D-11): the
  // picker offers exactly the registry labels — no "custom…" escape hatch, and
  // the API would reject one anyway.
  await page.getByRole("button", { name: "+ Add" }).click();
  const labelPicker = page.getByLabel("Relationship label");
  await expect(labelPicker).toHaveValue("related-to");
  // Exactly the registry, in registry order. This list has now gone stale
  // three times for the same structural reason — e2e is out of CI
  // (docs/setup.md), so a registry addition is only noticed the next time
  // someone runs the suite by hand. `has_handoff` joined in sprint 025 (H-1),
  // caught in 028; `includes` and `collides-with` joined in 044, caught in 049;
  // `materializes` joined in 051 and was caught *in* 051, by reading this
  // comment during the ship rather than by a later run; `has_attachment`
  // joined in 056 and was caught by the first suite run afterwards, in 057.
  await expect(labelPicker.locator("option")).toHaveText([
    "covers",
    "finding",
    "depends_on",
    "related-to",
    "has_handoff",
    "includes",
    "collides-with",
    "materializes",
    "has_attachment",
  ]);
  await labelPicker.selectOption({ label: "depends_on" });
  await page.getByPlaceholder("42").fill(bId);
  await page.getByPlaceholder("42").press("Enter");
  await expect(page.getByText("depends_on", { exact: true })).toBeVisible();
  await expect(page.getByText(new RegExp(`#${bId} `))).toBeVisible();

  // Remove it.
  // Two presses: this is irreversible, so it confirms (WI #549). The button's
  // accessible name changes to "Confirm: …" once armed.
  const removeRel = page.getByRole("button", { name: "Remove relationship" });
  await removeRel.click();
  await page.getByRole("button", { name: "Confirm: Remove relationship" }).click();
  await expect(page.getByText("No relationships.")).toBeVisible();

  // Archive.
  await page.getByRole("button", { name: "Archive" }).click();
  await expect(page.getByText("Archived", { exact: true })).toBeVisible();
});

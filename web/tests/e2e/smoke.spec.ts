import { test, expect } from "@playwright/test";

// Structural smoke: the four pages render and the SPA nav works. These
// assertions are data-independent so the gate is deterministic.

test("landing renders the Today overview", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("link", { name: "korg" })).toBeVisible();
  // The slim Today (sprint 050, WI #965): the proposals tray is the page's
  // answer to "which sprint am I on", open by default; cards collapsed.
  await expect(page.getByTestId("tray-toggle-proposals")).toBeVisible();
  await expect(page.getByTestId("tray-toggle-cards")).toBeVisible();
});

test("nav reaches the other pages", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("link", { name: "Cards" }).click();
  await expect(page.getByRole("heading", { name: "Cards" })).toBeVisible();
  await page.getByRole("link", { name: "Work Items" }).click();
  await expect(page.getByRole("heading", { name: "Work items" })).toBeVisible();
  await page.getByRole("link", { name: "Reading" }).click();
  await expect(
    page.getByRole("heading", { name: "Reading list" }),
  ).toBeVisible();
});

test("cards board shows all columns and toggles to list", async ({ page }) => {
  await page.goto("/cards");
  for (const col of ["Backlog", "Research", "OnDeck", "Active", "Done"]) {
    await expect(page.getByText(col, { exact: true }).first()).toBeVisible();
  }
  await expect(page.getByTestId("cut-toggle")).toBeVisible(); // Cut autohidden
  await page.getByRole("button", { name: "List" }).click();
  await expect(page.getByRole("columnheader", { name: "Title" })).toBeVisible();
});

test("work items page has a project rail", async ({ page }) => {
  await page.goto("/work-items");
  await expect(page.getByRole("heading", { name: "Work items" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "All projects" }),
  ).toBeVisible();
});

test("reading list has an add control", async ({ page }) => {
  await page.goto("/reading-list");
  await expect(page.getByPlaceholder("https://…")).toBeVisible();
  await expect(page.getByRole("button", { name: "Add" })).toBeVisible();
});

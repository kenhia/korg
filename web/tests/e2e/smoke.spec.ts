import { test, expect } from "@playwright/test";

// Structural smoke: the four pages render and the SPA nav works. These
// assertions are data-independent so the gate is deterministic.

test("landing renders week + reading list", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("link", { name: "korg" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "This week" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Reading list" })).toBeVisible();
});

test("nav reaches all four pages", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("link", { name: "Cards" }).click();
  await expect(page.getByRole("heading", { name: "Cards" })).toBeVisible();
  await page.getByRole("link", { name: "Work Items" }).click();
  await expect(page.getByRole("heading", { name: "Work items" })).toBeVisible();
  await page.getByRole("link", { name: "Reading" }).click();
  await expect(page.getByRole("heading", { name: "Reading list" })).toBeVisible();
});

test("cards board shows all columns and toggles to list", async ({ page }) => {
  await page.goto("/cards");
  for (const col of ["Backlog", "Research", "OnDeck", "Active", "Done", "Cut"]) {
    await expect(page.getByText(col, { exact: true }).first()).toBeVisible();
  }
  await page.getByRole("button", { name: "List" }).click();
  await expect(page.getByRole("columnheader", { name: "Title" })).toBeVisible();
});

test("work items page has a project selector", async ({ page }) => {
  await page.goto("/work-items");
  await expect(page.getByRole("heading", { name: "Work items" })).toBeVisible();
  await expect(page.locator("select")).toBeVisible();
});

test("reading list has an add control", async ({ page }) => {
  await page.goto("/reading-list");
  await expect(page.getByPlaceholder("https://…")).toBeVisible();
  await expect(page.getByRole("button", { name: "Add" })).toBeVisible();
});

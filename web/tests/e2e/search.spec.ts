import { test, expect } from "@playwright/test";

// Sprint 066 (#1177): full-text search — the header box on every page, the
// results route, and the two things the response says that no other read does.

test("search from the header finds a work item by its body text", async ({ page }) => {
  const token = `zarquon${Date.now()}`;
  const title = `searchable item ${Date.now()}`;
  const project = `e2e-search-${Date.now()}`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  // The token is in the BODY only — a title scan cannot answer this, which is
  // the whole increment #1177 measured.
  await page.getByPlaceholder("Content (markdown)").fill(`the ${token} marker`);
  await page.getByRole("button", { name: "Save" }).first().click();
  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();

  // The box is in the header, so it is reachable from the page you are already
  // on rather than from a tab you have to go to first.
  await page.getByRole("searchbox", { name: "Search korg" }).fill(token);
  await page.getByRole("searchbox", { name: "Search korg" }).press("Enter");

  await expect(page).toHaveURL(/\/search\?q=/);
  await expect(page.getByText(title)).toBeVisible();
  await expect(page.getByText("1 result", { exact: false })).toBeVisible();
});

test("a relaxed answer says so, and the scope toggle survives a re-search", async ({
  page,
}) => {
  const token = `plugh${Date.now()}`;
  const project = `e2e-relax-${Date.now()}`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();
  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(`relaxable ${Date.now()}`);
  await page.getByPlaceholder("Content (markdown)").fill(`the ${token} marker`);
  await page.getByRole("button", { name: "Save" }).first().click();

  // No document holds every one of these words, so the all-terms parse finds
  // nothing and the any-term fallback answers. The page must SAY that: a broad
  // answer passing for a precise one is the failure this feature came from.
  await page.goto(`/search?q=${token}+absolutelynosuchword`);
  await expect(page.getByText("any-term")).toBeVisible();

  // Turning on "search everything" and then editing the query from the header
  // must not silently drop back to the live-only default.
  await page.getByLabel("Search everything").check();
  await expect(page).toHaveURL(/scope=all/);
  await page.getByRole("searchbox", { name: "Search korg" }).fill(token);
  await page.getByRole("searchbox", { name: "Search korg" }).press("Enter");
  await expect(page).toHaveURL(/scope=all/);
});

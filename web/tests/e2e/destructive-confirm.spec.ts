import { test, expect } from "@playwright/test";
import type { APIRequestContext, Page } from "@playwright/test";

// WI #549: nothing irreversible fires on a single click.
//
// Comment delete is the case the WI names explicitly, and the sharpest: a
// deleted comment cannot be recovered through the UI at all.
//
// Each test creates its own work item through the API rather than reaching for
// "the first row". The suite runs fullyParallel against one database, so
// "first" is whatever another worker happened to leave there — which made the
// first draft of these tests flaky for reasons that had nothing to do with the
// behaviour under test.
async function seedWorkItem(request: APIRequestContext) {
  const title = `confirm probe ${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  const res = await request.post("/api/work-items", {
    data: { title, content: "probe", wi_type: "task" },
  });
  expect(res.ok()).toBeTruthy();
  const wi = await res.json();
  return { title, wi_number: wi.wi_number as number };
}

/** Open a work item's detail pane by its number.
 *
 *  Find-by-ID rather than clicking a row: the seeded item has no project, and
 *  the page remembers a sticky project selection, so the row may legitimately
 *  be filtered out of view. Find-by-ID is the feature that exists to reach any
 *  item regardless of the current filters (WI #260). */
async function openWorkItem(page: Page, wi: number) {
  await page.goto("/work-items");
  await page.waitForLoadState("networkidle");
  await page.getByLabel("Find a work item or node by id").fill(String(wi));
  await page.getByRole("button", { name: "Go" }).click();
  await page.getByRole("row", { name: new RegExp(`\\b${wi}\\b`) }).first().click();
}

test("deleting a comment takes two presses", async ({ page, request }) => {
  const { wi_number } = await seedWorkItem(request);
  await openWorkItem(page, wi_number);

  const body = `comment ${Date.now()}`;
  await page.getByTestId("comment-input").fill(body);
  await page.getByRole("button", { name: "Add", exact: true }).click();

  const comment = page.getByTestId("comment-list").getByText(body);
  await expect(comment).toBeVisible();

  const del = page.getByRole("button", { name: "Delete comment" });

  // First press arms rather than deletes.
  await del.click();
  await expect(
    page.getByRole("button", { name: "Confirm: Delete comment" }),
  ).toBeVisible();
  await expect(comment).toBeVisible();

  // Second press commits.
  await page.getByRole("button", { name: "Confirm: Delete comment" }).click();
  await expect(comment).toHaveCount(0);
});

test("an armed delete disarms on blur", async ({ page, request }) => {
  const { wi_number } = await seedWorkItem(request);
  await openWorkItem(page, wi_number);

  const body = `comment ${Date.now()}`;
  await page.getByTestId("comment-input").fill(body);
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByTestId("comment-list").getByText(body)).toBeVisible();

  await page.getByRole("button", { name: "Delete comment" }).click();
  const armed = page.getByRole("button", { name: "Confirm: Delete comment" });
  await expect(armed).toBeVisible();

  // Clicking away must not leave a primed destructive control behind for a
  // later stray click to trigger.
  await page.getByTestId("comment-input").click();
  await expect(armed).toHaveCount(0);
  await expect(page.getByTestId("comment-list").getByText(body)).toBeVisible();
});

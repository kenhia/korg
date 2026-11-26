import { test, expect } from "@playwright/test";

// WI #5024 — the direct `/work-items/<id>` page is where "find by ID" and every
// pasted link land, and it could take comments but not an edit: the route was
// "Show in project", reopen the item, then Edit. It now edits in place with the
// list page's own form.

test("a work item is edited in place from its own page", async ({ page, request }) => {
  const stamp = Date.now();
  const project = `e2e-de-${stamp}`;
  const pid = (await (await request.post("/api/projects", { data: { name: project } })).json())
    .id as number;
  await request.post("/api/areas", { data: { project, name: "front" } });
  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `de item ${stamp}`, content: "before", project_id: pid },
    })
  ).json();

  await page.goto(`/work-items/${wi.wi_number}`);
  await expect(page.getByTestId("node-detail")).toContainText(`de item ${stamp}`);

  await page.getByTestId("wi-edit").click();
  // The list page's form, seeded from the item, with the project's areas.
  await expect(page.getByText(`Edit #${wi.wi_number}`)).toBeVisible();
  await expect(page.getByPlaceholder("Title")).toHaveValue(`de item ${stamp}`);
  await expect(page.locator("option", { hasText: "front" })).toHaveCount(1);

  await page.getByPlaceholder("Title").fill(`de item ${stamp} edited`);
  await page.getByTestId("wi-content").fill("after");
  await page.getByRole("button", { name: "Save" }).first().click();

  // Back on the read view, without having left the page, showing the save.
  await expect(page).toHaveURL(new RegExp(`/work-items/${wi.wi_number}$`));
  const detail = page.getByTestId("node-detail");
  await expect(detail).toContainText(`de item ${stamp} edited`);
  await expect(detail).toContainText("after");

  const saved = await (await request.get(`/api/work-items/${wi.wi_number}`)).json();
  expect(saved.title).toBe(`de item ${stamp} edited`);
  expect(saved.content).toBe("after");
});

test("Cancel leaves the item unchanged", async ({ page, request }) => {
  const stamp = Date.now();
  const project = `e2e-dec-${stamp}`;
  const pid = (await (await request.post("/api/projects", { data: { name: project } })).json())
    .id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `dec item ${stamp}`, content: "x", project_id: pid },
    })
  ).json();

  await page.goto(`/work-items/${wi.wi_number}`);
  await page.getByTestId("wi-edit").click();
  await page.getByRole("button", { name: "Cancel" }).first().click();
  await expect(page.getByTestId("node-detail")).toContainText(`dec item ${stamp}`);
  await expect(page.getByTestId("wi-edit")).toBeVisible();
});

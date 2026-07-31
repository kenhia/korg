import { test, expect } from "@playwright/test";

// Create a project + work item, then open its detail and verify the kwi-style
// fields render (Size/Status/Sprint meta + markdown content).

test("create a project and a work item, view detail fields", async ({ page }) => {
  const project = `e2e-${Date.now()}`;
  const title = `spec item ${Date.now()}`;

  await page.goto("/work-items");

  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByPlaceholder("Content (markdown)").fill("**bold** body");
  await page.getByRole("button", { name: "Save" }).first().click();

  const row = page.getByRole("row", { name: new RegExp(title) });
  await expect(row).toBeVisible();

  // Open detail and verify the tuned fields + markdown.
  await row.click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await expect(page.getByText("Size", { exact: true })).toBeVisible();
  await expect(page.getByText("Status", { exact: true })).toBeVisible();
  await expect(page.getByText("Sprint", { exact: true })).toBeVisible();
  await expect(page.locator("strong", { hasText: "bold" })).toBeVisible();

  // Back returns to the list.
  await page.getByRole("button", { name: "← Back" }).click();
  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();
});

// Row tickers (sprint 030): "there is more on this item than its title".
// Worth a test because both markers are plumbed from row fields that a future
// projection change could quietly drop — the row would still render, just
// without ever admitting the item has a details section or a comment thread.
//
// Project-scoped on purpose: on "All projects" a freshly created item's number
// is past the 500-row cap and the row never appears at all (#762).
test("list rows ticker their comments and details", async ({ page, request }) => {
  const project = `e2e-tickers-${Date.now()}`;
  await request.post("/api/projects", { data: { name: project } });
  const projects = await (await request.get("/api/projects")).json();
  const project_id = projects.find((p: { name: string }) => p.name === project).id;

  const plain = await (
    await request.post("/api/work-items", {
      data: { title: "bare item", content: "no extras", project_id },
    })
  ).json();
  const rich = await (
    await request.post("/api/work-items", {
      data: {
        title: "annotated item",
        content: "has extras",
        details: "a details section",
        project_id,
      },
    })
  ).json();
  await request.post(`/api/nodes/${rich.node_id}/comments`, {
    data: { body: "one comment" },
  });

  await page.goto("/work-items");
  await page.getByRole("button", { name: project, exact: true }).click();

  const richRow = page.getByRole("row", { name: /annotated item/ });
  await expect(richRow).toContainText("💬1");
  await expect(richRow).toContainText("📝");

  // The other half of the assertion: a bare item stays bare, so the markers
  // mean something.
  const plainRow = page.getByRole("row", { name: /bare item/ });
  await expect(plainRow).not.toContainText("💬");
  await expect(plainRow).not.toContainText("📝");
  expect(plain.wi_number).toBeLessThan(rich.wi_number);
});

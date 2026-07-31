import { test, expect } from "@playwright/test";
import type { APIRequestContext } from "@playwright/test";

// WI #570 — the Review completed page.
//
// The two things worth a test here are the two *correctness* holes it closes,
// not its looks. Both are assertions about what the page will not do:
//
//   * it cannot show an open item, however the filters are set; and
//   * closing is a single button with one destination, so there is no way to
//     land an item in a state nobody chose.
//
// Fixtures go in through the API rather than the UI: this spec is about the
// review page, and creating three items through forms would fail for reasons
// that have nothing to do with it.

async function seed(request: APIRequestContext, project: string) {
  await request.post("/api/projects", { data: { name: project } });
  const p = await (await request.get("/api/projects")).json();
  const project_id = p.find((x: { name: string }) => x.name === project).id;

  async function item(title: string, status: string, extra: object = {}) {
    const res = await request.post("/api/work-items", {
      data: { title, content: `body of ${title}`, project_id, ...extra },
    });
    const wi = await res.json();
    if (status !== "open") {
      await request.patch(`/api/work-items/${wi.wi_number}`, {
        data: { wi_status: status },
      });
    }
    return wi.wi_number as number;
  }

  return {
    project_id,
    open: await item("still open item", "open"),
    done: await item("finished item", "done", {
      details: "the detail nobody reads",
    }),
    resolved: await item("implemented item", "resolved"),
  };
}

test("shows only done/resolved, and closing is one click and undoable", async ({
  page,
  request,
}) => {
  const project = `e2e-review-${Date.now()}`;
  const ids = await seed(request, project);
  await request.post(`/api/nodes/${(await (await request.get(`/api/work-items/${ids.done}`)).json()).node_id}/comments`, {
    data: { body: "the comment that changes your mind" },
  });

  await page.goto(`/work-items/review?project=${encodeURIComponent(project)}`);

  // Hole 1: an open item is not merely filtered out, it is unreachable — the
  // rows come from a status-filtered server read.
  await expect(page.getByRole("row", { name: /finished item/ })).toBeVisible();
  await expect(page.getByRole("row", { name: /implemented item/ })).toBeVisible();
  await expect(page.getByRole("row", { name: /still open item/ })).toHaveCount(0);

  // Hole 3: selecting a row shows content, details AND comments together.
  await page.getByRole("row", { name: /finished item/ }).click();
  const detail = page.getByTestId("review-detail");
  await expect(detail.getByText(`body of finished item`)).toBeVisible();
  await expect(detail.getByText("the detail nobody reads")).toBeVisible();
  await expect(
    detail.getByText("the comment that changes your mind"),
  ).toBeVisible();

  // Hole 2: one button, one destination. No dropdown exists on this page.
  await expect(page.locator("select#review-project")).toHaveCount(1); // the only select
  await expect(page.locator("table select")).toHaveCount(0);

  await page.getByTestId(`review-close-${ids.done}`).click();
  await expect(page.getByRole("row", { name: /finished item/ })).toHaveCount(0);
  const after = await (await request.get(`/api/work-items/${ids.done}`)).json();
  expect(after.wi_status).toBe("closed");

  // ...and it is reversible, which is why it does not confirm.
  await page.getByTestId("toast-success").getByRole("button", { name: "Undo" }).click();
  await expect(page.getByRole("row", { name: /finished item/ })).toBeVisible();
  const undone = await (await request.get(`/api/work-items/${ids.done}`)).json();
  expect(undone.wi_status).toBe("done");
});

test("says so when there is nothing to review", async ({ page, request }) => {
  const project = `e2e-review-empty-${Date.now()}`;
  await request.post("/api/projects", { data: { name: project } });

  await page.goto(`/work-items/review?project=${encodeURIComponent(project)}`);
  await expect(page.getByTestId("review-empty")).toContainText(project);
});

test("Work Items links to it, carrying the selected project", async ({
  page,
  request,
}) => {
  const project = `e2e-review-link-${Date.now()}`;
  await seed(request, project);

  await page.goto("/work-items");
  await page.getByRole("button", { name: project, exact: true }).click();
  await page.getByTestId("review-completed-link").click();

  await expect(page).toHaveURL(
    new RegExp(`/work-items/review\\?project=${project}$`),
  );
  await expect(page.getByRole("heading", { name: "Review completed" })).toBeVisible();
  // Scoped to the project it was opened from, so the Project column is dropped.
  await expect(page.getByRole("columnheader", { name: "Project" })).toHaveCount(0);
});

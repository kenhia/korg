import { test, expect, type Page } from "@playwright/test";

// WI #762 — the Work Items list used to keep only the first page of a capped
// read and call it "N items". On the production corpus that meant 500 of 572
// rows, and because the order is `wi_number` ascending the 72 it dropped were
// the *newest* ones. Every filter, the tag chip row and the footer count all
// read from that truncated array, so they were wrong together and silently.
//
// The first spec is the regression test on real data; the rest drive the
// bounded case through mocks, because the page bound (2000 rows) is well past
// anything the database holds.

const ISO = "2026-01-01T00:00:00Z";

/** A row carrying every field the list view touches. */
function row(n: number, over: Record<string, unknown> = {}) {
  return {
    wi_number: n,
    node_id: 100000 + n,
    project: "mockproj",
    area: null,
    wi_type: "bug",
    wi_status: "open",
    wi_tshirt: "S",
    sprint: null,
    title: `mock item ${n}`,
    content: "",
    details: null,
    category: null,
    tags: [],
    parent: null,
    archived: false,
    comment_count: 0,
    created: ISO,
    updated: ISO,
    ...over,
  };
}

/** Serve `corpus` rows from a fake collection of `total`, honouring the
 *  limit/offset the client asks for — so the page's own walk decides how much
 *  of it it actually gets. */
async function mockWorkItems(page: Page, corpus: number, total: number) {
  await page.route("**/api/work-items?**", async (route) => {
    const q = new URL(route.request().url()).searchParams;
    const limit = Number(q.get("limit") ?? 500);
    const offset = Number(q.get("offset") ?? 0);
    const items = Array.from(
      { length: Math.max(0, Math.min(limit, corpus - offset)) },
      (_, i) => row(offset + i + 1),
    );
    await route.fulfill({ json: { items, total, limit, offset } });
  });
}

/** The list is sticky on the last project picked (WI #83), and the bug only
 *  ever bit "All projects". */
async function openAllProjects(page: Page) {
  await page.goto("/work-items");
  await page.getByRole("button", { name: "All projects" }).click();
}

test("the list holds every work item, not the first page", async ({
  page,
  request,
}) => {
  // A brand-new work item takes the highest wi_number there is, which is
  // precisely the row a single ascending, capped read cannot reach. This test
  // fails on a production-sized database before the fix and passes on a small
  // one either way — which is why the bug survived so long.
  const stamp = Date.now();
  const pr = await request.post("/api/projects", {
    data: { name: `e2e-tail-${stamp}` },
  });
  const pid = (await pr.json()).id as number;
  const title = `tail item ${stamp}`;
  const created = await request.post("/api/work-items", {
    data: { title, content: "x", project_id: pid },
  });
  const wi = (await created.json()).wi_number as number;

  await openAllProjects(page);
  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();

  // And the footer is quiet, because nothing is hidden.
  await expect(page.getByTestId("list-count")).toHaveText(/^\d+ items$/);
  await expect(page.getByTestId("list-truncated")).toHaveCount(0);

  // The row is reachable by id as well as by scrolling to it.
  await page.getByPlaceholder("find by ID…").fill(String(wi));
  await page.getByRole("button", { name: "Go", exact: true }).click();
  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();
});

test("a bounded list says so, in amber, and can be finished", async ({
  page,
}) => {
  // 2200 rows against a 2000-row bound: the walk stops short, which is the
  // state the footer has to admit to rather than report as "N items".
  await mockWorkItems(page, 2200, 2200);
  await openAllProjects(page);

  const truncated = page.getByTestId("list-truncated");
  await expect(page.getByTestId("list-count")).toHaveText("2000 items");
  await expect(truncated).toHaveText("· 2000 of 2200 loaded");
  await expect(truncated).toHaveClass(/text-amber-300/);

  await page.getByTestId("load-all").click();

  await expect(page.getByTestId("list-count")).toHaveText("2200 items");
  await expect(truncated).toHaveCount(0);
  await expect(page.getByTestId("load-all")).toHaveCount(0);
});

test("find-by-ID reaches an item the list never loaded", async ({ page }) => {
  // #817's acceptance: "jump to #123 must still work when #123 is not on the
  // current page". Loading everything makes that true by accident today, so it
  // is asserted here against a list that genuinely does not hold the target.
  await mockWorkItems(page, 10, 9999);
  const title = "far past the bound";
  await page.route("**/api/nodes/*", async (route) =>
    route.fulfill({
      json: {
        node_id: 105000,
        kind: "workitem",
        wi_number: 5000,
        title,
        project: "mockproj",
        tags: [],
        archived: false,
        badges: [],
        fields: [],
        body: null,
        body_label: null,
        details: null,
        created: ISO,
        updated: ISO,
      },
    }),
  );
  await page.route("**/api/work-items/5000", async (route) =>
    route.fulfill({
      json: {
        ...row(5000, { title }),
        comments: [],
        comments_truncated: false,
        related: [],
        related_truncated: false,
      },
    }),
  );

  await openAllProjects(page);
  await expect(
    page.getByRole("row", { name: new RegExp(title) }),
  ).toHaveCount(0);

  await page.getByPlaceholder("find by ID…").fill("5000");
  await page.getByRole("button", { name: "Go", exact: true }).click();

  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();
});

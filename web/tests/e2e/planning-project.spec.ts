import { test, expect } from "@playwright/test";

// WI #565 — the proposal queue spans repos, so the Planning page shows each
// proposal's project and can scope the queue to one. WI #536 — the covered
// work items now come from a single get_proposal read instead of a
// neighbors call per proposal plus a client-side join.

test("planning page chips and filters proposals by project, stickily", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const alpha = `e2e-plan-a-${stamp}`;
  const zeta = `e2e-plan-z-${stamp}`;
  const alphaTitle = `alpha sprint ${stamp}`;
  const zetaTitle = `zeta sprint ${stamp}`;
  const wiTitle = `covered item ${stamp}`;

  const aid = (
    await (await request.post("/api/projects", { data: { name: alpha } })).json()
  ).id as number;
  const zid = (
    await (await request.post("/api/projects", { data: { name: zeta } })).json()
  ).id as number;
  const wi = (
    await (
      await request.post("/api/work-items", {
        data: { title: wiTitle, content: "x", project_id: aid },
      })
    ).json()
  ).wi_number as number;
  await request.post("/api/proposals", {
    data: {
      title: alphaTitle,
      summary: "alpha work",
      project_id: aid,
      work_item_numbers: [wi],
    },
  });
  await request.post("/api/proposals", {
    data: { title: zetaTitle, summary: "zeta work", project_id: zid },
  });

  await page.goto("/planning");
  await expect(page.getByText(alphaTitle)).toBeVisible();
  await expect(page.getByText(zetaTitle)).toBeVisible();

  // Each card names its project…
  await expect(
    page.getByTestId("proposal-project").filter({ hasText: alpha }),
  ).toBeVisible();
  // …and the covered work item still renders (via get_proposal).
  await expect(
    page.getByRole("button", { name: new RegExp(`#${wi} ${wiTitle}`) }),
  ).toBeVisible();

  // Filtering scopes the queue server-side.
  const filter = page.getByTestId("planning-project-filter");
  await filter.selectOption(zeta);
  await expect(page.getByText(zetaTitle)).toBeVisible();
  await expect(page.getByText(alphaTitle)).toHaveCount(0);

  // …and the choice survives a reload, like the work-items rail.
  await page.reload();
  await expect(page.getByText(zetaTitle)).toBeVisible();
  await expect(page.getByText(alphaTitle)).toHaveCount(0);

  // Reset so the shared instance isn't left filtered for other specs.
  await page.getByTestId("planning-project-filter").selectOption("");
  await expect(page.getByText(alphaTitle)).toBeVisible();
});

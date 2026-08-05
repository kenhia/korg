import { test, expect } from "@playwright/test";

// WI #565 — the proposal queue spans repos, so the Planning page shows each
// proposal's project and can scope the queue to one. WI #536 — the covered
// work items now come from a single get_proposal read instead of a
// neighbors call per proposal plus a client-side join.
//
// WI #823 (sprint 042) — the `<select>` became a project rail matching Work
// Items', so the scoping interaction is now a click on a named rail entry
// rather than `selectOption`. Each entry carries the planning count, which is
// asserted below: a rail that lists projects but cannot say how much of each
// is spoken for is the dropdown again with extra steps.

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

  // The rail carries each project's planning count (WI #823). alpha has one
  // live proposal covering its single work item, so it reads "1 | 1/1"; zeta
  // has a proposal that covers nothing and no items, so "1 | 0/0".
  const rail = page.getByTestId("planning-project-rail");
  await expect(rail.getByRole("button", { name: new RegExp(`^${alpha}\\s*1 \\| 1/1`) })).toBeVisible();
  await expect(rail.getByRole("button", { name: new RegExp(`^${zeta}\\s*1 \\| 0/0`) })).toBeVisible();

  // Clicking a rail entry scopes the queue.
  await rail.getByRole("button", { name: new RegExp(`^${zeta}`) }).click();
  await expect(page.getByText(zetaTitle)).toBeVisible();
  await expect(page.getByText(alphaTitle)).toHaveCount(0);

  // …and the choice survives a reload, like the work-items rail.
  await page.reload();
  await expect(page.getByText(zetaTitle)).toBeVisible();
  await expect(page.getByText(alphaTitle)).toHaveCount(0);

  // Reset so the shared instance isn't left filtered for other specs.
  await page
    .getByTestId("planning-project-rail")
    .getByRole("button", { name: /^All Projects/ })
    .click();
  await expect(page.getByText(alphaTitle)).toBeVisible();
});

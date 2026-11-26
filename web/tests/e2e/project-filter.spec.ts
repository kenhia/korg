import { test, expect } from "@playwright/test";

// WI #5071 — a filter box under "All projects" on the Work Items rail. Picking
// a project that is not starred meant scrolling the rail down and then back up
// to see its items; typing a fragment of the name should reach it instead.
//
// The names carry a per-run stamp so the match set is exactly the projects
// this test made, whatever else the database holds.

test("the rail filter narrows to a flat alphabetical list of matches", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  // Created out of alphabetical order, and one of them starred, so a result in
  // creation order or with a starred band first would fail the order check.
  const names = [`zz-pf${stamp}-home-c`, `aa-pf${stamp}-home-a`, `mm-pf${stamp}-home-b`];
  const other = `pf${stamp}-hypr`;
  for (const name of [...names, other]) {
    await request.post("/api/projects", { data: { name } });
  }
  await request.patch(`/api/projects/${encodeURIComponent(names[0])}`, {
    data: { starred: true },
  });

  await page.goto("/work-items");
  const filter = page.getByTestId("project-filter");
  await expect(filter).toBeVisible();

  // Case-insensitive: the query is upper-case, the names are not.
  await filter.fill(`PF${stamp}-HOME`);
  const matches = page.getByTestId("project-filter-matches");
  await expect(matches.getByRole("button", { name: /^[a-z]{2}-pf\d+-home-[abc]$/ })).toHaveText([
    `aa-pf${stamp}-home-a`,
    `mm-pf${stamp}-home-b`,
    `zz-pf${stamp}-home-c`,
  ]);
  // One row each — the starred one is not doubled by a band — in name order.
  // Flat: no starred-band rule while filtering, and the non-match is gone.
  await expect(page.getByTestId("starred-band-rule")).toHaveCount(0);
  await expect(page.getByRole("button", { name: other, exact: true })).toHaveCount(0);
  // "All projects" stays reachable while filtering.
  await expect(page.getByRole("button", { name: "All projects", exact: true })).toBeVisible();

  // A match can be picked straight from the filtered list.
  await matches.getByRole("button", { name: `mm-pf${stamp}-home-b`, exact: true }).click();
  await expect(
    page.getByRole("button", { name: `mm-pf${stamp}-home-b`, exact: true }),
  ).toHaveAttribute("aria-current", "true");

  // Nothing matches → said so, not an empty box that looks broken.
  await filter.fill(`pf${stamp}-nothing-here`);
  await expect(matches).toContainText("No project matches");

  // Clearing restores the normal rail, starred band included.
  await filter.fill("");
  await expect(page.getByTestId("project-filter-matches")).toHaveCount(0);
  await expect(page.getByRole("button", { name: other, exact: true })).toBeVisible();
  await expect(page.getByTestId("starred-band-rule")).toBeVisible();
});

test("the rail filter does not survive navigation", async ({ page }) => {
  await page.goto("/work-items");
  await page.getByTestId("project-filter").fill("something");
  await page.goto("/planning");
  await page.goto("/work-items");
  await expect(page.getByTestId("project-filter")).toHaveValue("");
});

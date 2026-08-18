import { test, expect } from "@playwright/test";

// WI #1310 — the selected project is ONE selection, shared by the Work Items
// rail and the Planning rail. Before this, each page kept its own sticky key:
// pick a project in one, switch to the other, and it was still showing a
// different repo's work. Both directions are asserted because the storage is
// written by whichever rail you touch, and a one-way fix is the easy way to get
// this wrong.

test("project selection carries between Work Items and Planning", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const mine = `e2e-scope-a-${stamp}`;
  const other = `e2e-scope-b-${stamp}`;
  for (const name of [mine, other]) {
    await request.post("/api/projects", { data: { name } });
  }

  // Pick on Work Items…
  await page.goto("/work-items");
  await page.getByRole("button", { name: mine, exact: true }).click();
  await expect(page.getByRole("button", { name: mine, exact: true })).toHaveAttribute(
    "aria-current",
    "true",
  );

  // …and Planning is already there.
  await page.goto("/planning");
  const rail = page.getByTestId("planning-project-rail");
  await expect(rail.getByRole("button", { name: new RegExp(`^${mine}`) })).toHaveAttribute(
    "aria-current",
    "true",
  );

  // Now pick the other one on Planning…
  await rail.getByRole("button", { name: new RegExp(`^${other}`) }).click();
  await expect(rail.getByRole("button", { name: new RegExp(`^${other}`) })).toHaveAttribute(
    "aria-current",
    "true",
  );

  // …and Work Items follows. This is the direction Ken's report describes:
  // "I might select k-homelab in one, then switch to the other and it's giving
  // me details about korg".
  await page.goto("/work-items");
  await expect(page.getByRole("button", { name: other, exact: true })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await expect(page.getByRole("button", { name: mine, exact: true })).toHaveAttribute(
    "aria-current",
    "false",
  );

  // "All projects" is a scope like any other and carries the same way — it is
  // stored as a sentinel rather than as "nothing stored", so this is a distinct
  // path from the two above.
  await page.getByRole("button", { name: "All projects", exact: true }).click();
  await page.goto("/planning");
  await expect(
    page.getByTestId("planning-project-rail").getByRole("button", { name: /^All Projects/ }),
  ).toHaveAttribute("aria-current", "true");
});

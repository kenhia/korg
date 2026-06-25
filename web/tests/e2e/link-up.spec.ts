import { test, expect } from "@playwright/test";

// WI #84 — the Link Up page lists Cards, Work Items, and Reading List so the
// user can select items across kinds and relate them. This smoke test (step 3)
// only asserts the page mounts and shows the three lists.

test("link up page shows the three lists", async ({ page }) => {
  await page.goto("/link-up");

  await expect(page.getByRole("heading", { name: "Link Up", exact: true })).toBeVisible();
  await expect(page.getByRole("region", { name: "Cards" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Work Items" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Reading List" })).toBeVisible();
});

// step 4 — a per-list text filter narrows that list.
test("text filter narrows the work items list", async ({ page, request }) => {
  const stamp = Date.now();
  const proj = `e2e-lu-${stamp}`;
  const keep = `lu-keep-${stamp}`;
  const drop = `lu-drop-${stamp}`;

  // Seed a project and two work items directly through the API.
  const pr = await request.post("/api/projects", { data: { name: proj } });
  const pid = (await pr.json()).id as number;
  for (const title of [keep, drop]) {
    await request.post("/api/work-items", { data: { title, content: "x", project_id: pid } });
  }

  await page.goto("/link-up");
  const wiRegion = page.getByRole("region", { name: "Work Items" });
  await expect(wiRegion.getByText(keep, { exact: true })).toBeVisible();
  await expect(wiRegion.getByText(drop, { exact: true })).toBeVisible();

  // Filtering to `keep` hides `drop`.
  await page.getByRole("textbox", { name: "Filter work items by text" }).fill(keep);
  await expect(wiRegion.getByText(keep, { exact: true })).toBeVisible();
  await expect(wiRegion.getByText(drop, { exact: true })).toHaveCount(0);
});

// step 5 — selecting >= 2 items enables the "Link N items" action bar.
test("selecting two items enables the link bar", async ({ page, request }) => {
  const stamp = Date.now();
  const pr = await request.post("/api/projects", { data: { name: `e2e-lu5-${stamp}` } });
  const pid = (await pr.json()).id as number;
  const a = `lu5-a-${stamp}`;
  const b = `lu5-b-${stamp}`;
  for (const title of [a, b]) {
    await request.post("/api/work-items", { data: { title, content: "x", project_id: pid } });
  }

  await page.goto("/link-up");
  const linkBtn = page.getByRole("button", { name: /^Link \d+ items$/ });
  await expect(linkBtn).toBeDisabled();

  const wiRegion = page.getByRole("region", { name: "Work Items" });
  await wiRegion.getByText(a, { exact: true }).click();
  await expect(linkBtn).toBeDisabled(); // 1 selected
  await wiRegion.getByText(b, { exact: true }).click();

  await expect(page.getByRole("button", { name: "Link 2 items", exact: true })).toBeEnabled();
});

// step 6 (ACCEPTANCE) — clicking "Link Items" relates the selected items.
test("linking two selected items relates them", async ({ page, request }) => {
  const stamp = Date.now();
  const pr = await request.post("/api/projects", { data: { name: `e2e-lu6-${stamp}` } });
  const pid = (await pr.json()).id as number;
  const a = `lu6-a-${stamp}`;
  const b = `lu6-b-${stamp}`;
  const ra = await request.post("/api/work-items", {
    data: { title: a, content: "x", project_id: pid },
  });
  const rb = await request.post("/api/work-items", {
    data: { title: b, content: "x", project_id: pid },
  });
  const aNode = (await ra.json()).node_id as number;
  const bNode = (await rb.json()).node_id as number;

  await page.goto("/link-up");
  const wiRegion = page.getByRole("region", { name: "Work Items" });
  await wiRegion.getByText(a, { exact: true }).click();
  await wiRegion.getByText(b, { exact: true }).click();

  await page.getByRole("button", { name: "Link 2 items", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("Linked 2 items");

  // The two items are now related (undirected edge visible from a -> b).
  const neigh = await request.get(`/api/nodes/${aNode}/neighbors`);
  const nodes = (await neigh.json()).map((n: { node_id: number }) => n.node_id);
  expect(nodes).toContain(bNode);
});

// WI #87 — a page-level "Show All" checkbox (default off) hides Closed work
// items and Cut cards; checking it reveals them.
test("show all toggles closed work items and cut cards", async ({ page, request }) => {
  const stamp = Date.now();
  const pr = await request.post("/api/projects", { data: { name: `e2e-lu7-${stamp}` } });
  const pid = (await pr.json()).id as number;

  const openWi = `lu7-open-${stamp}`;
  const closedWi = `lu7-closed-${stamp}`;
  await request.post("/api/work-items", { data: { title: openWi, content: "x", project_id: pid } });
  const rc = await request.post("/api/work-items", {
    data: { title: closedWi, content: "x", project_id: pid },
  });
  const closedNum = (await rc.json()).wi_number as number;
  await request.patch(`/api/work-items/${closedNum}`, { data: { wi_status: "closed" } });

  const liveCard = `lu7-live-${stamp}`;
  const cutCard = `lu7-cut-${stamp}`;
  await request.post("/api/cards", { data: { title: liveCard, project_id: pid } });
  await request.post("/api/cards", { data: { title: cutCard, status: "Cut", project_id: pid } });

  await page.goto("/link-up");
  const wiRegion = page.getByRole("region", { name: "Work Items" });
  const cardRegion = page.getByRole("region", { name: "Cards" });

  // Default (Show All off): active items visible, closed/cut hidden.
  await expect(wiRegion.getByText(openWi, { exact: true })).toBeVisible();
  await expect(cardRegion.getByText(liveCard, { exact: true })).toBeVisible();
  await expect(wiRegion.getByText(closedWi, { exact: true })).toHaveCount(0);
  await expect(cardRegion.getByText(cutCard, { exact: true })).toHaveCount(0);

  // Turn Show All on: closed WI and cut card now appear.
  await page.getByRole("checkbox", { name: "Show All" }).check();
  await expect(wiRegion.getByText(closedWi, { exact: true })).toBeVisible();
  await expect(cardRegion.getByText(cutCard, { exact: true })).toBeVisible();
});

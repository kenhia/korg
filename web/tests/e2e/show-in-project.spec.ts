import { test, expect } from "@playwright/test";

// WI #2380 — Ken types an id to get the full render, then wants the rest of
// that item's project. The full view knew the project (it renders it as a chip)
// and offered no way to act on it, so the only route was retyping `/work-items`
// and re-picking the project from the rail.
//
// The control writes the *shared* scope (`korg.project`, WI #1310) rather than
// a URL parameter, which is why the assertion below is on the rail's
// `aria-current` and not on a query string: the destination is the ordinary
// Work Items page arriving at the ordinary sticky scope.

test("Show in project takes a work item's full view to its project", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const mine = `e2e-sip-${stamp}`;
  const other = `e2e-sip-other-${stamp}`;
  const pid = (await (await request.post("/api/projects", { data: { name: mine } })).json())
    .id as number;
  await request.post("/api/projects", { data: { name: other } });

  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `sip item ${stamp}`, content: "x", project_id: pid },
    })
  ).json();

  // Start from the *other* project being selected, so landing on `mine` cannot
  // be the scope that happened to be stored already — the test would pass
  // against a control that does nothing.
  await page.goto("/work-items");
  await page.getByRole("button", { name: other, exact: true }).click();
  await expect(page.getByRole("button", { name: other, exact: true })).toHaveAttribute(
    "aria-current",
    "true",
  );

  await page.goto(`/work-items/${wi.wi_number}`);
  await expect(page.getByTestId("node-detail")).toBeVisible();
  await page.getByTestId("show-in-project").click();

  await expect(page).toHaveURL(/\/work-items$/);
  await expect(page.getByRole("button", { name: mine, exact: true })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await expect(page.getByRole("button", { name: other, exact: true })).toHaveAttribute(
    "aria-current",
    "false",
  );
});

// The control is opt-in per route, because only the Work Items and Planning
// rails honour the shared scope. A kind whose list page has no project rail
// must not offer a button that writes a scope nobody reads — so the generic
// detail view does not grow one by default.
test("a kind with no project rail gets no Show in project control", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = `e2e-sip-card-${stamp}`;
  const pid = (await (await request.post("/api/projects", { data: { name: project } })).json())
    .id as number;
  const card = await (
    await request.post("/api/cards", {
      data: { title: `sip card ${stamp}`, project_id: pid },
    })
  ).json();

  await page.goto(`/cards/${card.node_id}`);
  await expect(page.getByTestId("node-detail")).toBeVisible();
  // The project is still shown — it is the *control* that is absent.
  await expect(page.getByTestId("node-detail").getByText(project, { exact: true })).toBeVisible();
  await expect(page.getByTestId("show-in-project")).toHaveCount(0);
});

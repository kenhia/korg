import { test, expect } from "@playwright/test";

// Sprint 026 (#611): a handoff is reachable "from the work it belongs to".
// There is no web form to *create* a handoff — it is API/skill-driven — so
// these seed via the API, then drive the UI to open the handoff from each
// owning context through the shared node preview (#610's viewer).

test("open a handoff from a work item's related block", async ({ page, request }) => {
  const stamp = Date.now();
  const wiTitle = `handoff owner ${stamp}`;
  const hTitle = `WI handoff ${stamp}`;

  const wi = await (
    await request.post("/api/work-items", { data: { title: wiTitle, content: "body" } })
  ).json();
  const h = await request.post("/api/handoffs", {
    data: {
      title: hTitle,
      summary: "e2e summary",
      body: "# Handoff\ncross-machine context",
      related_node_ids: [wi.node_id],
    },
  });
  expect(h.ok()).toBeTruthy();

  await page.goto("/work-items");
  // These work items have no project, and the page lands on whichever project
  // was touched most recently — so on a database where another spec just wrote
  // one, the row is filtered out and this fails for reasons that have nothing
  // to do with handoffs. "All projects" makes the seeded row visible whatever
  // else is in the database.
  await page.getByRole("button", { name: "All projects" }).click();
  await page.getByRole("row", { name: new RegExp(wiTitle) }).click();
  await expect(page.getByRole("heading", { name: wiTitle })).toBeVisible();

  // The related block lists the has_handoff edge; clicking it opens the handoff.
  await page.getByRole("button", { name: hTitle }).click();
  const panel = page.getByTestId("node-preview-panel");
  await expect(panel).toBeVisible();
  await expect(panel.getByText(hTitle)).toBeVisible();
  await expect(panel.getByText("cross-machine context")).toBeVisible();
});

test("open a handoff from a proposal card on Planning", async ({ page, request }) => {
  const stamp = Date.now();
  const hTitle = `proposal handoff ${stamp}`;

  // A proposal loads its related block only when it covers something, so seed a
  // covered work item too. A fresh browser context defaults to "All projects",
  // so the card is visible without touching the project filter.
  //
  // Both carry a project because sprint 043 made a proposal single-project and
  // REQUIRED: an untargeted create is now `invalid_input`, and every covered
  // item is checked against the proposal's project. This spec kept passing an
  // untargeted create until sprint 049 ran the suite — the same "e2e is out of
  // CI" lag as the relationship-label list in work-item-edit.spec.ts.
  const project = `handoff-e2e-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `covered ${stamp}`, content: "b", project_id: pid },
    })
  ).json();
  const prop = await (
    await request.post("/api/proposals", {
      data: {
        title: `bundle ${stamp}`,
        summary: "s",
        project_id: pid,
        work_item_numbers: [wi.wi_number],
      },
    })
  ).json();
  const h = await request.post("/api/handoffs", {
    data: { title: hTitle, summary: "s", body: "proposal context", related_node_ids: [prop.node_id] },
  });
  expect(h.ok()).toBeTruthy();

  await page.goto("/planning");
  await expect(page.getByText(`bundle ${stamp}`)).toBeVisible();

  // The card shows the handoff as a 📄 chip; clicking it opens the handoff.
  await page.getByRole("button", { name: new RegExp(hTitle) }).click();
  const panel = page.getByTestId("node-preview-panel");
  await expect(panel).toBeVisible();
  await expect(panel.getByText(hTitle)).toBeVisible();
  await expect(panel.getByText("proposal context")).toBeVisible();
});

// WI #621 — the slide-over is the quick peek and the entry point to a full
// page: a long handoff is unreadable at max-w-md, and commenting on one needs
// somewhere to put the thread. The body stays read-only; a comment is the only
// thing a reader can write.
test("open a handoff on its own page and comment on it", async ({ page, request }) => {
  const stamp = Date.now();
  const wiTitle = `page owner ${stamp}`;
  const hTitle = `page handoff ${stamp}`;

  const wi = await (
    await request.post("/api/work-items", { data: { title: wiTitle, content: "body" } })
  ).json();
  const h = await (
    await request.post("/api/handoffs", {
      data: {
        title: hTitle,
        summary: "read me on a real page",
        body: "## Decisions\nthe long form body",
        related_node_ids: [wi.node_id],
      },
    })
  ).json();

  await page.goto("/work-items");
  await page.getByRole("button", { name: "All projects" }).click();
  await page.getByRole("row", { name: new RegExp(wiTitle) }).click();
  await page.getByRole("button", { name: hTitle }).click();

  // The control on the preview navigates to the dedicated route. Since #982 it
  // is driven by `nodePage(kind)` rather than a hardcoded `handoff`, so the
  // testid is kind-agnostic — programs render the same button.
  await page.getByTestId("open-node-page").click();
  await expect(page).toHaveURL(new RegExp(`/handoffs/${h.node_id}$`));

  await expect(page.getByRole("heading", { name: hTitle, level: 1 })).toBeVisible();
  await expect(page.getByText("read me on a real page")).toBeVisible();
  // Markdown is rendered, not shown as source.
  await expect(page.getByRole("heading", { name: "Decisions" })).toBeVisible();

  // The work it belongs to is reachable from here — arriving by URL is
  // otherwise a dead end.
  await expect(page.getByRole("button", { name: new RegExp(wiTitle) })).toBeVisible();

  // The one thing a reader can write.
  await page.getByTestId("comment-input").fill("picked this up on kai");
  await page.getByTestId("comment-input").press("Control+Enter");
  await expect(
    page.getByTestId("comment-list").getByText("picked this up on kai"),
  ).toBeVisible();

  // A direct visit (bookmark, pasted URL) works the same — the route does not
  // depend on having come through the slide-over.
  await page.goto(`/handoffs/${h.node_id}`);
  await expect(page.getByRole("heading", { name: hTitle, level: 1 })).toBeVisible();
  await expect(
    page.getByTestId("comment-list").getByText("picked this up on kai"),
  ).toBeVisible();
});

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
  const wi = await (
    await request.post("/api/work-items", { data: { title: `covered ${stamp}`, content: "b" } })
  ).json();
  const prop = await (
    await request.post("/api/proposals", {
      data: { title: `bundle ${stamp}`, summary: "s", work_item_numbers: [wi.wi_number] },
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

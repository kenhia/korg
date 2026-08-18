import { test, expect } from "@playwright/test";

// WI #1162 — a proposal's own full-width page, reached by the magnifier beside
// pin and copy. The pop-out on the title stays: the spec asserts BOTH, because
// "keep the right pop-out when clicking title" is half of Ken's ask and the
// easy half to break while adding the other.

test("the magnifier opens a proposal's detail page; the title still pops out", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = `e2e-detail-${stamp}`;
  const title = `detail sprint ${stamp}`;
  const wiTitle = `covered item ${stamp}`;
  // Long enough that the card's collapsed `<details>` is the wrong place to
  // read it — which is the whole reason this page exists.
  const notes = `## Analysis ${stamp}\n\nA measured paragraph the card can only collapse.`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const wi = (
    await (
      await request.post("/api/work-items", {
        data: { title: wiTitle, content: "x", project_id: pid },
      })
    ).json()
  ).wi_number as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: {
        title,
        summary: "the routing contract",
        notes,
        project_id: pid,
        work_item_numbers: [wi],
      },
    })
  ).json();

  await page.goto("/planning");
  const card = page.getByTestId(`proposal-${proposal.node_id}`);
  await expect(card).toBeVisible();

  // The title still opens the slide-over — the quick look Ken asked to keep.
  await card.getByTestId("proposal-open-detail").click();
  await expect(page.getByTestId("node-preview-panel")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("node-preview-panel")).toHaveCount(0);

  // The magnifier navigates to the page.
  await card.getByTestId("proposal-detail-link").click();
  await expect(page).toHaveURL(new RegExp(`/planning/${proposal.node_id}$`));
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await expect(page.getByTestId("proposal-detail-id")).toHaveText(`#${proposal.node_id}`);
  await expect(page.getByTestId("proposal-summary")).toHaveText("the routing contract");

  // `notes` renders as Markdown at full width, not as a collapsed summary.
  await expect(
    page.getByTestId("proposal-notes").getByRole("heading", { name: `Analysis ${stamp}` }),
  ).toBeVisible();

  // The covered items are a list, and each one previews.
  await expect(page.getByTestId("proposal-covered")).toContainText(`#${wi}`);
  await page.getByTestId("proposal-covered").getByRole("button", { name: wiTitle }).click();
  await expect(page.getByTestId("node-preview-panel")).toBeVisible();
  await page.keyboard.press("Escape");

  // The status control drives the same lifecycle as the Planning card, so a
  // proposal read here can be acted on here.
  await page.getByTestId("proposal-status-control").getByRole("button", { name: "active" }).click();
  await expect(
    page.getByTestId("proposal-status-control").getByRole("button", { name: "active" }),
  ).toHaveAttribute("aria-pressed", "true");
  const after = await (await request.get(`/api/proposals/${proposal.node_id}`)).json();
  expect(after.status).toBe("active");

  // Back to the queue, by the control rather than browser history.
  await page.getByTestId("back-to").click();
  await expect(page).toHaveURL(/\/planning$/);
});

// The kind now has a page of its own, so the generic preview offers "Open full
// page ↗" for it — that affordance is driven by `nodePage(kind)`, so this is
// the assertion that the routing table entry landed rather than only the route.
test("the generic preview offers a proposal's full page", async ({ page, request }) => {
  const stamp = Date.now() + 1;
  const title = `preview sprint ${stamp}`;
  // A proposal needs a project (0022 — single-project bundles), so the roster
  // entry comes first even for a proposal that covers nothing.
  const pid = (
    await (
      await request.post("/api/projects", { data: { name: `e2e-detail-p-${stamp}` } })
    ).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title, summary: "s", project_id: pid },
    })
  ).json();

  await page.goto("/planning");
  await page.getByTestId(`proposal-${proposal.node_id}`).getByTestId("proposal-open-detail").click();
  const open = page.getByTestId("open-node-page");
  await expect(open).toHaveAttribute("href", `/planning/${proposal.node_id}`);
  await open.click();
  await expect(page).toHaveURL(new RegExp(`/planning/${proposal.node_id}$`));
});

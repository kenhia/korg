import { test, expect } from "@playwright/test";

// Sprint 070 (#1467) — every node kind is reachable at a URL.
//
// The gap these cover was invisible on the board because the consumers hitting
// it were degrading correctly rather than breaking: kfdc's Net Log rendered
// plain text where a kind had no page, and korg's own awaiting lane linked to a
// *list* page when it could not link to the thing. The one exception was
// `/work-items?wi=<n>`, a URL `nodeHref` produced and the Work Items page never
// read — it landed on the unfiltered list and did nothing with the parameter.
//
// So these are coverage assertions. The interesting ones are the last two: the
// resolver, which is the call a consumer holding a locator cannot make for
// itself, and the comment anchor, which is the half `nodePage` provably cannot
// reconstruct from a search hit.

test("a work item has a page of its own, and the proposal links to it", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = `e2e-routes-${stamp}`;
  const title = `addressable item ${stamp}`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title, content: "reachable at a URL", project_id: pid },
    })
  ).json();

  await page.goto(`/work-items/${wi.wi_number}`);
  const detail = page.getByTestId("node-detail");
  await expect(detail).toBeVisible();
  await expect(detail).toContainText(title);
  await expect(page.getByTestId("node-detail-id")).toContainText(`#${wi.wi_number}`);

  // The link that used to be dead. `/work-items?wi=<n>` landed on the list.
  const proposal = await (
    await request.post("/api/proposals", {
      data: {
        title: `proposal ${stamp}`,
        summary: "covers the addressable item",
        project_id: pid,
        work_item_numbers: [wi.wi_number],
      },
    })
  ).json();
  await page.goto(`/planning/${proposal.node_id}`);
  await page.getByTestId("proposal-covered").getByRole("link").first().click();
  await expect(page).toHaveURL(new RegExp(`/work-items/${wi.wi_number}$`));
  await expect(page.getByTestId("node-detail")).toContainText(title);
});

test("a card, a link and a schedule each render at their own URL", async ({
  page,
  request,
}) => {
  const stamp = Date.now();

  const card = await (
    await request.post("/api/cards", { data: { title: `e2e card ${stamp}` } })
  ).json();
  const link = await (
    await request.post("/api/links", {
      data: { url: `https://example.invalid/${stamp}`, title: `e2e link ${stamp}` },
    })
  ).json();
  const schedule = await (
    await request.post("/api/schedules", {
      data: { title: `e2e schedule ${stamp}`, cadence: "monthly" },
    })
  ).json();

  for (const [path, text] of [
    [`/cards/${card.node_id}`, `e2e card ${stamp}`],
    [`/reading-list/${link.node_id}`, `e2e link ${stamp}`],
    [`/schedules/${schedule.node_id}`, `e2e schedule ${stamp}`],
  ] as const) {
    await page.goto(path);
    await expect(page.getByTestId("node-detail"), path).toContainText(text);
  }
});

test("a route says so when the id is a different kind", async ({ page, request }) => {
  // Node ids are one sequence across every kind, so /cards/<proposal id> is a
  // typo away. Rendering it under the Cards heading would be the wrong answer.
  const stamp = Date.now();
  const project = `e2e-wrongkind-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `not a card ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();

  await page.goto(`/cards/${proposal.node_id}`);
  const notice = page.getByTestId("wrong-kind");
  await expect(notice).toContainText("sprint_proposal");
  await notice.getByRole("link").click();
  await expect(page).toHaveURL(new RegExp(`/planning/${proposal.node_id}$`));
});

test("/n/:node_id resolves a locator to the node's page", async ({ page, request }) => {
  // The whole point of the resolver: the caller supplies an id and no kind.
  const stamp = Date.now();
  const project = `e2e-resolve-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `resolvable ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();

  await page.goto(`/n/${proposal.node_id}`);
  await expect(page).toHaveURL(new RegExp(`/planning/${proposal.node_id}$`));
  await expect(page.getByTestId("proposal-detail-id")).toContainText(
    `#${proposal.node_id}`,
  );
});

test("a comment locator lands on the comment", async ({ page, request }) => {
  // `korg:<node>#comment-<id>` is a spelling korg already prints. This is the
  // half a consumer cannot rebuild: a search hit on a comment carries the
  // owning node's id without its kind.
  const stamp = Date.now();
  const project = `e2e-anchor-${stamp}`;
  const needle = `zamboni resurfacing ${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `anchored ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();
  // Enough comments that landing on the right one is visibly different from
  // landing at the top of the thread.
  for (let i = 0; i < 6; i++) {
    await request.post(`/api/nodes/${proposal.node_id}/comments`, {
      data: { body: `filler ${i} ${stamp}` },
    });
  }
  const target = await (
    await request.post(`/api/nodes/${proposal.node_id}/comments`, {
      data: { body: needle },
    })
  ).json();

  // korg returns the URL; the page follows it rather than deriving one.
  const hits = await (
    await request.get(`/api/search?q=${encodeURIComponent(needle)}&scope=all`)
  ).json();
  const hit = hits.items.find((h: { comment_id: number | null }) => h.comment_id === target.id);
  expect(hit, "the comment should be findable by its own body").toBeTruthy();
  expect(hit.url).toBe(`/planning/${proposal.node_id}#comment-${target.id}`);

  await page.goto(hit.url);
  const anchored = page.locator(`#comment-${target.id}`);
  await expect(anchored).toContainText(needle);
  await expect(anchored).toBeInViewport();
});

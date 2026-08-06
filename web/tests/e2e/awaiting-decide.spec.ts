import { test, expect } from "@playwright/test";

// Sprint 049 (#981) — register a decision in one gesture.
//
// Before this, answering a judgment-shaped ask on the lane cost three surfaces:
// follow the row link, comment on the node's own page, navigate back, clear the
// marker. The lane is meant to be the review inbox, so the answer belongs on it.
//
// The load-bearing assertion is the last one: the comment must land on the
// NODE, where agents read it. A lane-local store would satisfy the UI and lose
// the decision — D-3/D-7 keep the lane a flag and nothing more.

test("answering on the lane comments on the node and clears the ask", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const title = `awaiting decision ${stamp}`;

  const wi = await (
    await request.post("/api/work-items", {
      data: { title, content: "needs a call" },
    })
  ).json();
  const marked = await request.put(`/api/nodes/${wi.node_id}/awaiting`, {
    data: { awaiting: true, note: "proceed or kill?" },
  });
  expect(marked.ok()).toBeTruthy();

  await page.goto("/");
  const row = page.getByTestId(`awaiting-${wi.node_id}`);
  await expect(row).toBeVisible();
  // #980's sweep reaches the lane too.
  await expect(row.getByText(`#${wi.wi_number}`)).toBeVisible();
  await expect(row).toContainText("proceed or kill?");

  // One gesture: type the decision, send.
  await row.getByRole("button", { name: "reply" }).click();
  const decision = `proceed — greenlit ${stamp}`;
  await row.getByTestId("awaiting-reply").fill(decision);
  await row.getByTestId("awaiting-reply-submit").click();

  // The row leaves the lane…
  await expect(page.getByTestId(`awaiting-${wi.node_id}`)).toBeHidden();

  // …the marker is genuinely cleared server-side…
  const awaiting = await (await request.get("/api/awaiting")).json();
  expect(awaiting.some((r: { node_id: number }) => r.node_id === wi.node_id)).toBe(
    false,
  );

  // …and the decision is a comment on the NODE, which is where an agent
  // calling get_work_item will find it. This is the assertion that would fail
  // if the reply were ever stored anywhere lane-local.
  const comments = await (
    await request.get(`/api/nodes/${wi.node_id}/comments`)
  ).json();
  expect(comments.map((c: { body: string }) => c.body)).toContain(decision);
});

test("every kind on the lane is clickable", async ({ page, request }) => {
  const stamp = Date.now() + 1;
  const project = `awaiting-e2e-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;

  // A card has no page of its own — it used to render unlinked, leaving Ken an
  // ask he could not follow. `nodeHref` sends it to the board it lives on.
  const card = await (
    await request.post("/api/cards", {
      data: { title: `awaiting card ${stamp}`, description: "d", project_id: pid },
    })
  ).json();
  await request.put(`/api/nodes/${card.node_id}/awaiting`, {
    data: { awaiting: true, note: "your call" },
  });

  await page.goto("/");
  const row = page.getByTestId(`awaiting-${card.node_id}`);
  await expect(row).toBeVisible();
  await expect(
    row.getByRole("link", { name: `awaiting card ${stamp}` }),
  ).toHaveAttribute("href", "/cards");
});

import { test, expect, type APIRequestContext } from "@playwright/test";

// WI #3294 — Ken, on report #3202: the row's blurb is cut off with no way to
// read the rest. Two causes, and the spec pins both answers. The list row
// clips with CSS (`truncate`), and the stored summary is itself capped at 200
// characters by the writer, so un-clipping the field would still stop
// mid-word. The report's own page shows the BODY, whose first paragraph is
// the whole sentence — once, not repeated as a standfirst (korg:3310 ruling).

/** Reports are written over MCP, not REST — one stateless POST. */
async function createReport(
  request: APIRequestContext,
  args: Record<string, unknown>,
): Promise<number> {
  const res = await request.post("/mcp", {
    headers: { Accept: "application/json, text/event-stream" },
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: { name: "create_report", arguments: args },
    },
  });
  expect(res.ok()).toBeTruthy();
  const body = await res.json();
  expect(body.result.isError).toBeFalsy();
  return JSON.parse(body.result.content[0].text).node_id as number;
}

// Today, not a far-future date: a report dated ahead of every real one would
// become "the latest report" on every other page the suite visits.
const DATE = new Date().toISOString().slice(0, 10);

// Longer than the 200-character cap, and marked at the end: the sentence the
// clipped summary never reaches.
const LEAD =
  "All ten hosts collected; none skipped. One host shows a discrepancy between its " +
  "expected_state record and the live host: the autostart unit the record says must " +
  "stay is gone, and the entry's verified date is now misleading. END-OF-LEAD";

async function fixture(
  request: APIRequestContext,
  stamp: number,
): Promise<number> {
  return createReport(request, {
    // `stamp` alone collides: parallel workers read the same millisecond, and
    // two upserts on one (source, report_date) race each other.
    source: `e2e-detail-${stamp}-${Math.random().toString(36).slice(2, 8)}`,
    report_date: DATE,
    status: "attention",
    summary: LEAD.slice(0, 200),
    body: `## status\n\n**ATTENTION** — ${LEAD}\n\n## findings\n\n- the rest of the report ${stamp}`,
  });
}

test("the #id opens the report's page, which shows the whole lead", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const id = await fixture(request, stamp);

  await page.goto("/daily-reports");
  await page.getByTestId(`report-link-${id}`).click();
  await expect(page).toHaveURL(new RegExp(`/daily-reports/${id}$`));

  await expect(page.getByTestId("report-detail-id")).toHaveText(`#${id}`);
  const body = page.getByTestId("report-body");
  await expect(body).toContainText("END-OF-LEAD");
  await expect(body).toContainText("ATTENTION");
  // Exactly once on the page: the lead is the body's first paragraph, and a
  // standfirst repeating it was ruled noise.
  await expect(page.getByText(/END-OF-LEAD/)).toHaveCount(1);
  await expect(page.getByTestId("report-body")).toContainText(
    `the rest of the report ${stamp}`,
  );

  // The one control the kind has, on the page you read it in.
  const reviewed = page.getByTestId("report-detail-reviewed");
  await expect(reviewed).toHaveAttribute("aria-pressed", "false");
  await reviewed.click();
  await expect(reviewed).toHaveAttribute("aria-pressed", "true");
  await page.reload();
  await expect(page.getByTestId("report-detail-reviewed")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("an open row wraps its summary instead of clipping it", async ({
  page,
  request,
}) => {
  const id = await fixture(request, Date.now());

  await page.goto("/daily-reports");
  const summary = page.getByTestId(`report-summary-${id}`);
  const toggle = page.locator("button[aria-expanded]", { has: summary });
  // Only the newest row arrives open, and today has several; make it explicit.
  if ((await toggle.getAttribute("aria-expanded")) !== "true")
    await summary.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(summary).toHaveCSS("white-space", "normal");
  await expect(summary).toHaveCSS("text-overflow", "clip");

  // Shut again, it goes back to one line — the list stays a list.
  await summary.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(summary).toHaveCSS("white-space", "nowrap");
});

test("at phone width the lead is readable without scrolling sideways", async ({
  page,
  request,
}) => {
  const id = await fixture(request, Date.now());

  await page.setViewportSize({ width: 375, height: 800 });
  await page.goto(`/daily-reports/${id}`);
  await expect(page.getByTestId("report-body")).toContainText("END-OF-LEAD");
  const overflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth -
      document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});

test("an id that is not a report says what it is", async ({
  page,
  request,
}) => {
  // The generic view this page replaced answered this case; graduating must
  // not answer it worse.
  const stamp = Date.now();
  const project = `e2e-reportkind-${stamp}`;
  const pid = (
    await (
      await request.post("/api/projects", { data: { name: project } })
    ).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `not a report ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();

  await page.goto(`/daily-reports/${proposal.node_id}`);
  const notice = page.getByTestId("wrong-kind");
  await expect(notice).toContainText("sprint_proposal");
  await notice.getByRole("link").click();
  await expect(page).toHaveURL(new RegExp(`/planning/${proposal.node_id}$`));
});

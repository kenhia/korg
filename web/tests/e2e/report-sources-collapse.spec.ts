import { test, expect, type APIRequestContext } from "@playwright/test";

// WI #1523 — the Sources panel was pushing the newest daily report below the
// fold. It is collapsed now, and the assertion that matters is what SURVIVES
// the collapse: #950's whole premise is that a source which stopped filing is
// itself an alert, so a bar that hides staleness would have traded the bug for
// a worse one.

/**
 * korg creates reports over MCP, not REST (`/api/reports` is read-only), and
 * a source has no freshness until something has filed. `/mcp` is stateless
 * JSON — one POST, no handshake — so this is a call, not a harness.
 */
async function createReport(
  request: APIRequestContext,
  source: string,
  report_date: string,
): Promise<void> {
  const res = await request.post("/mcp", {
    headers: { Accept: "application/json, text/event-stream" },
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: {
        name: "create_report",
        arguments: {
          source,
          report_date,
          status: "ok",
          summary: `e2e ${source}`,
          body: "filed by the e2e suite",
        },
      },
    },
  });
  expect(res.ok()).toBeTruthy();
  expect((await res.json()).result.isError).toBeFalsy();
}

/** `YYYY-MM-DD`, `daysAgo` days back. */
function day(daysAgo: number): string {
  return new Date(Date.now() - daysAgo * 86_400_000).toISOString().slice(0, 10);
}

test("the Sources bar collapses but keeps every scheduled source's freshness", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const fresh = `e2e-fresh-${stamp}`;
  const stale = `e2e-stale-${stamp}`;
  const retired = `e2e-retired-${stamp}`;

  try {
    // A declared cadence beats an inferred one, so one report plus a PATCH is
    // the whole fixture: today's report is fresh, a month-old one is stale.
    await createReport(request, fresh, day(0));
    await createReport(request, stale, day(30));
    await createReport(request, retired, day(0));
    for (const source of [fresh, stale, retired]) {
      await request.patch(`/api/report-sources/${source}`, { data: { cadence_days: 1 } });
    }
    await request.patch(`/api/report-sources/${retired}`, { data: { retired: true } });

    await page.goto("/daily-reports");

    // Shut by default — the report is what the page is for.
    const detail = page.getByTestId("sources-detail");
    await expect(page.getByTestId("sources-summary")).toBeVisible();
    await expect(detail).toBeHidden();

    // Both scheduled sources report from the closed bar, `<source>: <status>`.
    const freshChip = page.getByTestId(`source-chip-${fresh}`);
    const staleChip = page.getByTestId(`source-chip-${stale}`);
    await expect(freshChip).toBeVisible();
    await expect(freshChip).toContainText(`${fresh}:`);
    await expect(freshChip).toContainText("fresh");
    await expect(staleChip).toContainText("stale");

    // Colour alone carries the status — no pill border on the bar (Ken's ask),
    // and the two states must not look alike, which is the half that makes
    // dropping the border safe.
    const freshWord = freshChip.getByTestId("source-chip-freshness");
    const staleWord = staleChip.getByTestId("source-chip-freshness");
    await expect(freshWord).toHaveCSS("border-top-width", "0px");
    const freshColor = await freshWord.evaluate((el) => getComputedStyle(el).color);
    const staleColor = await staleWord.evaluate((el) => getComputedStyle(el).color);
    expect(staleColor).not.toBe(freshColor);

    // A retired source is nobody's alert, so it is off the bar — and still in
    // the panel, because "retired" is a fact about the source that the page is
    // the record of.
    await expect(page.getByTestId(`source-chip-${retired}`)).toHaveCount(0);

    await page.getByTestId("sources-summary").click();
    await expect(detail).toBeVisible();
    await expect(detail).toContainText(retired);
    await expect(detail).toContainText(fresh);
  } finally {
    // Retire what the run created. Without this every e2e run leaves a source
    // permanently shouting `stale` from the collapsed bar of whatever database
    // the suite was pointed at — which is the exact signal this panel exists to
    // make trustworthy.
    for (const source of [fresh, stale, retired]) {
      await request.patch(`/api/report-sources/${source}`, {
        data: { retired: true, note: "e2e fixture" },
      });
    }
  }
});

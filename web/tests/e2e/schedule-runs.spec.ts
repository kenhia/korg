import { test, expect } from "@playwright/test";

// WI #1105, sprint 055 — a schedule's materialisation history.
//
// `materialized_count` renders "3 runs", which cannot distinguish three runs on
// schedule from three runs in one week. For a quarterly restore drill that
// distinction *is* the value: the history is the evidence the maintenance
// happened, and `last_wi_number` explicitly cannot supply it either (see its
// doc comment). `ScheduleDetail.materialized[]` has carried it since sprint
// 051; until now nothing rendered it.

test("a schedule's run history expands from its count", async ({ page, request }) => {
  const stamp = Date.now();
  const project = `e2e-sched-runs-${stamp}`;
  const title = `e2e drill ${stamp}`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;

  // Quarterly rather than `once`: a `once` schedule goes `done` the moment it
  // fires and the list hides finished schedules by default, so the row under
  // test would vanish. Anchored in the past so it is due now.
  const sched = await (
    await request.post("/api/schedules", {
      data: {
        title,
        cadence: "quarterly",
        anchor_mode: "created",
        anchor_at: "2026-01-02T00:00:00Z",
        project_id: pid,
      },
    })
  ).json();
  const nodeId = sched.node_id as number;

  const wi = (
    await (await request.post(`/api/schedules/${nodeId}/materialize`, { data: {} })).json()
  ).work_item.wi_number as number;

  await page.goto("/schedules");
  const row = page.locator("li").filter({ hasText: title });
  await expect(row).toBeVisible();

  // Collapsed by default — the row is dense, and the history is what you go
  // looking for rather than what you scan.
  await expect(row.getByTestId("schedule-runs")).toHaveCount(0);

  const toggle = row.getByTestId("schedule-runs-toggle");
  await expect(toggle).toContainText("1 run");
  await toggle.click();

  // The three facts the count loses: which item, when it actually fired, and
  // where that item stands now.
  const runs = row.getByTestId("schedule-runs");
  await expect(runs).toContainText(`#${wi}`);
  await expect(runs).toContainText(/\d{4}-\d{2}-\d{2}/);
  await expect(runs).toContainText("open");

  // And it collapses again.
  await toggle.click();
  await expect(row.getByTestId("schedule-runs")).toHaveCount(0);
});

test("materialising refills an open run history", async ({ page, request }) => {
  const stamp = Date.now();
  const project = `e2e-sched-refill-${stamp}`;
  const title = `e2e refill drill ${stamp}`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const sched = await (
    await request.post("/api/schedules", {
      data: {
        title,
        cadence: "quarterly",
        anchor_mode: "created",
        anchor_at: "2026-01-02T00:00:00Z",
        project_id: pid,
      },
    })
  ).json();
  const nodeId = sched.node_id as number;

  const first = (
    await (await request.post(`/api/schedules/${nodeId}/materialize`, { data: {} })).json()
  ).work_item.wi_number as number;
  // Clear `outstanding` — a schedule whose last item is still open refuses to
  // materialise again, which is the point of that clause (#581).
  await request.patch(`/api/work-items/${first}`, { data: { wi_status: "done" } });

  await page.goto("/schedules");
  const row = page.locator("li").filter({ hasText: title });
  await row.getByTestId("schedule-runs-toggle").click();
  await expect(row.getByTestId("schedule-runs")).toContainText(`#${first}`);

  // The cache behind the open disclosure is dropped and refilled by
  // `materialize` — without that, the history would keep showing the run
  // *before* the one you just made, which is the single most misleading thing
  // an evidence trail could do.
  await row.getByRole("button", { name: /Materialise/ }).click();
  await expect(row.getByTestId("schedule-runs-toggle")).toContainText("2 runs");
  const runs = row.getByTestId("schedule-runs");
  await expect(runs.locator("li")).toHaveCount(2);
  await expect(runs).toContainText(`#${first}`);
});

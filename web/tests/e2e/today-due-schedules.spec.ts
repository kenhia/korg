import { test, expect } from "@playwright/test";

// WI #1385, sprint 063 — the due pill on Today.
//
// The failure this fences: schedule 1112 came due and sat there for nine days,
// because due-ness was computed only on /schedules and `list_schedules` — the
// places you go when you are *already* thinking about schedules. Today is the
// page Ken opens for an overview, and "is anything due?" is part of that
// overview the same way "is anything wrong?" is (the report-health pill beside
// it, sprint 044).
//
// The pill renders only when something is due. The suite shares one corpus
// across parallel workers and other specs file due schedules of their own, so
// what these tests hold is per-schedule membership of the pill rather than a
// global count — asserting "no pill at all" would be asserting something about
// every other test's data.

test("a due schedule shows on Today and links to where you can act on it", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const project = `e2e-due-pill-${stamp}`;
  const title = `e2e due infographic ${stamp}`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;

  // Fortnightly (#1113) anchored well over a fortnight back: due now, and the
  // cadence this sprint added, so the pill and the new vocabulary value are
  // exercised by the same row.
  await request.post("/api/schedules", {
    data: {
      title,
      cadence: "fortnightly",
      anchor_mode: "created",
      anchor_at: "2026-01-02T00:00:00Z",
      project_id: pid,
    },
  });

  await page.goto("/");
  const pill = page.getByTestId("due-schedules");
  await expect(pill).toBeVisible();
  await expect(pill).toContainText("due");

  // "Due *what*?" is answerable without a click — the title carries the
  // substituted preview titles — and the click lands on the page that can
  // materialise them.
  await expect(pill).toHaveAttribute("title", new RegExp(title));
  await pill.click();
  await expect(page).toHaveURL(/\/schedules$/);
  await expect(page.locator("li").filter({ hasText: title })).toBeVisible();
});

test("the pill is absent when its schedule is no longer due", async ({ page, request }) => {
  const stamp = Date.now();
  const project = `e2e-due-pill-gone-${stamp}`;
  const title = `e2e paused drill ${stamp}`;

  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const sched = await (
    await request.post("/api/schedules", {
      data: {
        title,
        cadence: "fortnightly",
        anchor_mode: "created",
        anchor_at: "2026-01-02T00:00:00Z",
        project_id: pid,
      },
    })
  ).json();

  await page.goto("/");
  await expect(page.getByTestId("due-schedules")).toHaveAttribute("title", new RegExp(title));

  // Pausing is the cleanest way to take a schedule out of due-ness without
  // touching its anchor, and it exercises the clause a re-derived query would
  // most likely have dropped.
  await request.patch(`/api/schedules/${sched.node_id}`, { data: { status: "paused" } });

  await page.goto("/");
  const pill = page.getByTestId("due-schedules");
  // Either the pill is gone (nothing else is due) or it no longer names this
  // schedule. Both say the same thing about the row under test.
  if (await pill.count()) {
    await expect(pill).not.toHaveAttribute("title", new RegExp(title));
  }
});

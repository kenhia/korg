import { test, expect } from "@playwright/test";

// Sprint 079 (#2151/#2152) — `soaking` on the program page.
//
// Two things under test, and they are the two halves of the same feature:
// the status control OFFERS `soaking` (a status korg grew between deploys is
// worth nothing if the browser cannot reach it), and a soaking program RENDERS
// its extended tests. For a soaking program that array is the whole content —
// every slice is terminal by the time it may enter the state — so a page that
// showed the status and not the soaks would show the label and hide the reason.
//
// The refusal is exercised from the browser too. korg's one entry rule lives in
// core, and what matters here is that a user who clicks `soaking` on a program
// that does not qualify is TOLD, rather than watching a button do nothing.

async function seed(
  request: import("@playwright/test").APIRequestContext,
  stamp: number,
  opts: { sliceStatus: string; soak: boolean },
) {
  const project = `soak-e2e-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;

  const slice = await (
    await request.post("/api/proposals", {
      data: { title: `soak slice ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();
  await request.patch(`/api/proposals/${slice.node_id}`, {
    data: { status: opts.sliceStatus },
  });

  const program = await (
    await request.post("/api/programs", {
      data: {
        title: `soak program ${stamp}`,
        aim: "the aim",
        slices: [slice.node_id],
      },
    })
  ).json();

  let soak: { wi_number: number } | null = null;
  if (opts.soak) {
    soak = await (
      await request.post("/api/work-items", {
        data: {
          title: `the extended test ${stamp}`,
          content: "three consecutive nightlies",
          project_id: pid,
          check_after: "2026-09-13",
          invalidated_if: "kai's baseline is regenerated",
        },
      })
    ).json();
    await request.post("/api/relationships", {
      data: { left: program.node_id, right: soak!.wi_number, label: "soaks" },
    });
  }
  return { program, soak, stamp };
}

test("a program with a terminal slice and a live soak can be set soaking, and shows its extended tests", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const { program, soak } = await seed(request, stamp, {
    sliceStatus: "done",
    soak: true,
  });

  await page.goto(`/programs/${program.node_id}`);
  const control = page.getByTestId("program-status-control");

  // The vocabulary reached the browser at all — this is the half a generated
  // `vocab.ts` that had not been regenerated would fail.
  await expect(control.getByRole("button", { name: "soaking", exact: true })).toBeVisible();

  await control.getByRole("button", { name: "soaking", exact: true }).click();
  await expect(control.getByRole("button", { name: "soaking", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(
    (await (await request.get(`/api/programs/${program.node_id}`)).json()).status,
  ).toBe("soaking");

  // The soaks array renders, with the two fields that make it actionable: when
  // it may be judged, and what would void it.
  const soaks = page.getByTestId("program-soaks");
  await expect(soaks).toBeVisible();
  const row = page.getByTestId(`soak-${soak!.wi_number}`);
  await expect(row).toContainText(`the extended test ${stamp}`);
  await expect(row).toContainText("judge from 2026-09-13");
  await expect(row).toContainText("kai's baseline is regenerated");
});

test("a program that does not qualify is refused, visibly, and keeps its status", async ({
  page,
  request,
}) => {
  const stamp = Date.now() + 1;
  // A slice still in the queue, so the program has work outstanding that is not
  // a clock.
  const { program } = await seed(request, stamp, {
    sliceStatus: "proposed",
    soak: true,
  });

  await page.goto(`/programs/${program.node_id}`);
  const control = page.getByTestId("program-status-control");
  await control.getByRole("button", { name: "soaking", exact: true }).click();

  // korg refused; the page must say so rather than silently doing nothing.
  await expect(page.getByRole("alert")).toContainText(/terminal/i);
  expect(
    (await (await request.get(`/api/programs/${program.node_id}`)).json()).status,
  ).toBe("queued");
});

test("a program with no soaks renders no extended-tests section", async ({ page, request }) => {
  const stamp = Date.now() + 2;
  const { program } = await seed(request, stamp, { sliceStatus: "done", soak: false });
  await page.goto(`/programs/${program.node_id}`);
  await expect(page.getByTestId("program-status-control")).toBeVisible();
  await expect(page.getByTestId("program-soaks")).toHaveCount(0);
});

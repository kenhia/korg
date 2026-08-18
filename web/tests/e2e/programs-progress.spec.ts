import { test, expect } from "@playwright/test";

// Sprint 049 (#980, #982) — Ken's 2026-08-05 user test of /programs, as a spec.
//
// The state that produced both findings is the one seeded below: a program
// whose work is entirely implemented and none of which Ken has closed. The old
// two-part count rendered that as `0/3`, which he read as "still has
// implementation to do"; the whole point of the three-part figure is that this
// exact state is unmistakable.

/** Seed a program with one slice covering three work items in the given states. */
async function seedProgram(
  request: import("@playwright/test").APIRequestContext,
  stamp: number,
  statuses: string[],
) {
  const project = `prog-e2e-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;

  const numbers: number[] = [];
  for (const [i, wi_status] of statuses.entries()) {
    const wi = await (
      await request.post("/api/work-items", {
        data: { title: `prog item ${i} ${stamp}`, content: "x", project_id: pid },
      })
    ).json();
    // `closed` is Ken's, and create does not take it — patch after the fact.
    if (wi_status !== "open") {
      await request.patch(`/api/work-items/${wi.wi_number}`, { data: { wi_status } });
    }
    numbers.push(wi.wi_number as number);
  }

  const slice = await (
    await request.post("/api/proposals", {
      data: {
        title: `slice ${stamp}`,
        summary: "the slice",
        project_id: pid,
        work_item_numbers: numbers,
      },
    })
  ).json();

  const program = await (
    await request.post("/api/programs", {
      data: {
        title: `program ${stamp}`,
        aim: "the aim",
        slices: [slice.node_id],
      },
    })
  ).json();

  return { program, slice, project, numbers };
}

test("slice progress separates work-complete from Ken-verified", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  // Everything implemented, nothing closed — the misread state.
  const { program, slice } = await seedProgram(request, stamp, [
    "resolved",
    "done",
    "resolved",
  ]);

  await page.goto(`/programs/${program.node_id}`);

  // #980 half one: the ids agents cite are on the page.
  await expect(page.getByTestId("program-id")).toHaveText(`#${program.node_id}`);
  const row = page.getByTestId(`slice-${slice.node_id}`);
  await expect(row.getByText(`#${slice.node_id}`)).toBeVisible();

  // #980 half two: 3 work-complete / 0 verified / 3 total. The old display
  // showed `0/3 done` here, which is what "still has implementation to do"
  // was read off.
  const progress = row.getByTestId("slice-progress");
  await expect(progress).toContainText("3");
  await expect(progress).toContainText("0");
  await expect(progress).toHaveText(/3\s*\/\s*0\s*\/\s*3/);
});

test("closing an item moves it from work-complete to verified", async ({
  page,
  request,
}) => {
  const stamp = Date.now() + 1;
  const { program, slice, numbers } = await seedProgram(request, stamp, [
    "resolved",
    "done",
    "open",
  ]);

  await page.goto(`/programs/${program.node_id}`);
  const progress = page
    .getByTestId(`slice-${slice.node_id}`)
    .getByTestId("slice-progress");
  // Two implemented, none verified, one still open.
  await expect(progress).toHaveText(/2\s*\/\s*0\s*\/\s*3/);

  // Ken closes one — `closed` counts in BOTH the left figure and the middle
  // one, because a closed item is work that is complete *and* verified.
  await request.patch(`/api/work-items/${numbers[0]}`, {
    data: { wi_status: "closed" },
  });
  await page.reload();
  await expect(
    page.getByTestId(`slice-${slice.node_id}`).getByTestId("slice-progress"),
  ).toHaveText(/2\s*\/\s*1\s*\/\s*3/);
});

test("a program's status is settable from its own page", async ({ page, request }) => {
  const stamp = Date.now() + 2;
  const { program, slice } = await seedProgram(request, stamp, ["resolved"]);
  // #1168 put a confirmation in front of `done` when a slice is unfinished, and
  // a proposal still sitting at `proposed` is unfinished however complete its
  // work items are. This test is about the control writing at all, so its slice
  // is closed out first; the gate itself has its own spec
  // (program-close-out.spec.ts), including the case that reaches this dialog.
  await request.patch(`/api/proposals/${slice.node_id}`, { data: { status: "done" } });

  await page.goto(`/programs/${program.node_id}`);
  const control = page.getByTestId("program-status-control");
  // The current status is the pressed one, and is not clickable.
  await expect(control.getByRole("button", { name: "active" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  // #980's third finding: this used to be a read-only pill, so marking 979 done
  // needed a REST call on Ken's behalf.
  await control.getByRole("button", { name: "done", exact: true }).click();
  await expect(control.getByRole("button", { name: "done", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  // It really wrote — the server agrees, not just the button.
  const after = await (await request.get(`/api/programs/${program.node_id}`)).json();
  expect(after.status).toBe("done");
});

test("find-by-ID resolves a program to its real title and page", async ({
  page,
  request,
}) => {
  const stamp = Date.now() + 3;
  const { program } = await seedProgram(request, stamp, ["open"]);

  // #982: entering a program's id used to render the generic fallback — chip
  // PROGRAM, title literally "program #979".
  await page.goto("/work-items");
  await page.getByLabel("Find a work item or node by id").fill(String(program.node_id));
  await page.getByRole("button", { name: "Go" }).click();

  const panel = page.getByTestId("node-preview-panel");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(`program ${stamp}`);
  await expect(panel).not.toContainText(`program #${program.node_id}`);

  // And it offers the node's own page, which used to be handoff-only.
  await panel.getByTestId("open-node-page").click();
  await expect(page).toHaveURL(new RegExp(`/programs/${program.node_id}$`));
});

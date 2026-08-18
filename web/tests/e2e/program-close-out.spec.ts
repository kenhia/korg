import { test, expect } from "@playwright/test";

// WI #1168 — closing a program from the browser, with the gate Ken asked for:
// "enabled all the time with a 'not all the slices are complete, you sure you
// want to close this' confirmation".
//
// The bare status buttons shipped in 049. What is under test here is the
// difference between "you can click done" and "clicking done asks first, and
// says which slices are not finished".

async function seedProgram(
  request: import("@playwright/test").APIRequestContext,
  stamp: number,
  slices: { status?: string; wi_statuses: string[] }[],
) {
  const project = `close-e2e-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;

  const sliceIds: number[] = [];
  const sliceTitles: string[] = [];
  for (const [i, spec] of slices.entries()) {
    const numbers: number[] = [];
    for (const [j, wi_status] of spec.wi_statuses.entries()) {
      const wi = await (
        await request.post("/api/work-items", {
          data: { title: `close item ${i}-${j} ${stamp}`, content: "x", project_id: pid },
        })
      ).json();
      if (wi_status !== "open") {
        await request.patch(`/api/work-items/${wi.wi_number}`, { data: { wi_status } });
      }
      numbers.push(wi.wi_number as number);
    }
    const title = `slice ${i} ${stamp}`;
    const slice = await (
      await request.post("/api/proposals", {
        data: { title, summary: "s", project_id: pid, work_item_numbers: numbers },
      })
    ).json();
    if (spec.status) {
      await request.patch(`/api/proposals/${slice.node_id}`, {
        data: { status: spec.status },
      });
    }
    sliceIds.push(slice.node_id as number);
    sliceTitles.push(title);
  }

  const program = await (
    await request.post("/api/programs", {
      data: { title: `close program ${stamp}`, aim: "the aim", slices: sliceIds },
    })
  ).json();
  return { program, sliceIds, sliceTitles };
}

test("closing a program with unfinished slices asks first, and names them", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const { program, sliceIds, sliceTitles } = await seedProgram(request, stamp, [
    // Finished both ways — must not appear in the dialog.
    { status: "done", wi_statuses: ["closed"] },
    // Closed out as a proposal, but its work is not: one item still open.
    { status: "done", wi_statuses: ["resolved", "open"] },
    // Work all implemented, but the proposal never left the queue.
    { status: "proposed", wi_statuses: ["resolved"] },
  ]);

  await page.goto(`/programs/${program.node_id}`);
  const control = page.getByTestId("program-status-control");
  await control.getByRole("button", { name: "done", exact: true }).click();

  const dialog = page.getByTestId("program-close-confirm");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("2 slices are not finished");
  // Each unfinished slice is named, with the reason in its own vocabulary:
  // a proposal is unfinished by *status*, its work items by count.
  await expect(dialog).toContainText(sliceTitles[1]);
  await expect(dialog).toContainText("1 of 2 work items unfinished");
  await expect(dialog).toContainText(sliceTitles[2]);
  await expect(dialog).toContainText("still proposed");
  // The finished slice is not listed — a confirm that lists everything is a
  // confirm nobody reads.
  await expect(dialog).not.toContainText(sliceTitles[0]);
  await expect(dialog).not.toContainText(`#${sliceIds[0]}`);

  // Cancelling leaves the program exactly as it was, server included.
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(control.getByRole("button", { name: "active" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(
    (await (await request.get(`/api/programs/${program.node_id}`)).json()).status,
  ).toBe("active");

  // Confirming writes. Ken's own reasoning for enable-always: the program that
  // is really over but carries a slice nobody will pick up must still close.
  await control.getByRole("button", { name: "done", exact: true }).click();
  await page.getByTestId("program-close-anyway").click();
  await expect(control.getByRole("button", { name: "done", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(
    (await (await request.get(`/api/programs/${program.node_id}`)).json()).status,
  ).toBe("done");
});

test("a program whose slices are all finished closes without a prompt", async ({
  page,
  request,
}) => {
  const stamp = Date.now() + 1;
  const { program } = await seedProgram(request, stamp, [
    { status: "done", wi_statuses: ["closed", "done"] },
    // Declined counts as closed out: nothing is waiting on it.
    { status: "declined", wi_statuses: ["resolved"] },
  ]);

  await page.goto(`/programs/${program.node_id}`);
  const control = page.getByTestId("program-status-control");
  await control.getByRole("button", { name: "done", exact: true }).click();

  await expect(page.getByTestId("program-close-confirm")).toHaveCount(0);
  await expect(control.getByRole("button", { name: "done", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(
    (await (await request.get(`/api/programs/${program.node_id}`)).json()).status,
  ).toBe("done");
});

// The other two transitions are not close-outs and must stay one click — a
// confirm on a status you are *opening* is the trained-to-click-through failure
// ConfirmButton's scope note warns about.
test("holding and active are not gated", async ({ page, request }) => {
  const stamp = Date.now() + 2;
  const { program } = await seedProgram(request, stamp, [
    { status: "proposed", wi_statuses: ["open"] },
  ]);

  await page.goto(`/programs/${program.node_id}`);
  const control = page.getByTestId("program-status-control");
  await control.getByRole("button", { name: "holding", exact: true }).click();
  await expect(page.getByTestId("program-close-confirm")).toHaveCount(0);
  await expect(control.getByRole("button", { name: "holding", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

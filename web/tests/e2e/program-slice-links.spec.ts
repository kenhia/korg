import { test, expect } from "@playwright/test";

// WI #2444 — "when I'm reading a program I should be able to navigate to the
// individual slices". The slice list carried the id an agent cites and the
// title, both as plain text, on a page whose entire job is routing to slices.
//
// The link is on the **title**, not the id, and that is korg's convention
// rather than this sprint's preference: the programs list, this page's own
// soaks and Related lists, and the proposal page's covered items all link the
// title. Reading list and Schedules put the affordance on the id precisely
// because their titles are spoken for by another navigation. Asserted here so
// a later change cannot quietly flip it.

async function seedProgram(request: import("@playwright/test").APIRequestContext, stamp: number) {
  const project = `slice-link-${stamp}`;
  const pid = (await (await request.post("/api/projects", { data: { name: project } })).json())
    .id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `slice item ${stamp}`, content: "x", project_id: pid },
    })
  ).json();
  const slice = await (
    await request.post("/api/proposals", {
      data: {
        title: `linkable slice ${stamp}`,
        summary: "the slice",
        project_id: pid,
        work_item_numbers: [wi.wi_number],
      },
    })
  ).json();
  const program = await (
    await request.post("/api/programs", {
      data: { title: `slice link program ${stamp}`, aim: "the aim", slices: [slice.node_id] },
    })
  ).json();
  return { program, slice };
}

test("a slice's title navigates to its proposal", async ({ page, request }) => {
  const stamp = Date.now();
  const { program, slice } = await seedProgram(request, stamp);

  await page.goto(`/programs/${program.node_id}`);
  const row = page.getByTestId(`slice-${slice.node_id}`);
  await expect(row).toBeVisible();

  // The title is a link…
  const link = row.getByRole("link", { name: `linkable slice ${stamp}` });
  await expect(link).toHaveAttribute("href", `/planning/${slice.node_id}`);

  // …and the id beside it is not a control. `#id` is a handle you read and
  // retype, which is the whole reason it is on screen (#980).
  await expect(row.getByRole("link", { name: `#${slice.node_id}` })).toHaveCount(0);
  await expect(row.getByRole("button", { name: `#${slice.node_id}` })).toHaveCount(0);

  await link.click();
  await expect(page).toHaveURL(new RegExp(`/planning/${slice.node_id}$`));
  await expect(page.getByTestId("proposal-detail-id")).toHaveText(`#${slice.node_id}`);
});

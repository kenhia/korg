import { test, expect, type Locator } from "@playwright/test";

// WI #2710 — "there is not enough difference between the different states for
// me to at-a-glance see the current state of a program (the one in the
// screenshot is `done`)".
//
// The status control gave "this IS the state" and "these are the states you may
// set" the same visual weight: six same-sized chips, one of them current. For
// most statuses the hue carried it anyway. For the hueless ones it did not, and
// `done` is hueless **on purpose** — `domain.ts` argues at length that `done`
// must not compete with `active`, that `queued` must not read louder than
// `active`, and that `soaking` must not read as either. Those hues are right.
//
// So the affordance that says *current* has to be independent of hue, and that
// is what these tests pin. Colour is asserted nowhere here: a treatment that
// only worked for the coloured statuses would pass a colour test and still be
// the bug Ken filed.
//
// Both controls are covered because they are the same control. The proposal
// page's `proposed` and `declined` are as hueless as a program's `done`, and
// fixing one surface would leave korg with two answers to one question — which
// is #1603's lesson about a second copy of a status idea.

/** The one button a status control marks as current, and one that it doesn't. */
async function currentAndOther(control: Locator, otherName: string) {
  const current = control.locator('[data-current="true"]');
  await expect(current).toHaveCount(1);
  const other = control.getByRole("button", { name: otherName, exact: true });
  await expect(other).toHaveAttribute("data-current", "false");
  return { current, other };
}

/** A ring the hueless statuses can rely on: present on the current option,
 *  absent from the settable ones. */
async function assertRing(current: Locator, other: Locator) {
  const ring = await current.evaluate((el) => getComputedStyle(el).boxShadow);
  const none = await other.evaluate((el) => getComputedStyle(el).boxShadow);
  expect(ring).not.toBe("none");
  expect(none).toBe("none");
}

/** …and weight, so the current option reads as a statement in a row of
 *  options even in a screenshot with no hover and no focus. */
async function assertWeight(current: Locator, other: Locator) {
  const bold = Number(await current.evaluate((el) => getComputedStyle(el).fontWeight));
  const plain = Number(await other.evaluate((el) => getComputedStyle(el).fontWeight));
  expect(bold).toBeGreaterThan(plain);
}

test("a done program's status is legible in a row of six options", async ({ page, request }) => {
  const stamp = Date.now();
  const project = `status-legible-${stamp}`;
  const pid = (await (await request.post("/api/projects", { data: { name: project } })).json())
    .id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `status item ${stamp}`, content: "x", project_id: pid },
    })
  ).json();
  const slice = await (
    await request.post("/api/proposals", {
      data: {
        title: `status slice ${stamp}`,
        summary: "s",
        project_id: pid,
        work_item_numbers: [wi.wi_number],
      },
    })
  ).json();
  const program = await (
    await request.post("/api/programs", {
      data: { title: `status program ${stamp}`, aim: "the aim", slices: [slice.node_id] },
    })
  ).json();
  // `done` is Ken's screenshotted case and the hardest one: no hue to lean on.
  await request.patch(`/api/programs/${program.node_id}`, { data: { status: "done" } });

  await page.goto(`/programs/${program.node_id}`);
  const control = page.getByTestId("program-status-control");
  await expect(control).toBeVisible();

  const { current, other } = await currentAndOther(control, "active");
  await expect(current).toHaveText(/done/);
  await assertRing(current, other);
  await assertWeight(current, other);
});

test("a proposal's status is legible the same way", async ({ page, request }) => {
  const stamp = Date.now();
  const project = `status-prop-${stamp}`;
  const pid = (await (await request.post("/api/projects", { data: { name: project } })).json())
    .id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `prop status item ${stamp}`, content: "x", project_id: pid },
    })
  ).json();
  const proposal = await (
    await request.post("/api/proposals", {
      data: {
        title: `prop status ${stamp}`,
        summary: "s",
        project_id: pid,
        work_item_numbers: [wi.wi_number],
      },
    })
  ).json();

  await page.goto(`/planning/${proposal.node_id}`);
  const control = page.getByTestId("proposal-status-control");
  await expect(control).toBeVisible();

  // A fresh proposal is `proposed`, which shares `declined`'s neutral ground
  // and differs from it only in ink — the proposal-side twin of `done`.
  const { current, other } = await currentAndOther(control, "active");
  await expect(current).toHaveText(/proposed/);
  await assertRing(current, other);
  await assertWeight(current, other);
});

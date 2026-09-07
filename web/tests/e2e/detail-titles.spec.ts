import { test, expect, type APIRequestContext } from "@playwright/test";

// Sprint 078 (#1969) — the tab names the node you are *looking at*.
//
// Sprint 077 put the id in the tab on every detail **route**, and that was the
// wrong half: the routes are not how a node usually gets opened. Clicking a
// Work Items row, a proposal's title on Planning, or a card on the board opens
// an in-page surface and changes no URL, so a route-bound title never moved.
//
// The rule these lock down: **the topmost open surface owns the title.** It is
// worth a test rather than a comment because it is enforced by two *different*
// mechanisms that have to agree — in-page surfaces publish a `<svelte:head>`,
// while the slide-over assigns `document.title` imperatively. That asymmetry is
// not stylistic: `document.title` reads the FIRST `<title>` in the document, so
// a second `<svelte:head>` mounted by the overlay would lose to the page
// underneath, silently. The stacking test at the bottom is the one that would
// catch a well-meaning refactor to `<svelte:head>` everywhere.

/** A tag no other worker can produce.
 *
 *  `Date.now()` alone is not enough: the suite is `fullyParallel`, so two tests
 *  entering within the same millisecond seed rows with identical titles and the
 *  row lookup picks the wrong one. `destructive-confirm.spec.ts` carries the
 *  same helper for the same reason. */
const uniq = () => `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;

async function seed(request: APIRequestContext, stamp: string) {
  const project = `e2e-titles-${stamp}`;
  const pid = (
    await (await request.post("/api/projects", { data: { name: project } })).json()
  ).id as number;
  const wi = await (
    await request.post("/api/work-items", {
      data: { title: `titled row ${stamp}`, content: "opened from the list", project_id: pid },
    })
  ).json();
  const proposal = await (
    await request.post("/api/proposals", {
      data: {
        title: `titled bundle ${stamp}`,
        summary: "s",
        project_id: pid,
        work_item_numbers: [wi.wi_number],
      },
    })
  ).json();
  return { project, wi, proposal };
}

test("the Work Items detail panel names its item, and closing gives the tab back", async ({
  page,
  request,
}) => {
  const stamp = uniq();
  const { wi } = await seed(request, stamp);

  await page.goto("/work-items");
  await expect(page).toHaveTitle("korg");

  // "All projects", because the list is sticky on the last project picked and
  // the seeded item's project is not it.
  await page.getByRole("button", { name: "All projects" }).click();
  await page.getByRole("row", { name: new RegExp(`titled row ${stamp}`) }).first().click();

  // The panel carries no testid of its own; its "← Back" control exists only
  // while it is open, which is the same assertion by another name.
  await expect(page.getByRole("button", { name: "← Back" })).toBeVisible();
  await expect(page).toHaveTitle(`korg — WI ${wi.wi_number}`);

  // The URL never moved — which is the whole reason 077's title did not either.
  await expect(page).toHaveURL(/\/work-items$/);

  await page.keyboard.press("Escape");
  await expect(page).toHaveTitle("korg");
});

test("the Planning slide-over names the proposal it is showing", async ({
  page,
  request,
}) => {
  const stamp = uniq();
  const { proposal } = await seed(request, stamp);

  await page.goto("/planning");
  await expect(page).toHaveTitle("korg");

  // The proposal's TITLE opens the quick look; the 🔍 beside it navigates to
  // the full page instead. Both should name the proposal, by different routes
  // through the code — this is the one that used to say plain "korg".
  await page
    .getByTestId("proposal-open-detail")
    .filter({ hasText: `titled bundle ${stamp}` })
    .first()
    .click();

  await expect(page).toHaveTitle(`korg — proposal ${proposal.node_id}`);
  await expect(page).toHaveURL(/\/planning$/);

  await page.keyboard.press("Escape");
  await expect(page).toHaveTitle("korg");
});

test("the card editor names the card", async ({ page, request }) => {
  const stamp = uniq();
  const card = await (
    await request.post("/api/cards", { data: { title: `titled card ${stamp}` } })
  ).json();

  await page.goto("/cards");
  await expect(page).toHaveTitle("korg");

  await page.getByText(`titled card ${stamp}`).first().click();
  await expect(page.getByTestId("card-modal")).toBeVisible();
  await expect(page).toHaveTitle(`korg — card ${card.node_id}`);

  await page.keyboard.press("Escape");
  await expect(page).toHaveTitle("korg");
});

test("a preview opened over a titled page wins, and hands the title back", async ({
  page,
  request,
}) => {
  // The precedence rule, on the page where both mechanisms are live at once:
  // the proposal route publishes a `<svelte:head>` title, and the slide-over
  // opened from its covered-items list assigns `document.title` over the top.
  //
  // A `<svelte:head>` in the slide-over would fail exactly here and nowhere
  // else — the page's own title element is first in the document and would keep
  // winning, so the preview would look right everywhere it is opened from an
  // untitled list page and be wrong only here.
  const stamp = uniq();
  const { wi, proposal } = await seed(request, stamp);

  await page.goto(`/planning/${proposal.node_id}`);
  await expect(page).toHaveTitle(`korg — proposal ${proposal.node_id}`);

  await page.getByTestId("proposal-covered").getByRole("button").first().click();
  await expect(page).toHaveTitle(`korg — WI ${wi.wi_number}`);

  await page.keyboard.press("Escape");
  await expect(page).toHaveTitle(`korg — proposal ${proposal.node_id}`);
});

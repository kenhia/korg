import { test, expect } from "@playwright/test";

// WI #1629 — a starred project rides a band at the top of both rails.
//
// The two things worth pinning are the ones a reasonable implementation gets
// wrong: the starred project must appear TWICE (the band is a shortcut, not a
// move — Ken's mock keeps it in its category too), and the band must be shared
// state rather than a per-browser preference, which is why Planning shows it
// without having any control to set it (D-1, D-2).

/** True when `project` appears before the first category header in a rail's
 *  rendered text, i.e. the starred band is above the categories. Asserts the
 *  header was actually found, so a missed match cannot pass as an ordering. */
function railBands(railText: string, project: string): boolean {
  const text = railText.toLowerCase();
  const header = text.indexOf("uncategorised");
  expect(header, "the Uncategorised header should be in the rail").toBeGreaterThan(-1);
  const first = text.indexOf(project.toLowerCase());
  expect(first, "the project should be in the rail").toBeGreaterThan(-1);
  return first < header;
}

async function newProject(page: import("@playwright/test").Page, name: string) {
  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(name);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name, exact: true }).first()).toBeVisible();
}

test("starring a project bands it at the top of both rails", async ({ page }) => {
  const project = `e2e-star-${Date.now()}`;
  await newProject(page, project);

  const rail = page.getByTestId("project-rail");
  const rows = rail.getByRole("button", { name: project, exact: true });

  // A new project has no category, so it starts in "Uncategorised" and nowhere
  // else. One row, no band.
  await rail.getByRole("button", { name: project, exact: true }).first().click();
  await expect(rows).toHaveCount(1);

  // The control lives on the Project Details bar and is readable while the
  // section is shut — that is the whole of D-2.
  const star = page.getByTestId("project-star");
  await expect(star).toHaveAttribute("aria-pressed", "false");
  await expect(star).toHaveAccessibleName(`Star ${project}`);

  // #1666 — the two states must differ by COLOUR, not only by ★ vs ☆ at 14px.
  // Off is pinned to the grey the "Project Details" label already carries, so
  // this reads that element rather than hard-coding a token a palette change
  // would silently rot.
  //
  // Park the pointer before every read. The star sits INSIDE the summary, the
  // summary carries `hover:text-[var(--color-accent)]`, and before the fix the
  // star had no colour of its own — so a hovered bar recoloured it by
  // inheritance. The first draft of this assertion passed against the exact
  // code it was written to fail, for precisely that reason: it read the
  // on-state with the mouse still resting on the star it had just clicked.
  const detailsBar = page.locator(
    'summary:has([data-testid="project-details-name"])',
  );
  await page.mouse.move(0, 0);
  const grey = await detailsBar.evaluate((el) => getComputedStyle(el).color);
  const offColour = await star.evaluate((el) => getComputedStyle(el).color);
  expect(offColour).toBe(grey);

  await star.click();

  await expect(star).toHaveAttribute("aria-pressed", "true");
  await expect(star).toHaveAccessibleName(`Unstar ${project}`);
  await page.mouse.move(0, 0);
  const onColour = await star.evaluate((el) => getComputedStyle(el).color);
  expect(onColour).not.toBe(offColour);

  // Twice: once in the band, once in its normal category position. The
  // duplication is the feature — the rail must not reshuffle under the click.
  await expect(rows).toHaveCount(2);

  // And the band is ABOVE the categories. innerText is DOM order, so the first
  // mention preceding the "Uncategorised" header is the band.
  //
  // Lower-cased first: the category headers carry Tailwind's `uppercase`, and
  // innerText reports text as CSS renders it, so a literal "Uncategorised"
  // never matches and `indexOf` quietly returns -1 — which reads as "the band
  // is above it" and would pass this assertion for the wrong reason.
  expect(railBands(await rail.innerText(), project)).toBe(true);

  // Planning reflects it — which is only possible because the star is a column
  // and not this browser's localStorage — and offers no way to change it.
  await page.goto("/planning");
  const planningRail = page.getByTestId("planning-project-rail");
  // Not `exact`: Planning's rows carry their proposal/work-item counts inside
  // the button, so the accessible name is the project name plus those numbers.
  // The name is timestamped, so a substring match is still unambiguous.
  await expect(planningRail.getByRole("button", { name: project })).toHaveCount(2);
  expect(railBands(await planningRail.innerText(), project)).toBe(true);
  await expect(page.getByTestId("project-star")).toHaveCount(0);

  // The band carries the rule Ken's mock draws between "All Projects" and it —
  // the category headers bring their own, so a headerless band would otherwise
  // run straight on from the All-projects row. Exactly one, however many
  // projects are starred: it belongs to the band, not to the rows.
  await expect(page.getByTestId("starred-band-rule")).toHaveCount(1);

  // Unstarring puts the rail back exactly as it was: the band renders only
  // when something is starred.
  await page.goto("/work-items");
  await rail.getByRole("button", { name: project, exact: true }).first().click();
  await page.getByTestId("project-star").click();
  await expect(rows).toHaveCount(1);
  await expect(rail.getByText("Uncategorised")).toBeVisible();
  // Deliberately NOT asserting the band is gone. These suites run against a
  // shared database and another project may legitimately be starred in it, so
  // the band's absence is not this test's to claim — only that *this* project
  // left it, which the count above says exactly.
});

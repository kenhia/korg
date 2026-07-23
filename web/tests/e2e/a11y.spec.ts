import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// Automated accessibility floor across every route (WI #548).
//
// A floor, not a goal, and the sprint that added this file is itself the
// evidence: the bug that started WI #571 was tag chips that "fade into the
// background", and axe passes them — the *text* contrast measured 5.23:1, well
// past AA. What was wrong was that the chip's background matched its container
// (1.00:1 on a kanban tile), which no automated checker asserts. So this suite
// catches the mechanical failures — unlabelled controls, missing roles, real
// text-contrast misses — and human judgement still has to cover the rest.
//
// Scoped to `serious` and `critical`. `moderate` and `minor` are worth reading
// but not worth failing a build over while the baseline is being established.

const ROUTES = [
  "/",
  "/history",
  "/topics",
  "/plan",
  "/cards",
  "/work-items",
  "/planning",
  "/daily-reports",
  "/reading-list",
  "/link-up",
];

for (const route of ROUTES) {
  test(`no serious axe violations on ${route}`, async ({ page }) => {
    await page.goto(route);
    // The pages render their lists after a fetch; without this the scan can run
    // against an empty shell and pass for the wrong reason.
    await page.waitForLoadState("networkidle");

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      // `color-contrast` is disabled because axe-core mis-reads `oklch()`, and
      // korg's entire palette is oklch (`app.css`).
      //
      // This is not a shrug — it was measured. axe reported `--color-muted` as
      // #6e7076 giving 3.61:1 on `--color-surface`. The colour Chromium
      // actually paints for `oklch(0.68 0.01 270)` is #96989f, sampled from a
      // canvas round-trip, which is 6.21:1 on that same surface. Every
      // `color-contrast` violation it raised against this theme was a false
      // positive produced by the wrong foreground.
      //
      // Contrast is still checked — see `theme-contrast.spec.ts`, which reads
      // the painted pixels rather than parsing the CSS, so it cannot make this
      // mistake. Re-enable this rule if axe-core gains real oklch support.
      .disableRules(["color-contrast"])
      .analyze();

    const serious = results.violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    );

    // Name the offending selectors in the failure — "3 violations" sends you
    // back to the browser, a list of nodes sends you to the line.
    const detail = serious
      .map(
        (v) =>
          `${v.impact} ${v.id}: ${v.help}\n    ${v.nodes
            .map((n) => n.target.join(" "))
            .slice(0, 5)
            .join("\n    ")}`,
      )
      .join("\n  ");

    expect(serious, `axe violations on ${route}:\n  ${detail}`).toEqual([]);
  });
}

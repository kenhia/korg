import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";

// Contrast, measured from the pixels the browser actually paints (WI #571).
//
// This exists because the two obvious ways to check contrast both failed here:
//
//   * axe-core's `color-contrast` rule mis-parses `oklch()`, and korg's whole
//     palette is oklch. It reported `--color-muted` at 3.61:1 when the painted
//     colour is 6.21:1 — a false positive on every muted element in the app.
//   * Reading the CSS and converting by hand is what produced that error in the
//     first place, one layer up.
//
// So this asks the browser to resolve each colour to sRGB via a canvas
// round-trip and computes WCAG contrast from that. Whatever the CSS says, this
// is what a person sees.
//
// It also covers the failure axe *cannot* catch, and which started WI #571:
// a chip whose background matches its container. The tag chip's label passed
// AA at 5.23:1 while being invisible, because the chip had no edge — text
// contrast was never the problem.

type RGB = [number, number, number];

/** Resolve any CSS colour to sRGB by painting it, so oklch/color-mix/alpha all
 *  come back as the browser renders them. */
async function resolve(page: Page, colors: string[]) {
  return page.evaluate((list: string[]) => {
    const root = getComputedStyle(document.documentElement);
    const c = document.createElement("canvas");
    c.width = c.height = 1;
    const ctx = c.getContext("2d")!;
    return list.map((col) => {
      // Canvas `fillStyle` does not understand `var()` — it silently ignores
      // an unparseable value and keeps the previous one, which reads back as
      // whatever was painted last rather than as an error. Expand the custom
      // property here; `oklch()` itself canvas handles fine.
      const m = /^var\((--[\w-]+)\)$/.exec(col.trim());
      const value = m ? root.getPropertyValue(m[1]).trim() : col;
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = "#000";
      ctx.fillRect(0, 0, 1, 1);
      const before = ctx.fillStyle;
      ctx.fillStyle = value;
      if (ctx.fillStyle === before && value !== "#000000") {
        throw new Error(`could not resolve colour: ${col} -> ${value}`);
      }
      ctx.fillRect(0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      return [d[0], d[1], d[2]] as [number, number, number];
    });
  }, colors);
}

function luminance([r, g, b]: RGB): number {
  const lin = [r, g, b].map((v) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

function ratio(a: RGB, b: RGB): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const VAR = (n: string) => `var(--color-${n})`;

test("body and muted text clear AA against every surface", async ({ page }) => {
  await page.goto("/");
  const [text, muted, bg, surface, surfaceHi] = await resolve(page, [
    VAR("text"),
    VAR("muted"),
    VAR("bg"),
    VAR("surface"),
    VAR("surface-hi"),
  ]);

  for (const [name, fg] of [
    ["text", text],
    ["muted", muted],
  ] as const) {
    for (const [sname, s] of [
      ["bg", bg],
      ["surface", surface],
      ["surface-hi", surfaceHi],
    ] as const) {
      const r = ratio(fg as RGB, s as RGB);
      expect(
        r,
        `--color-${name} on --color-${sname} is ${r.toFixed(2)}:1, needs 4.5:1`,
      ).toBeGreaterThanOrEqual(4.5);
    }
  }
});

test("chips are legible AND separable from the surface behind them", async ({
  page,
}) => {
  await page.goto("/work-items");
  await page.waitForLoadState("networkidle");

  // Paint each chip the way domain.ts does, and read back what lands.
  const chips = await page.evaluate(() => {
    const specs = [
      { name: "project", cls: "bg-teal-900/60 text-teal-300" },
      { name: "category", cls: "bg-violet-900/70 text-violet-200" },
      { name: "tag", cls: "bg-amber-900/70 text-amber-200" },
    ];
    const out: { name: string; bgOn: Record<string, string>; fg: string }[] = [];
    for (const spec of specs) {
      const bgOn: Record<string, string> = {};
      for (const surface of ["bg", "surface", "surface-hi"]) {
        const host = document.createElement("div");
        host.style.background = `var(--color-${surface})`;
        const el = document.createElement("span");
        el.className = spec.cls;
        el.textContent = "x";
        host.appendChild(el);
        document.body.appendChild(host);
        bgOn[surface] = getComputedStyle(el).backgroundColor;
        if (!out.find((o) => o.name === spec.name)) {
          out.push({ name: spec.name, bgOn: {}, fg: getComputedStyle(el).color });
        }
        host.remove();
      }
      out.find((o) => o.name === spec.name)!.bgOn = bgOn;
    }
    return out;
  });

  const surfaces = await resolve(page, [VAR("bg"), VAR("surface"), VAR("surface-hi")]);
  const surfaceByName: Record<string, RGB> = {
    bg: surfaces[0] as RGB,
    surface: surfaces[1] as RGB,
    "surface-hi": surfaces[2] as RGB,
  };

  for (const chip of chips) {
    const fgs = await resolve(page, [chip.fg]);
    for (const [sname, cssBg] of Object.entries(chip.bgOn)) {
      // The chip's background is semi-transparent, so composite it the way the
      // page does before measuring.
      const composited = await page.evaluate(
        ([bgCss, surfaceVar]) => {
          const c = document.createElement("canvas");
          c.width = c.height = 1;
          const ctx = c.getContext("2d")!;
          ctx.fillStyle = getComputedStyle(document.documentElement)
            .getPropertyValue(surfaceVar)
            .trim();
          ctx.fillRect(0, 0, 1, 1);
          ctx.fillStyle = bgCss;
          ctx.fillRect(0, 0, 1, 1);
          const d = ctx.getImageData(0, 0, 1, 1).data;
          return [d[0], d[1], d[2]] as [number, number, number];
        },
        [cssBg, `--color-${sname}`] as [string, string],
      );

      const label = ratio(fgs[0] as RGB, composited as RGB);
      expect(
        label,
        `${chip.name} chip label on ${sname} is ${label.toFixed(2)}:1, needs 4.5:1`,
      ).toBeGreaterThanOrEqual(4.5);

      // The bug axe cannot see. `chip.tag` used to be `--color-surface-hi` on a
      // `--color-surface-hi` tile: exactly 1.00:1, an invisible container, while
      // its label sailed past AA.
      //
      // 1.15 is calibrated, not aspirational. Measured in-browser, the three
      // shipping chips separate from the surface behind them by:
      //
      //   project (teal)     1.41 / 1.38 / 1.29   (bg / surface / surface-hi)
      //   tag     (amber)    1.58 / 1.50 / 1.38
      //   category (violet)  1.37 / 1.31 / 1.19
      //
      // The floor sits just under the weakest real chip — category on
      // surface-hi — because redesigning that one is not this sprint's job.
      // What matters is that it is far above the failure being fenced: the old
      // tag chip was 1.00, a container painted in its container's own colour.
      // Raise this when the category chip is revisited.
      const edge = ratio(composited as RGB, surfaceByName[sname]);
      expect(
        edge,
        `${chip.name} chip has no edge on ${sname}: ${edge.toFixed(2)}:1`,
      ).toBeGreaterThanOrEqual(1.15);
    }
  }
});

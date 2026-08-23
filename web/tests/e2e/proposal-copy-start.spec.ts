import { test, expect } from "@playwright/test";

// WI #1550 — the `/start-sprint` prompt is copied from wherever a proposal is
// read, not only from the queue. kfdc's pane opens `/planning/<id>` directly,
// so the detail page was the one surface where deciding to start the thing you
// were reading meant navigating away first.
//
// Both surfaces are asserted in one spec on purpose: the change moved the
// button into a shared component, and the failure that would matter is the
// Planning card losing it while the detail page gains one.

/** Read the system clipboard the page just wrote to. */
async function clipboard(page: import("@playwright/test").Page): Promise<string> {
  return page.evaluate(() => navigator.clipboard.readText());
}

test("both proposal surfaces copy the /start-sprint prompt", async ({
  page,
  context,
  request,
}) => {
  // 127.0.0.1 is a secure context, so `navigator.clipboard` is present and the
  // primary path is the one under test. The read permission is the test's, not
  // korg's — korg never calls readText (kfdc withholds it deliberately, korg+
  // GP-17), and this is how the spec sees what was written.
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);

  const stamp = Date.now();
  const pid = (
    await (
      await request.post("/api/projects", { data: { name: `e2e-copy-${stamp}` } })
    ).json()
  ).id as number;
  const title = `copy sprint ${stamp}`;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title, summary: "the routing contract", project_id: pid },
    })
  ).json();
  const expected = `/start-sprint korg:${proposal.node_id}`;

  // 1. The Planning card — where the button has always been.
  await page.goto("/planning");
  const card = page.getByTestId(`proposal-${proposal.node_id}`);
  await expect(card).toBeVisible();
  await card.getByTestId("proposal-copy-start").click();
  await expect(card.getByTestId("proposal-copy-start")).toHaveText("✓");
  expect(await clipboard(page)).toBe(expected);

  // Clear it, so the detail page's copy cannot be confused with this one.
  await page.evaluate(() => navigator.clipboard.writeText("not the prompt"));

  // 2. The detail page — #1550's ask. Same prompt, same component, and here it
  //    carries its label because the page has room for one.
  await page.goto(`/planning/${proposal.node_id}`);
  const button = page.getByTestId("proposal-copy-start");
  await expect(button).toContainText("Copy /start-sprint");
  await button.click();
  await expect(button).toContainText("Copied");
  expect(await clipboard(page)).toBe(expected);

  // No toast: the copy succeeded, and #1498's message is for the case where it
  // does not.
  await expect(page.getByTestId("toast-error")).toHaveCount(0);
});

// WI #1498 — the failure message. A rejected `writeText()` used to report
// "Copy to clipboard failed" and stop, which tells the one audience that meets
// it — the next cross-origin embedder (korg-dash, kdeskdash, korg-vs) — nothing
// about the `allow` attribute that fixes it (korg+ GP-17).
//
// The rejection is the fixture, not the thing under test. Producing a real one
// means an embedder that withheld the delegation, from an origin on korg's
// `frame-ancestors` allowlist, with the frame reloaded so policy recomputes —
// the same construct-the-failure cost that #1498's comment 1061 measured and
// declined. korg's *response* to a rejection is what this asserts, so the
// rejection is stubbed and the assertions are all on korg.
test("a blocked copy names the fix and offers a way out", async ({ page, request }) => {
  const stamp = Date.now() + 1;
  const pid = (
    await (
      await request.post("/api/projects", { data: { name: `e2e-copyfail-${stamp}` } })
    ).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `blocked copy ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();

  await page.addInitScript(() => {
    // The rejection branch specifically: the object is PRESENT and writeText
    // fails. Deleting `navigator.clipboard` instead would exercise the absence
    // branch, which is the LAN-over-HTTP case and already works.
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: () =>
          Promise.reject(
            new DOMException("The Clipboard API has been blocked", "NotAllowedError"),
          ),
      },
    });
  });

  await page.goto(`/planning/${proposal.node_id}`);
  await page.getByTestId("proposal-copy-start").click();

  const toast = page.getByTestId("toast-error");
  await expect(toast).toBeVisible();
  // The browser's own message survives — the hint is a second line, not a
  // replacement, so the composed sentence still says what went wrong.
  await expect(toast).toContainText("Copy to clipboard failed");
  await expect(toast).toContainText("The Clipboard API has been blocked");
  await expect(page.getByTestId("toast-hint")).toContainText('allow="clipboard-write"');
  // The escape hatch is a real link to korg's own page for this proposal, built
  // from the generated route table (GP-16) rather than a hand-made path, and
  // opened top-level so the copy works where it lands.
  const action = page.getByTestId("toast-action");
  await expect(action).toHaveAttribute("href", `/planning/${proposal.node_id}`);
  await expect(action).toHaveAttribute("target", "_blank");
});

// The Planning card reports the same way, because it is the same component.
// Worth its own assertion: the failure path is the half of #1550's extraction
// that nothing else on the page would notice going missing.
test("the Planning card reports a blocked copy the same way", async ({ page, request }) => {
  const stamp = Date.now() + 2;
  const pid = (
    await (
      await request.post("/api/projects", { data: { name: `e2e-copyfail-card-${stamp}` } })
    ).json()
  ).id as number;
  const proposal = await (
    await request.post("/api/proposals", {
      data: { title: `blocked card copy ${stamp}`, summary: "s", project_id: pid },
    })
  ).json();

  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: () =>
          Promise.reject(new DOMException("Document is not focused.", "NotAllowedError")),
      },
    });
  });

  await page.goto("/planning");
  await page.getByTestId(`proposal-${proposal.node_id}`).getByTestId("proposal-copy-start").click();

  await expect(page.getByTestId("toast-error")).toContainText("Copy to clipboard failed");
  await expect(page.getByTestId("toast-hint")).toContainText('allow="clipboard-write"');
  await expect(page.getByTestId("toast-action")).toHaveAttribute(
    "href",
    `/planning/${proposal.node_id}`,
  );
  // The card must not claim success it did not have.
  await expect(
    page.getByTestId(`proposal-${proposal.node_id}`).getByTestId("proposal-copy-start"),
  ).toHaveText("⧉");
});

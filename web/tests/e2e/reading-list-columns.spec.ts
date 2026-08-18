import { test, expect } from "@playwright/test";

// WI #1115 — the reading-list rows' Edit buttons must line up.
//
// The bug was pure flex: the row was `justify-between` with four content-sized
// children, so each row spread its own leftover width between all four, and
// Edit landed at whatever x that row's title length produced. Ken's screenshot
// (img-46b) is two rows with very different title lengths and Edit in two
// different places — which is exactly what this spec seeds.
//
// Asserting geometry rather than class names: the class list is the fix, the
// alignment is the requirement, and a later restyle that keeps the alignment
// should not fail here.

test("Edit and the disposition chip are columns, whatever the title length", async ({
  page,
  request,
}) => {
  const stamp = Date.now();
  const short = `t${stamp}`;
  const long = `a considerably longer reading list title that runs on and on ${stamp}`;

  await request.post("/api/links", {
    data: { url: `https://example.com/short-${stamp}`, title: short },
  });
  const longLink = await (
    await request.post("/api/links", {
      data: { url: `https://example.com/long-${stamp}`, title: long },
    })
  ).json();
  // Different dispositions too: the chip is the last item, so its width sets
  // where Edit sits. `Unread` and `Summarized` are the widest gap in the
  // vocabulary, and before the fixed width they moved Edit by a few px.
  await request.patch(`/api/links/${longLink.node_id}`, {
    data: { disposition: "Summarized" },
  });

  await page.goto("/reading-list");

  const shortEdit = page.getByRole("button", { name: `Edit ${short}` });
  const longEdit = page.getByRole("button", { name: `Edit ${long}` });
  await expect(shortEdit).toBeVisible();
  await expect(longEdit).toBeVisible();

  const a = await shortEdit.boundingBox();
  const b = await longEdit.boundingBox();
  expect(a).not.toBeNull();
  expect(b).not.toBeNull();
  // One pixel of tolerance for sub-pixel layout; the bug was tens of pixels.
  expect(Math.abs(a!.x - b!.x)).toBeLessThanOrEqual(1);

  // And the long title truncates rather than pushing the row wide — `min-w-0`
  // is what makes the ellipsis actually engage on a flex child.
  const anchor = page.getByRole("link", { name: long });
  const row = anchor.locator("xpath=..");
  const anchorBox = await anchor.boundingBox();
  const rowBox = await row.boundingBox();
  expect(anchorBox!.width).toBeLessThanOrEqual(rowBox!.width);
});

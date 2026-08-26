import { test, expect, type Page, type Locator } from "@playwright/test";

// Sprint 057 (#1120, #1121) — the images UI, against the real engine.
//
// This drives genuine PNG bytes through a genuine multipart upload: the test
// paints a canvas in the page, turns it into a File, and hands that File to a
// synthetic paste. Nothing here is stubbed, because every interesting failure
// in this feature lives in the seam between the browser's clipboard and the
// server's store — a mocked upload would pass while the real one 400s.

/** Paste a freshly-painted 240×120 PNG into a textarea, the way Ctrl-V does.
 *
 *  The image is painted in the page rather than read from a fixture so the
 *  bytes are unambiguously a real encoded PNG — the server sniffs the format
 *  from the bytes and ignores what the upload claims, so a fixture that had
 *  gone stale would fail in a confusing place. */
async function pasteImage(target: Locator, name: string) {
  await target.evaluate(async (el, name) => {
    const canvas = document.createElement("canvas");
    canvas.width = 240;
    canvas.height = 120;
    const g = canvas.getContext("2d")!;
    g.fillStyle = "#2b6cb0";
    g.fillRect(0, 0, 240, 120);
    g.fillStyle = "#f6e05e";
    g.fillRect(20, 20, 80, 80);
    const blob: Blob = await new Promise((resolve) =>
      canvas.toBlob((b) => resolve(b!), "image/png"),
    );
    const dt = new DataTransfer();
    dt.items.add(new File([blob], name, { type: "image/png" }));
    el.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }),
    );
  }, name);
}

async function newProject(page: Page, project: string) {
  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();
}

test("paste a screenshot into a new work item and it lands inline", async ({ page }) => {
  const project = `e2e-img-${Date.now()}`;
  const title = `pasted shot ${Date.now()}`;

  await newProject(page, project);
  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);

  const content = page.getByTestId("wi-content");
  await content.fill("before\n");
  await pasteImage(content, "shot.png");

  // The token replaces the placeholder once the upload resolves. Asserting on
  // the *token* rather than on a spinner is the point: this is the text that
  // gets saved, and a placeholder left behind is the failure mode.
  await expect(content).toHaveValue(/!\[img-[0-9a-f]+\]\(\/api\/img\/img-[0-9a-f]+\/thumb\)/);
  await expect(content).toHaveValue(/^before\n/);

  await page.getByRole("button", { name: "Save" }).first().click();
  const row = page.getByRole("row", { name: new RegExp(title) });
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();

  // Inline: the token rendered as a thumbnail control, not as a broken image
  // and not as literal markdown.
  const thumb = page.locator(".prose button.korg-thumb").first();
  await expect(thumb).toBeVisible();
  await expect(thumb.locator("img")).toHaveAttribute("src", /\/api\/img\/img-[0-9a-f]+\/thumb/);

  // Linked, not pending — the save claimed it — and reported as inline.
  const list = page.getByTestId("attachments");
  await expect(list).toContainText("shot.png");
  await expect(list).toContainText("inline");
  await expect(list).not.toContainText("pending");

  // Click the thumbnail: the lightbox opens on the *original*, not the thumb.
  await thumb.click();
  const full = page.getByTestId("lightbox-image");
  await expect(full).toBeVisible();
  await expect(full).toHaveAttribute("src", /\/api\/img\/img-[0-9a-f]+$/);
  // And it is a real decoded image, not a 404 the browser rendered as nothing.
  expect(await full.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(240);
  await page.keyboard.press("Escape");
  await expect(full).toBeHidden();

  // Discard takes the attachment. The token stays in the text — placement and
  // existence are separate (handoff D2) — so what disappears is the list entry.
  await page.getByRole("button", { name: /^Discard img-/ }).click();
  await page.getByRole("button", { name: /^Confirm: Discard img-/ }).click();
  await expect(list).toContainText("No attachments.");
});

test("paste into a comment, and the image belongs to the node", async ({ page }) => {
  const project = `e2e-imgc-${Date.now()}`;
  const title = `commented shot ${Date.now()}`;

  await newProject(page, project);
  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByTestId("wi-content").fill("body");
  await page.getByRole("button", { name: "Save" }).first().click();

  const row = page.getByRole("row", { name: new RegExp(title) });
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await expect(page.getByTestId("attachments")).toContainText("No attachments.");

  const input = page.getByTestId("comment-input");
  await input.fill("see this: ");
  await pasteImage(input, "in-comment.png");
  await expect(input).toHaveValue(/!\[img-[0-9a-f]+\]/);
  await input.press("Control+Enter");

  // The comment shows the picture, and since #1625 the surrounding text is
  // rendered markdown rather than literal — `toContainText` reads either, so
  // what this still pins is that the token became a thumbnail and the prose
  // around it survived the swap.
  const comments = page.getByTestId("comment-list");
  await expect(comments).toContainText("see this:");
  await expect(comments.locator("button.korg-thumb img")).toBeVisible();

  // A comment is not a node (handoff I3), so the image is owned by the work
  // item — and shows up in *its* attachment list, linked rather than pending.
  const list = page.getByTestId("attachments");
  await expect(list).toContainText("in-comment.png");
  await expect(list).not.toContainText("pending");
  // Placed in a comment body, which counts as inline.
  await expect(list).toContainText("inline");
});

test("the paperclip attaches a file that no text mentions", async ({ page }) => {
  const project = `e2e-imgp-${Date.now()}`;
  const title = `clipped ${Date.now()}`;

  await newProject(page, project);
  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByTestId("wi-content").fill("no images in this text");
  await page.getByRole("button", { name: "Save" }).first().click();

  const row = page.getByRole("row", { name: new RegExp(title) });
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();

  await page.getByTestId("attach-input").setInputFiles({
    name: "clipped.png",
    mimeType: "image/png",
    // A 1×1 PNG — the smallest thing the decoder will accept.
    buffer: Buffer.from(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
      "base64",
    ),
  });

  // Attached to the item, never placed in its prose: exactly the state the
  // "not inline" indicator exists to name.
  const list = page.getByTestId("attachments");
  await expect(list).toContainText("clipped.png");
  await expect(list).toContainText("not inline");
  await expect(list).not.toContainText("pending");
  await expect(page.locator(".prose button.korg-thumb")).toHaveCount(0);
});

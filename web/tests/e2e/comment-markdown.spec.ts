import { test, expect } from "@playwright/test";

// #1625 — comments render as markdown.
//
// This reverses the #1120/#1121 rule ("comments render as literal text in korg
// and keep doing so: a `#582` is a work-item reference and a `*` is an
// asterisk"), and the cases below are the ones that rule existed to protect.
// The argument for reversing it is that korg's renderer config already defends
// them — `breaks: true`, and an ATX heading requiring whitespace after the
// hashes. That is an argument; this is the check. Every assertion here would
// have been a reason NOT to ship the swap.

test("comment markdown renders without eating korg's own notation", async ({ page }) => {
  const project = `e2e-cmark-${Date.now()}`;
  const title = `markdown comment ${Date.now()}`;

  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();

  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await page.getByPlaceholder("Title").fill(title);
  await page.getByPlaceholder("Content (markdown)").fill("body");
  await page.getByRole("button", { name: "Save" }).first().click();

  const row = page.getByRole("row", { name: new RegExp(title) });
  await expect(row).toBeVisible();
  await row.click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();

  // One comment carrying all four cases at once, because they interact: the
  // list and the emphasis have to survive `breaks: true` turning every newline
  // into a <br>.
  await page.getByTestId("comment-input").fill(
    ["see #582 and snake_case", "then *emphasis* here", "- alpha", "- beta"].join("\n"),
  );
  await page.getByTestId("comment-input").press("Control+Enter");

  const comments = page.getByTestId("comment-list");
  await expect(comments).toContainText("see #582 and snake_case");

  // `#582` is a work-item reference, not a heading. An ATX heading needs
  // whitespace after the hashes, which is the whole reason this is safe.
  await expect(comments.locator("h1, h2, h3, h4, h5, h6")).toHaveCount(0);

  // Intraword underscores are not emphasis, so identifiers survive. Asserted
  // rather than assumed: this is the case agents write constantly.
  await expect(comments.locator("em")).toHaveCount(1);
  await expect(comments.locator("em")).toHaveText("emphasis");

  // What the reversal buys: real lists, and line breaks that still behave the
  // way `whitespace-pre-wrap` used to.
  //
  // Scoped to `.prose` deliberately — `comment-list` is itself a <ul> and each
  // comment is an <li> of it, so an unscoped `li` locator counts the thread's
  // own chrome along with the list the markdown produced.
  const rendered = comments.locator(".prose");
  await expect(rendered.locator("li")).toHaveCount(2);
  await expect(rendered.locator("br")).not.toHaveCount(0);
});

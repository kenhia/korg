import { test, expect, type Page } from "@playwright/test";

// WI #2845 — typed-but-unsaved text survives the click that would throw it
// away. The report was a half-written work item and a moment of "let me just
// check something"; korg had no guard on any of the three ways that form dies.
//
// The suite watches `window.confirm` rather than asserting on korg markup: the
// guard is deliberately the native prompt (see `unsavedGuard.ts` for why), and
// a test that reached for a korg dialog would be asserting the opposite of the
// decision.

/**
 * Collect every native prompt, answering each one the same way.
 *
 * Read the tally with `expect.poll`, never a bare `expect`: on the router
 * paths the confirm fires inside `beforeNavigate`, which SvelteKit runs *after*
 * the click handler returns — so `click()` resolves before the dialog exists.
 * A bare assertion here passes or fails on timing rather than on behaviour.
 */
function watchDialogs(page: Page, answer: "accept" | "dismiss"): string[] {
  const seen: string[] = [];
  page.on("dialog", async (d) => {
    seen.push(d.message());
    await (answer === "accept" ? d.accept() : d.dismiss());
  });
  return seen;
}

async function openCreateForm(page: Page): Promise<void> {
  const project = `e2e-guard-${Date.now()}`;
  await page.goto("/work-items");
  await page.getByPlaceholder("new project…").fill(project);
  await page.getByPlaceholder("new project…").press("Enter");
  await expect(page.getByRole("button", { name: project, exact: true })).toBeVisible();
  await page.getByRole("button", { name: "+ New Work Item" }).click();
  await expect(page.getByPlaceholder("Title")).toBeVisible();
}

test("navigating away from a dirty form asks, and staying keeps the text", async ({ page }) => {
  const prompts = watchDialogs(page, "dismiss");
  await openCreateForm(page);
  await page.getByTestId("wi-content").fill("half a thought worth keeping");

  await page.getByRole("link", { name: "Planning" }).click();

  // Dismissed: still here, still typed.
  await expect.poll(() => prompts.length).toBe(1);
  expect(prompts[0]).toContain("unsaved");
  await expect(page).toHaveURL(/\/work-items/);
  await expect(page.getByTestId("wi-content")).toHaveValue("half a thought worth keeping");
});

test("an untouched form is not unsaved work", async ({ page }) => {
  const prompts = watchDialogs(page, "dismiss");
  await openCreateForm(page);

  // Opened and not typed in: leaving costs nothing, so it must not ask. A
  // confirm on a form nobody edited is how confirms stop being read.
  await page.getByRole("link", { name: "Planning" }).click();
  await expect(page).toHaveURL(/\/planning/);
  expect(prompts).toHaveLength(0);
});

test("accepting the prompt lets the navigation through", async ({ page }) => {
  const prompts = watchDialogs(page, "accept");
  await openCreateForm(page);
  await page.getByTestId("wi-content").fill("this one is expendable");

  await page.getByRole("link", { name: "Planning" }).click();
  await expect(page).toHaveURL(/\/planning/);
  await expect.poll(() => prompts.length).toBe(1);
});

test("Cancel and Escape ask too — the form dies by state, not by routing", async ({ page }) => {
  const prompts = watchDialogs(page, "dismiss");
  await openCreateForm(page);
  await page.getByPlaceholder("Title").fill("typed, then reconsidered");

  // Cancel: nothing navigates, the component is simply unmounted. The router
  // never hears about it, so the button has to ask on its own.
  await page.getByRole("button", { name: "Cancel" }).first().click();
  await expect.poll(() => prompts.length).toBe(1);
  await expect(page.getByPlaceholder("Title")).toHaveValue("typed, then reconsidered");

  // Escape closes the create form outright (work-items `onKey`), which is the
  // cheapest way there is to lose a paragraph.
  await page.getByRole("button", { name: "+ New Work Item" }).press("Escape");
  await expect.poll(() => prompts.length).toBe(2);
  await expect(page.getByPlaceholder("Title")).toHaveValue("typed, then reconsidered");
});

test("a saved form stops being dirty", async ({ page }) => {
  const prompts = watchDialogs(page, "dismiss");
  const title = `guard save ${Date.now()}`;
  await openCreateForm(page);
  await page.getByPlaceholder("Title").fill(title);
  await page.getByTestId("wi-content").fill("body");
  await page.getByRole("button", { name: "Save" }).first().click();
  await expect(page.getByRole("row", { name: new RegExp(title) })).toBeVisible();

  // The work is in korg now. A prompt here would be the guard crying wolf on
  // every single save, which is the failure that gets a guard switched off.
  await page.getByRole("link", { name: "Planning" }).click();
  await expect(page).toHaveURL(/\/planning/);
  expect(prompts).toHaveLength(0);
});

test("an unsent comment draft is unsaved work", async ({ page }) => {
  const prompts = watchDialogs(page, "dismiss");
  const title = `guard comment ${Date.now()}`;
  await openCreateForm(page);
  await page.getByPlaceholder("Title").fill(title);
  await page.getByTestId("wi-content").fill("body");
  await page.getByRole("button", { name: "Save" }).first().click();

  await page.getByRole("row", { name: new RegExp(title) }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();

  await page.getByTestId("comment-input").fill("a comment I have not posted yet");
  await page.getByRole("link", { name: "Planning" }).click();

  await expect.poll(() => prompts.length).toBe(1);
  await expect(page).toHaveURL(/\/work-items/);
});

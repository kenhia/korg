import { test, expect } from "@playwright/test";

// The behaviours WI #547 exists to guarantee: a failed request is never silent,
// and a failed load never looks like an empty one.
//
// Both are tested by intercepting the API rather than by breaking the server,
// because the point is what the *client* does with a failure — and a 500 that
// the client swallows leaves no trace anywhere else to assert on.

test("a failed load renders an error, not an empty page", async ({ page }) => {
  // link-up is the regression this guards. It loaded four collections with
  // `.catch(() => [])`, so an outage rendered a page that confidently said
  // there was nothing to link — indistinguishable from a genuinely empty
  // instance.
  await page.route("**/api/cards*", (r) =>
    r.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ error: "boom", code: "internal" }),
    }),
  );

  await page.goto("/link-up");

  const notice = page.getByTestId("error-notice");
  await expect(notice).toBeVisible();
  await expect(notice).toContainText(/couldn't load/i);
  // It must be announced, not merely painted.
  await expect(notice).toHaveAttribute("role", "alert");
});

test("a failed mutation surfaces a toast and does not change the list", async ({
  page,
}) => {
  await page.goto("/reading-list");
  await page.waitForLoadState("networkidle");

  const before = await page.locator("li").count();

  await page.route("**/api/links", async (route, request) => {
    if (request.method() === "POST") {
      await route.fulfill({
        status: 400,
        contentType: "application/json",
        body: JSON.stringify({
          error: "url must be absolute",
          code: "invalid_input",
        }),
      });
    } else {
      await route.continue();
    }
  });

  await page.getByLabel("Link URL").fill("https://example.com/nope");
  await page.getByRole("button", { name: "Add", exact: true }).click();

  const toast = page.getByTestId("toast-error");
  await expect(toast).toBeVisible();
  // The server's own sentence reaches the user — `invalid_input` is the
  // caller's problem, so the message is shown rather than replaced with a
  // generic apology.
  await expect(toast).toContainText("url must be absolute");

  // And the optimistic update never happened.
  expect(await page.locator("li").count()).toBe(before);
});

test("an internal error is reported without leaking server detail", async ({
  page,
}) => {
  await page.goto("/reading-list");
  await page.waitForLoadState("networkidle");

  await page.route("**/api/links", async (route, request) => {
    if (request.method() === "POST") {
      await route.fulfill({
        status: 500,
        contentType: "application/json",
        body: JSON.stringify({
          error: "sqlx: connection pool timed out",
          code: "internal",
        }),
      });
    } else {
      await route.continue();
    }
  });

  await page.getByLabel("Link URL").fill("https://example.com/boom");
  await page.getByRole("button", { name: "Add", exact: true }).click();

  const toast = page.getByTestId("toast-error");
  await expect(toast).toBeVisible();
  await expect(toast).toContainText(/internal error/i);
  // `internal` means korg broke; the raw server detail is console noise for a
  // user who did nothing wrong.
  await expect(toast).not.toContainText("sqlx");
});

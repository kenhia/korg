import { test, expect } from "@playwright/test";

function isoDate(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

test("drag a card into today's daily plan without moving its board status", async ({
  page,
}) => {
  const title = `planned card ${Date.now()}`;

  await page.goto("/cards");
  await page.getByPlaceholder("New card title…").fill(title);
  await page.getByPlaceholder("New card title…").press("Enter");

  const card = page.locator('[data-testid^="card-"]', { hasText: title });
  await expect(card).toBeVisible();
  await card.scrollIntoViewIfNeeded();
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();

  const day = page.getByTestId(`card-plan-day-${isoDate(new Date())}`);
  await expect(day).toBeVisible();
  await card.getByRole("button", { name: `Plan ${title}` }).dragTo(day);
  await expect(day.getByText(title)).toBeVisible({ timeout: 10000 });
  await expect(page.getByTestId("col-Backlog").getByText(title)).toBeVisible();
});

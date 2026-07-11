import { test, expect } from "@playwright/test";

function isoDate(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

test("history ends yesterday, reports stats, filters sources, and refreshes completion", async ({
  page,
}) => {
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const prior = new Date(today);
  prior.setDate(prior.getDate() - 2);
  let firstCompleted = false;
  const makeItems = () => [
    {
      node_id: 810001,
      plan_date: isoDate(prior),
      position: 0,
      display: "History alpha",
      source_node_id: 710001,
      source_kind: "topic",
      source_title: "History alpha",
      completed_at: firstCompleted ? "2026-01-01T01:00:00Z" : null,
      created_at: "2026-01-01T00:00:00Z",
    },
    {
      node_id: 810002,
      plan_date: isoDate(yesterday),
      position: 0,
      display: "History beta",
      source_node_id: 710002,
      source_kind: "card",
      source_title: "History beta",
      completed_at: "2026-01-01T01:00:00Z",
      created_at: "2026-01-01T00:00:00Z",
    },
  ];
  await page.route("**/api/daily-plan/history?**", async (route) => {
    const source = new URL(route.request().url()).searchParams.get(
      "source_node_id",
    );
    const items =
      source === null
        ? makeItems()
        : makeItems().filter((item) => String(item.source_node_id) === source);
    const completed = items.filter((item) => item.completed_at !== null).length;
    await route.fulfill({
      json: {
        from: isoDate(prior),
        to: isoDate(yesterday),
        total: items.length,
        completed,
        completion_rate: items.length === 0 ? 0 : completed / items.length,
        items,
      },
    });
  });
  await page.route("**/api/daily-plan/810001/completion", async (route) => {
    firstCompleted = true;
    await route.fulfill({ json: { ok: true } });
  });

  await page.goto("/history");
  await expect(page.getByTestId("history-stats")).toContainText("50%");
  await expect(page.getByTestId("history-stats")).toContainText(/1\s*\/\s*2/);
  await expect(page.getByTestId("history-list")).not.toContainText(
    isoDate(today),
  );
  await page.getByRole("checkbox", { name: "Complete History alpha" }).check();
  await expect(page.getByTestId("history-stats")).toContainText("100%");
  await page.getByLabel("Source").selectOption("710002");
  await expect(page.getByTestId("history-list")).toContainText("History beta");
  await expect(page.getByTestId("history-list")).not.toContainText(
    "History alpha",
  );
});

test("topic management creates, edits, and archives an active topic", async ({
  page,
}) => {
  const name = `managed topic ${Date.now()}`;
  const renamed = `${name} edited`;
  await page.goto("/topics");
  await page.getByRole("button", { name: "New topic" }).click();
  await page.getByLabel("Name").fill(name);
  await page.getByLabel("Description").fill("A managed planning topic");
  await page.getByLabel("Category").fill("focus");
  await page.getByLabel(/Tags/).fill("one, two");
  await page.getByRole("button", { name: "Save topic" }).click();
  await expect(page.getByText(name, { exact: true })).toBeVisible();

  await page.getByRole("button", { name: `Edit ${name}` }).click();
  await page.getByLabel("Name").fill(renamed);
  await page.getByRole("button", { name: "Save topic" }).click();
  await expect(page.getByText(renamed, { exact: true })).toBeVisible();
  await page.getByRole("button", { name: `Archive ${renamed}` }).click();
  await expect(page.getByText(renamed, { exact: true })).toHaveCount(0);
});

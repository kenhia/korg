import { test, expect, type Page } from "@playwright/test";

async function createTopicInDay(page: Page, date: string, name: string) {
  const container = page.getByTestId(`topic-picker-${date}`);
  const picker = container.getByRole("combobox", {
    name: `Add topic to ${date}`,
  });
  await picker.fill(name);
  const create = container.getByRole("option", {
    name: `Create topic “${name}”`,
  });
  await expect(create).toBeVisible();
  await picker.press("Enter");
  await expect(
    page.getByTestId(`planner-day-${date}`).getByText(name),
  ).toBeVisible();
}

test("select an existing topic and create a new topic in the planner", async ({
  page,
}) => {
  const first = `planner first ${Date.now()}`;
  const second = `planner second ${Date.now()}`;
  const created = await page.request.post("/api/topics", {
    data: { name: first },
  });
  expect(created.ok()).toBe(true);
  await page.goto("/");
  await page.getByRole("button", { name: "Next →" }).click();
  const days = page.locator('[data-testid^="planner-day-"]');
  await expect(days).toHaveCount(7);
  const dayIds = await days
    .evaluateAll((nodes) =>
      nodes.map((node) => node.getAttribute("data-testid") ?? ""),
    );
  const firstDate = dayIds[0].replace("planner-day-", "");
  const secondDate = dayIds[1].replace("planner-day-", "");

  const firstContainer = page.getByTestId(`topic-picker-${firstDate}`);
  const firstPicker = firstContainer.getByRole("combobox", {
    name: `Add topic to ${firstDate}`,
  });
  await firstPicker.fill(first);
  const firstOption = firstContainer.getByRole("option", {
    name: first,
    exact: true,
  });
  await expect(firstOption).toBeVisible();
  await firstPicker.press("Enter");
  await expect(
    page.getByTestId(`planner-day-${firstDate}`).getByText(first),
  ).toBeVisible();

  await createTopicInDay(page, secondDate, second);
});

test("complete, reorder, move, and delete open plan items", async ({
  page,
}) => {
  const first = `lifecycle first ${Date.now()}`;
  const second = `lifecycle second ${Date.now()}`;
  const firstTopic = await page.request.post("/api/topics", {
    data: { name: first },
  });
  const secondTopic = await page.request.post("/api/topics", {
    data: { name: second },
  });
  expect(firstTopic.ok()).toBe(true);
  expect(secondTopic.ok()).toBe(true);

  await page.goto("/");
  await page.getByRole("button", { name: "Next →" }).click();
  const days = page.locator('[data-testid^="planner-day-"]');
  await expect(days).toHaveCount(7);
  const dayIds = await days.evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute("data-testid") ?? ""),
  );
  const firstDate = dayIds[0].replace("planner-day-", "");
  const secondDate = dayIds[1].replace("planner-day-", "");
  const firstTopicBody = (await firstTopic.json()) as { node_id: number };
  const secondTopicBody = (await secondTopic.json()) as { node_id: number };
  expect(
    (
      await page.request.post("/api/daily-plan", {
        data: { source_node_id: firstTopicBody.node_id, plan_date: firstDate },
      })
    ).ok(),
  ).toBe(true);
  expect(
    (
      await page.request.post("/api/daily-plan", {
        data: { source_node_id: secondTopicBody.node_id, plan_date: firstDate },
      })
    ).ok(),
  ).toBe(true);
  await page.reload();
  await page.getByRole("button", { name: "Next →" }).click();
  await expect(
    page.getByTestId(`planner-day-${firstDate}`).getByText(first),
  ).toBeVisible();

  const firstItem = page
    .getByTestId(`planner-day-${firstDate}`)
    .locator('[data-testid^="plan-item-"]', { hasText: first });
  const secondItem = page
    .getByTestId(`planner-day-${firstDate}`)
    .locator('[data-testid^="plan-item-"]', { hasText: second });
  await firstItem.getByRole("checkbox").check();
  await expect(firstItem.getByRole("checkbox")).toBeChecked();

  await secondItem.dragTo(firstItem);
  await expect(page.getByRole("status")).toContainText("Plan updated");

  await firstItem.dragTo(page.getByTestId(`planner-day-${secondDate}`));
  await expect(
    page.getByTestId(`planner-day-${secondDate}`).getByText(first),
  ).toBeVisible();
  await page
    .getByTestId(`planner-day-${secondDate}`)
    .getByRole("button", { name: `Remove ${first}` })
    .click();
  await expect(
    page.getByTestId(`planner-day-${secondDate}`).getByText(first),
  ).toHaveCount(0);
});

test("past plan is frozen, completion remains editable, and dragging forward copies", async ({
  page,
}) => {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const iso = (date: Date) =>
    `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  const todayIso = iso(today);
  const pastIso = iso(yesterday);
  let copied = false;
  let completed = false;
  const pastItem = {
    node_id: 700001,
    plan_date: pastIso,
    position: 0,
    display: "Frozen source",
    source_node_id: 600001,
    source_kind: "topic",
    source_title: "Frozen source",
    completed_at: null,
    created_at: "2026-01-01T00:00:00Z",
  };

  await page.route("**/api/daily-plan?**", async (route) =>
    route.fulfill({
      json: copied
        ? [
            pastItem,
            {
              ...pastItem,
              node_id: 700002,
              plan_date: todayIso,
              completed_at: completed ? "2026-01-02T00:00:00Z" : null,
            },
          ]
        : [
            {
              ...pastItem,
              completed_at: completed ? "2026-01-02T00:00:00Z" : null,
            },
          ],
    }),
  );
  await page.route("**/api/topics", async (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route("**/api/cards", async (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route("**/api/work-items", async (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route("**/api/daily-plan/700001/completion", async (route) => {
    completed = true;
    await route.fulfill({ json: { ok: true } });
  });
  await page.route("**/api/daily-plan/700001/move", async (route) => {
    copied = true;
    await route.fulfill({ json: { node_id: 700002, copied: true } });
  });

  await page.goto("/");
  const pastDay = page.getByTestId(`planner-day-${pastIso}`);
  await expect(pastDay).toContainText("Frozen");
  await expect(
    pastDay.getByRole("button", { name: "Remove Frozen source" }),
  ).toHaveCount(0);
  await pastDay
    .getByRole("checkbox", { name: "Complete Frozen source" })
    .check();
  await expect(
    pastDay.getByRole("checkbox", { name: "Complete Frozen source" }),
  ).toBeChecked();
  await pastDay
    .locator('[data-testid="plan-item-700001"]')
    .dragTo(page.getByTestId(`planner-day-${todayIso}`));
  await expect(page.getByRole("status")).toContainText(
    "Copied from frozen history",
  );
  await expect(pastDay.getByText("Frozen source")).toBeVisible();
  await expect(
    page.getByTestId(`planner-day-${todayIso}`).getByText("Frozen source"),
  ).toBeVisible();
});

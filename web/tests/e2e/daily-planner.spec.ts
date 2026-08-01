import {
  test,
  expect,
  type Page,
  type APIRequestContext,
} from "@playwright/test";

// WI #701 — tall enough that a day column and its neighbour are both fully on
// screen. Playwright emulates a drag as hover-source → mouse-down →
// hover-target → mouse-up, and `dragstart` fires during that last move: if
// reaching the target scrolls the page, the row under the pressed pointer is no
// longer the one that was grabbed. On days holding rows from earlier runs the
// columns grew past the default 720px viewport and the drag picked up a
// bystander — the trace showed it moving plan item 972, a leftover, rather than
// the one the spec had just created. Nothing about the app needs this height;
// the drag emulation does.
test.use({ viewport: { width: 1600, height: 1400 } });

// The planner works in real calendar days, which every run of the suite shares,
// so a spec that plans into next week plans into the same two columns as every
// run before it. These work four weeks out — far enough that nothing has been
// planned there by hand — and remove what they planned when they are done, so
// the arena stays the empty one they assume.
const WEEKS_AHEAD = 4;

async function gotoFutureWeek(page: Page): Promise<string[]> {
  await page.goto("/");
  for (let i = 0; i < WEEKS_AHEAD; i++) {
    await page.getByRole("button", { name: "Next →" }).click();
  }
  const days = page.locator('[data-testid^="planner-day-"]');
  await expect(days).toHaveCount(7);
  const ids = await days.evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute("data-testid") ?? ""),
  );
  return ids.map((id) => id.replace("planner-day-", ""));
}

/** Best-effort: a failed cleanup must not fail a passing test. */
async function unplan(request: APIRequestContext, nodeIds: number[]) {
  for (const id of nodeIds) {
    await request.delete(`/api/daily-plan/${id}`).catch(() => undefined);
  }
}

async function planItemIds(
  request: APIRequestContext,
  date: string,
  match: string,
): Promise<number[]> {
  const res = await request.get(`/api/daily-plan?from=${date}&to=${date}`);
  if (!res.ok()) return [];
  const items = (await res.json()) as { node_id: number; display: string }[];
  return items.filter((i) => i.display.includes(match)).map((i) => i.node_id);
}

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
  request,
}) => {
  const first = `planner first ${Date.now()}`;
  const second = `planner second ${Date.now()}`;
  const created = await page.request.post("/api/topics", {
    data: { name: first },
  });
  expect(created.ok()).toBe(true);
  const [firstDate, secondDate] = await gotoFutureWeek(page);

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

  await unplan(request, [
    ...(await planItemIds(request, firstDate, first)),
    ...(await planItemIds(request, secondDate, second)),
  ]);
});

test("complete, reorder, move, and delete open plan items", async ({
  page,
  request,
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

  const [firstDate, secondDate] = await gotoFutureWeek(page);
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
  await gotoFutureWeek(page);
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
  // WI #701 — the toast is not the signal it looks like: `refresh()` raises it
  // *before* awaiting the reload that re-renders the list. Waiting on it and
  // then dragging again meant the next drag computed its grab point from the
  // pre-reorder layout.
  //
  // Waiting on the order itself is both the correct signal and the assertion
  // this spec was missing: it claimed to test reordering and only ever checked
  // that a toast appeared.
  const dayRows = page
    .getByTestId(`planner-day-${firstDate}`)
    .locator('[data-testid^="plan-item-"]');
  await expect(async () => {
    const texts = await dayRows.allInnerTexts();
    const at = (needle: string) => texts.findIndex((t) => t.includes(needle));
    expect(at(second)).toBeGreaterThanOrEqual(0);
    expect(at(first)).toBeGreaterThanOrEqual(0);
    expect(at(second)).toBeLessThan(at(first));
  }).toPass();

  await firstItem.dragTo(page.getByTestId(`planner-day-${secondDate}`));
  await expect(
    page.getByTestId(`planner-day-${secondDate}`).getByText(first),
  ).toBeVisible();
  // Two presses: removing a plan item is irreversible from this page, so it
  // confirms (WI #549).
  const removeItem = page
    .getByTestId(`planner-day-${secondDate}`)
    .getByRole("button", { name: `Remove ${first}` });
  await removeItem.click();
  await page
    .getByTestId(`planner-day-${secondDate}`)
    .getByRole("button", { name: `Confirm: Remove ${first}` })
    .click();
  await expect(
    page.getByTestId(`planner-day-${secondDate}`).getByText(first),
  ).toHaveCount(0);

  // `first` was removed through the UI above; `second` is still planned.
  await unplan(request, await planItemIds(request, firstDate, second));
});

// WI #701 — this spec asserts on "yesterday", and the planner renders a Mon–Sun
// week: on a Monday, yesterday is the *previous* week's Sunday and the element
// it waits for is never rendered. It was unrunnable one day in seven regardless
// of what was in the database.
//
// The fix is to pin the clock rather than to teach the spec about Mondays. Every
// API route it touches is already mocked below, so the server's idea of today
// never reaches it — the clock that matters is the browser's, and Playwright's
// `page.clock` pins exactly that. `timezoneId` pins it too, so the local-time
// arithmetic in `dates.ts` and the literals here cannot disagree about which day
// it is.
//
// Wednesday 2026-01-07: "yesterday" (Tue the 6th) and today both sit inside Mon
// the 5th – Sun the 11th, so the week the planner draws contains both. Pinning
// it to Monday the 5th instead reproduces the original failure exactly, which is
// how the pin was confirmed to be what carries this.
test.describe("frozen past", () => {
  test.use({ timezoneId: "Etc/UTC" });

  const NOW = "2026-01-07T12:00:00Z";
  const todayIso = "2026-01-07";
  const pastIso = "2026-01-06";

  test("past plan is frozen, completion remains editable, and dragging forward copies", async ({
    page,
  }) => {
    await page.clock.setFixedTime(new Date(NOW));
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
    await page.route("**/api/topics?**", async (route) =>
      route.fulfill({ json: { items: [], total: 0, limit: 500, offset: 0 } }),
    );
    await page.route("**/api/cards?**", async (route) =>
      route.fulfill({ json: { items: [], total: 0, limit: 500, offset: 0 } }),
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
});

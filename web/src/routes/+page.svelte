<script lang="ts">
  import { onMount } from "svelte";
  import TopicPicker from "$lib/components/TopicPicker.svelte";
  import {
    api,
    type Card,
    type DailyPlanItem,
    type Topic,
    type WorkItem,
  } from "$lib/api";
  import {
    addDays,
    isoDate,
    startOfWeek,
    weekDays,
    WEEKDAY_LABELS,
  } from "$lib/dates";

  const SOURCE_DRAG = "application/x-korg-source";
  const PLAN_DRAG = "application/x-korg-daily-plan-item";
  let weekStart = $state(startOfWeek(new Date()));
  let items = $state<DailyPlanItem[]>([]);
  let topics = $state<Topic[]>([]);
  let cards = $state<Card[]>([]);
  let workItems = $state<WorkItem[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let sourceSearch = $state("");
  let trayDate = $state(isoDate(new Date()));

  const days = $derived(weekDays(weekStart));
  const today = $derived(isoDate(new Date()));
  const sources = $derived(
    [
      ...cards
        .filter((card) => !card.archived && card.status !== "Cut")
        .map((card) => ({
          node_id: card.node_id,
          kind: "card",
          title: card.title,
        })),
      ...workItems
        .filter((item) => !item.archived && item.wi_status !== "closed")
        .map((item) => ({
          node_id: item.node_id,
          kind: "work item",
          title: `#${item.wi_number} ${item.title}`,
        })),
    ]
      .filter((source) =>
        source.title
          .toLocaleLowerCase()
          .includes(sourceSearch.toLocaleLowerCase()),
      )
      .slice(0, 40),
  );

  function itemsFor(date: string): DailyPlanItem[] {
    return items
      .filter((item) => item.plan_date === date)
      .sort((a, b) => a.position - b.position);
  }
  function frozen(date: string): boolean {
    return date < today;
  }
  function kindLabel(kind: DailyPlanItem["source_kind"]): string {
    return kind === "workitem" ? "WI" : kind === "card" ? "Card" : "Topic";
  }
  function kindClass(kind: DailyPlanItem["source_kind"]): string {
    return kind === "workitem"
      ? "bg-teal-950 text-teal-300"
      : kind === "card"
        ? "bg-violet-950 text-violet-300"
        : "bg-amber-950 text-amber-300";
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const from = isoDate(weekStart);
      const to = isoDate(addDays(weekStart, 6));
      const [plan, topicPage, cardPage, wiPage] = await Promise.all([
        api.dailyPlan(from, to),
        api.topics(),
        api.cards(),
        api.workItems(),
      ]);
      items = plan;
      topics = topicPage.items;
      cards = cardPage.items;
      workItems = wiPage.items;
      if (trayDate < today || trayDate < from || trayDate > to)
        trayDate = today >= from && today <= to ? today : from;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading = false;
    }
  }
  function shiftWeek(delta: number) {
    weekStart = addDays(weekStart, delta * 7);
    void load();
  }
  function resetWeek() {
    weekStart = startOfWeek(new Date());
    trayDate = today;
    void load();
  }
  async function refresh(message?: string) {
    if (message) notice = message;
    await load();
  }
  async function setCompletion(item: DailyPlanItem, completed: boolean) {
    try {
      await api.setDailyPlanCompletion(item.node_id, completed);
      await refresh(
        completed ? `Completed ${item.display}` : `Reopened ${item.display}`,
      );
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
      await load();
    }
  }
  async function remove(item: DailyPlanItem) {
    try {
      await api.deleteDailyPlanItem(item.node_id);
      await refresh(`Removed ${item.display}`);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }
  function dragPlan(event: DragEvent, item: DailyPlanItem) {
    event.dataTransfer?.setData(PLAN_DRAG, String(item.node_id));
    if (event.dataTransfer)
      event.dataTransfer.effectAllowed = frozen(item.plan_date)
        ? "copy"
        : "move";
  }
  function dragSource(event: DragEvent, sourceNodeId: number) {
    event.dataTransfer?.setData(SOURCE_DRAG, String(sourceNodeId));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy";
  }
  function draggedId(event: DragEvent, format: string): number | null {
    const value = event.dataTransfer?.getData(format) ?? "";
    return /^\d+$/.test(value) ? Number(value) : null;
  }
  async function dropOnDay(
    event: DragEvent,
    targetDate: string,
    targetPosition: number,
  ) {
    event.preventDefault();
    if (frozen(targetDate)) return;
    const planItemId = draggedId(event, PLAN_DRAG);
    const sourceNodeId = draggedId(event, SOURCE_DRAG);
    try {
      if (planItemId !== null) {
        const outcome = await api.moveDailyPlanItem(
          planItemId,
          targetDate,
          targetPosition,
        );
        await refresh(
          outcome.copied ? "Copied from frozen history" : "Plan updated",
        );
      } else if (sourceNodeId !== null) {
        await api.createDailyPlanItem(sourceNodeId, targetDate);
        await refresh("Added to plan");
      }
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
      await load();
    }
  }
  async function addSource(sourceNodeId: number, title: string) {
    try {
      await api.createDailyPlanItem(sourceNodeId, trayDate);
      await refresh(`Added ${title}`);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }
  onMount(load);
</script>

<section class="space-y-5">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <p
        class="text-xs font-medium uppercase tracking-[0.18em] text-[var(--color-accent)]"
      >
        Daily plan
      </p>
      <h1 class="text-2xl font-semibold">
        Week of {weekStart.toLocaleDateString(undefined, {
          month: "long",
          day: "numeric",
        })}
      </h1>
    </div>
    <div
      class="flex items-center rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-1 text-sm"
      aria-label="Week navigation"
    >
      <button
        class="rounded px-3 py-1.5 hover:bg-[var(--color-surface-hi)]"
        onclick={() => shiftWeek(-1)}>← Previous</button
      ><button
        class="rounded px-3 py-1.5 hover:bg-[var(--color-surface-hi)]"
        onclick={resetWeek}>Today</button
      ><button
        class="rounded px-3 py-1.5 hover:bg-[var(--color-surface-hi)]"
        onclick={() => shiftWeek(1)}>Next →</button
      >
    </div>
  </header>
  {#if error}<p
      class="rounded border border-red-900 bg-red-950 px-3 py-2 text-sm text-red-200"
      role="alert"
    >
      {error}
    </p>{/if}
  {#if notice}<p
      class="rounded border border-sky-900 bg-sky-950 px-3 py-2 text-sm text-sky-200"
      role="status"
    >
      {notice}
    </p>{/if}
  {#if loading}<p class="text-[var(--color-muted)]">Loading planner…</p>{:else}
    <div
      class="grid min-w-0 grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-7"
      data-testid="week-planner"
    >
      {#each days as day, index (isoDate(day))}
        {@const date = isoDate(day)}
        {@const dayItems = itemsFor(date)}
        <section
          class="flex min-h-72 min-w-0 flex-col rounded-lg border bg-[var(--color-surface)]"
          class:border-[var(--color-accent)]={date === today}
          class:border-[var(--color-border)]={date !== today}
          class:opacity-70={frozen(date)}
          data-testid={`planner-day-${date}`}
          aria-label={`${WEEKDAY_LABELS[index]} ${date}${frozen(date) ? ", frozen" : ""}`}
          ondragover={(event) => {
            if (!frozen(date)) event.preventDefault();
          }}
          ondrop={(event) => dropOnDay(event, date, dayItems.length)}
        >
          <div
            class="flex items-start justify-between border-b border-[var(--color-border)] px-3 py-2"
          >
            <div>
              <h2 class="text-sm font-semibold">{WEEKDAY_LABELS[index]}</h2>
              <p class="text-xs text-[var(--color-muted)]">
                {day.toLocaleDateString(undefined, {
                  month: "short",
                  day: "numeric",
                })}
              </p>
            </div>
            {#if frozen(date)}<span
                class="rounded bg-[var(--color-bg)] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[var(--color-muted)]"
                >Frozen</span
              >{:else if date === today}<span
                class="rounded bg-[var(--color-accent-soft)] px-1.5 py-0.5 text-[10px] uppercase tracking-wide"
                >Today</span
              >{/if}
          </div>
          <ol class="flex-1 space-y-2 p-2" aria-label={`Plan for ${date}`}>
            {#each dayItems as item, itemIndex (item.node_id)}
              <li
                class="group rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] p-2"
                class:opacity-60={item.completed_at !== null}
                draggable="true"
                data-testid={`plan-item-${item.node_id}`}
                ondragstart={(event) => dragPlan(event, item)}
                ondragover={(event) => {
                  if (!frozen(date)) event.preventDefault();
                }}
                ondrop={(event) => {
                  event.stopPropagation();
                  void dropOnDay(event, date, itemIndex);
                }}
              >
                <div class="flex items-start gap-2">
                  <input
                    type="checkbox"
                    class="mt-0.5 accent-sky-500"
                    aria-label={`Complete ${item.display}`}
                    checked={item.completed_at !== null}
                    onchange={(event) =>
                      setCompletion(item, event.currentTarget.checked)}
                  />
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-1.5">
                      <span
                        class={`shrink-0 rounded px-1 py-0.5 text-[9px] font-semibold uppercase ${kindClass(item.source_kind)}`}
                        >{kindLabel(item.source_kind)}</span
                      ><span
                        class="truncate text-xs font-medium"
                        class:line-through={item.completed_at !== null}
                        >{item.display}</span
                      >
                    </div>
                    {#if item.source_title !== item.display}<p
                        class="mt-1 truncate text-[10px] text-[var(--color-muted)]"
                        title="Current source title"
                      >
                        Now: {item.source_title}
                      </p>{/if}
                  </div>
                  {#if !frozen(date)}<button
                      class="rounded px-1 text-xs text-[var(--color-muted)] hover:bg-red-950 hover:text-red-200"
                      aria-label={`Remove ${item.display}`}
                      title="Remove from day"
                      onclick={() => remove(item)}>✕</button
                    >{/if}
                </div>
              </li>
            {:else}<li
                class="rounded border border-dashed border-[var(--color-border)] px-2 py-5 text-center text-xs text-[var(--color-muted)]"
              >
                {frozen(date) ? "No planned items" : "Drop a card or work item"}
              </li>{/each}
          </ol>
          {#if !frozen(date)}<div
              class="border-t border-[var(--color-border)] p-2"
            >
              <TopicPicker {topics} planDate={date} onadded={refresh} />
            </div>{/if}
        </section>
      {/each}
    </div>
  {/if}
  <section
    class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
    aria-labelledby="source-tray-title"
  >
    <div class="mb-3 flex flex-wrap items-end justify-between gap-3">
      <div>
        <h2 id="source-tray-title" class="font-semibold">Cards & work items</h2>
        <p class="text-xs text-[var(--color-muted)]">
          Drag into an open day, or choose a day and use Add.
        </p>
      </div>
      <div class="flex items-end gap-2">
        <label class="text-xs text-[var(--color-muted)]"
          >Add to <select
            class="ml-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)]"
            bind:value={trayDate}
            >{#each days.filter((day) => !frozen(isoDate(day))) as day (isoDate(day))}<option
                value={isoDate(day)}
                >{WEEKDAY_LABELS[(day.getDay() + 6) % 7]}
                {day.getDate()}</option
              >{/each}</select
          ></label
        ><input
          type="search"
          aria-label="Search cards and work items"
          class="w-64 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
          placeholder="Search sources…"
          bind:value={sourceSearch}
        />
      </div>
    </div>
    <ul class="grid max-h-56 gap-2 overflow-auto sm:grid-cols-2 lg:grid-cols-4">
      {#each sources as source (`${source.kind}-${source.node_id}`)}<li
          class="flex cursor-grab items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2 active:cursor-grabbing"
          draggable="true"
          ondragstart={(event) => dragSource(event, source.node_id)}
        >
          <span
            class="rounded bg-[var(--color-surface-hi)] px-1 text-[10px] uppercase text-[var(--color-muted)]"
            >{source.kind}</span
          ><span class="min-w-0 flex-1 truncate text-xs">{source.title}</span
          ><button
            class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]"
            onclick={() => addSource(source.node_id, source.title)}>Add</button
          >
        </li>{:else}<li class="text-sm text-[var(--color-muted)]">
          No matching active sources.
        </li>{/each}
    </ul>
  </section>
</section>

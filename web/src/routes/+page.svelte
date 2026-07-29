<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import TopicPicker from "$lib/components/TopicPicker.svelte";
  import {
    api,
    type CardRow,
    type DailyPlanItem,
    type ProposalRow,
    type ReportRow,
    type Topic,
  } from "$lib/api";
  import { chip, isCut, kindLabel, reportStatusPill } from "$lib/domain";
  import { attempt, notify, reportError } from "$lib/toast.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import ConfirmButton from "$lib/components/ConfirmButton.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";
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
  let cards = $state<CardRow[]>([]);
  let proposals = $state<ProposalRow[]>([]);
  // The most recent daily report, for the health pill beside the week heading.
  // `list_reports` orders by report_date DESC, so the head of the list is it.
  let latestReport = $state<ReportRow | null>(null);
  let loading = $state(true);
  let loadError = $state<unknown>(null);
  let sourceSearch = $state("");
  let trayDate = $state(isoDate(new Date()));
  let previewNode = $state<number | null>(null);

  // Proposals open, cards collapsed (Ken, 2026-07-29). Today is a planning
  // overview: "which sprint am I on" is the question it should answer at a
  // glance, and the card list is a long tray you open when you want it.
  let showProposals = $state(true);
  let showCards = $state(false);

  const days = $derived(weekDays(weekStart));
  const today = $derived(isoDate(new Date()));

  /** The target day, spelled the way the column header spells it. */
  const targetLabel = $derived.by(() => {
    const d = days.find((day) => isoDate(day) === trayDate);
    if (!d) return trayDate;
    const label = WEEKDAY_LABELS[(d.getDay() + 6) % 7];
    return `${label} ${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}`;
  });

  function matches(title: string): boolean {
    return title
      .toLocaleLowerCase()
      .includes(sourceSearch.toLocaleLowerCase());
  }

  // The tray is cards only as of sprint 029 — work items left it. They are
  // still plannable (drag from anywhere that exposes one, and the API still
  // takes them); they were simply drowning the list Ken actually picks from.
  const cardSources = $derived(
    cards
      .filter((card) => !card.archived && !isCut(card.status))
      .filter((card) => matches(card.title))
      .slice(0, 40),
  );

  // Only the live queue: a done or declined proposal is not something you plan
  // a day around.
  const proposalSources = $derived(
    proposals
      .filter((p) => !p.archived && (p.status === "proposed" || p.status === "active"))
      .filter((p) => matches(p.title))
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
  /** Saturday and Sunday, by position in the Mon-first week. */
  function weekend(index: number): boolean {
    return index >= 5;
  }
  // One colour per plannable kind. `sprint_proposal` needed its own rather than
  // inheriting the catch-all, which would have made a planned sprint look like
  // a topic.
  const KIND_CLASSES: Record<string, string> = {
    workitem: "bg-teal-950 text-teal-300",
    card: "bg-violet-950 text-violet-300",
    topic: "bg-amber-950 text-amber-300",
    sprint_proposal: "bg-sky-950 text-sky-300",
  };
  function kindClass(kind: DailyPlanItem["source_kind"]): string {
    return KIND_CLASSES[kind] ?? "bg-[var(--color-surface-hi)] text-[var(--color-muted)]";
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      const from = isoDate(weekStart);
      const to = isoDate(addDays(weekStart, 6));
      const [plan, topicPage, cardPage, proposalRows, reportRows] =
        await Promise.all([
          api.dailyPlan(from, to),
          api.topics(),
          api.cards(),
          api.proposals(),
          api.reports(),
        ]);
      items = plan;
      topics = topicPage.items;
      cards = cardPage.items;
      proposals = proposalRows;
      latestReport = reportRows[0] ?? null;
      if (trayDate < today || trayDate < from || trayDate > to)
        trayDate = today >= from && today <= to ? today : from;
    } catch (cause) {
      loadError = cause;
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
    if (message) notify(message);
    await load();
  }
  async function setCompletion(item: DailyPlanItem, completed: boolean) {
    const r = await attempt(
      () => api.setDailyPlanCompletion(item.node_id, completed),
      completed ? "Complete item" : "Reopen item",
    );
    // The checkbox is bound to server state, so resynchronise either way.
    await load();
    if (r) {
      notify(completed ? `Completed ${item.display}` : `Reopened ${item.display}`);
    }
  }

  // Removing a plan item is not undoable through this page (re-adding needs the
  // source and position), so it confirms — see the ConfirmButton at the call
  // site (WI #549).
  async function remove(item: DailyPlanItem) {
    const r = await attempt(
      () => api.deleteDailyPlanItem(item.node_id),
      "Remove from plan",
    );
    if (!r) return;
    await refresh(`Removed ${item.display}`);
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
      // The dragged item is showing where it was dropped, not where it is.
      reportError(cause, "Update the plan");
      await load();
    }
  }
  async function addSource(sourceNodeId: number, title: string) {
    const r = await attempt(
      () => api.createDailyPlanItem(sourceNodeId, trayDate),
      "Add to plan",
    );
    if (r) await refresh(`Added ${title}`);
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
      <div class="flex flex-wrap items-center gap-4">
        <h1 class="text-2xl font-semibold">
          Week of {weekStart.toLocaleDateString(undefined, {
            month: "long",
            day: "numeric",
          })}
        </h1>
        <!-- Latest daily-report status (Ken, 2026-07-29). Today is meant to be
             the page you open for an overview, and "is anything wrong?" is part
             of that overview. Links through to the report itself — the status
             is the question, the report is the answer. Same pill as the Daily
             Reports page, from one definition, so the colours cannot drift. -->
        {#if latestReport}
          <a
            href="/daily-reports"
            class={reportStatusPill(latestReport.status)}
            data-testid="latest-report-status"
            title={`Daily report ${latestReport.report_date} — ${latestReport.summary}`}
            >{latestReport.status}</a
          >
        {/if}
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <!-- History and Topics live here rather than the top nav (sprint 029):
           they are things you step into from planning and come back from, not
           destinations you start at. Both accept Esc to return. -->
      <div
        class="flex items-center rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-1 text-sm"
      >
        <a
          class="rounded px-3 py-1.5 hover:bg-[var(--color-surface-hi)]"
          href="/history">History</a
        ><a
          class="rounded px-3 py-1.5 hover:bg-[var(--color-surface-hi)]"
          href="/topics">Topics</a
        >
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
    </div>
  </header>
  {#if loadError}
    <ErrorNotice error={loadError} what="the planner" retry={load} />
  {/if}
  {#if loading}<p class="text-[var(--color-muted)]">Loading planner…</p>{:else}
    <div
      class="grid min-w-0 grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-7"
      data-testid="week-planner"
    >
      {#each days as day, index (isoDate(day))}
        {@const date = isoDate(day)}
        {@const dayItems = itemsFor(date)}
        <!-- Weekend columns carry a violet wash so the week has a visible
             shape at a glance (sprint 029). Subtle on purpose: it should read
             as "different kind of day", not as a status. -->
        <section
          class={`flex min-h-72 min-w-0 flex-col rounded-lg border ${
            weekend(index) ? "bg-violet-500/10" : "bg-[var(--color-surface)]"
          }`}
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
          <!-- The header is the target control (Ken, 2026-07-29). "Add to" was
               already a dropdown down in the tray, but nothing about a dropdown
               says "this is where things land" — clicking the day does. Amber
               rather than green: green reads as *complete* on a page whose
               items have completion checkboxes, and it would fight the blue
               "today" border, which has to stay legible at the same time. -->
          <div
            class={`flex items-start justify-between border-b border-[var(--color-border)] ${
              date === trayDate && !frozen(date) ? "bg-amber-500/20" : ""
            }`}
          >
            <button
              class="flex-1 px-3 py-2 text-left"
              class:cursor-default={frozen(date)}
              disabled={frozen(date)}
              aria-pressed={date === trayDate}
              title={frozen(date)
                ? "Frozen — cannot be a target"
                : `Add to ${WEEKDAY_LABELS[index]}`}
              onclick={() => (trayDate = date)}
            >
              <h2
                class="text-sm font-semibold"
                class:text-amber-200={date === trayDate && !frozen(date)}
              >
                {WEEKDAY_LABELS[index]}
              </h2>
              <p
                class="text-xs"
                class:text-amber-200={date === trayDate && !frozen(date)}
                class:text-[var(--color-muted)]={!(
                  date === trayDate && !frozen(date)
                )}
              >
                {day.toLocaleDateString(undefined, {
                  month: "short",
                  day: "numeric",
                })}
              </p>
            </button>
            <div class="px-3 py-2">
              {#if frozen(date)}<span
                  class="rounded bg-[var(--color-bg)] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[var(--color-muted)]"
                  >Frozen</span
                >{:else if date === today}<span
                  class="rounded bg-[var(--color-accent-soft)] px-1.5 py-0.5 text-[10px] uppercase tracking-wide"
                  >Today</span
                >{/if}
            </div>
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
                    <!-- Wraps to two lines rather than truncating to one
                         (WI #549). A day column is ~190px wide at 1440px, and
                         a single-line `truncate` was showing 81px of a 628px
                         title — 87% of the label gone, at a *desktop* width, on
                         the page whose whole job is telling you what you are
                         doing today. Two lines of a narrow column is cheap;
                         `title` keeps the full text one hover away. -->
                    <div class="flex items-start gap-1.5">
                      <span
                        class={`mt-0.5 shrink-0 rounded px-1 py-0.5 text-[9px] font-semibold uppercase ${kindClass(item.source_kind)}`}
                        >{kindLabel(item.source_kind)}</span
                      ><span
                        class="line-clamp-2 break-words text-xs font-medium"
                        class:line-through={item.completed_at !== null}
                        title={item.display}>{item.display}</span
                      >
                    </div>
                    {#if item.source_title !== item.display}<p
                        class="mt-1 line-clamp-2 break-words text-[10px] text-[var(--color-muted)]"
                        title={`Current source title: ${item.source_title}`}
                      >
                        Now: {item.source_title}
                      </p>{/if}
                  </div>
                  {#if !frozen(date)}<ConfirmButton
                      label={`Remove ${item.display}`}
                      class="rounded px-1 text-xs text-[var(--color-muted)] hover:bg-red-950 hover:text-red-200"
                      armedClass="rounded bg-red-900 px-1 text-xs text-red-100"
                      onconfirm={() => remove(item)}
                    >
                      ✕
                    </ConfirmButton>{/if}
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
  <!-- One search across both trays; the target day is chosen by clicking a day
       above, and echoed here so the answer is visible from wherever you are. -->
  <div class="flex flex-wrap items-center justify-between gap-3">
    <p class="text-xs text-[var(--color-muted)]">
      Adding to <span class="font-semibold text-amber-200"
        >{targetLabel}</span
      > — click a day to change it, or drag straight into one.
    </p>
    <input
      type="search"
      aria-label="Search proposals and cards"
      class="w-64 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="Search sources…"
      bind:value={sourceSearch}
    />
  </div>

  {@render tray(
    "Proposals",
    showProposals,
    () => (showProposals = !showProposals),
    proposalSources.length,
    proposalList,
  )}
  {@render tray(
    "Cards",
    showCards,
    () => (showCards = !showCards),
    cardSources.length,
    cardList,
  )}
</section>

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

<!-- Both trays are the same object: a collapsible panel with a count in its
     header, so the count is readable while collapsed. -->
{#snippet tray(
  title: string,
  open: boolean,
  toggle: () => void,
  count: number,
  body: Snippet,
)}
  <section
    class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
  >
    <button
      class="flex w-full items-center gap-2 px-4 py-3 text-left"
      aria-expanded={open}
      data-testid={`tray-toggle-${title.toLowerCase()}`}
      onclick={toggle}
    >
      <span class="text-xs text-[var(--color-muted)]">{open ? "▾" : "▸"}</span>
      <h2 class="font-semibold">{title}</h2>
      <span class="text-xs text-[var(--color-muted)]">{count}</span>
    </button>
    {#if open}
      <div class="px-4 pb-4">{@render body()}</div>
    {/if}
  </section>
{/snippet}

{#snippet proposalList()}
  <ul class="grid max-h-56 gap-2 overflow-auto sm:grid-cols-2 lg:grid-cols-3">
    {#each proposalSources as p (p.node_id)}
      <li
        class="flex cursor-grab items-start gap-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2 active:cursor-grabbing"
        draggable="true"
        ondragstart={(event) => dragSource(event, p.node_id)}
      >
        {#if p.project}<span class={`${chip.project} shrink-0`}>{p.project}</span
          >{/if}
        <span
          class="min-w-0 flex-1 line-clamp-2 break-words text-xs"
          title={p.title}>{p.title}</span
        >
        {@render quickView(p.node_id, p.title)}
        <button
          class="shrink-0 rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]"
          onclick={() => addSource(p.node_id, p.title)}>Add</button
        >
      </li>
    {:else}
      <li class="text-sm text-[var(--color-muted)]">
        No proposals in the queue{sourceSearch ? " match that search" : ""}.
      </li>
    {/each}
  </ul>
{/snippet}

{#snippet cardList()}
  <ul class="grid max-h-56 gap-2 overflow-auto sm:grid-cols-2 lg:grid-cols-4">
    {#each cardSources as card (card.node_id)}
      <li
        class="flex cursor-grab items-start gap-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2 active:cursor-grabbing"
        draggable="true"
        ondragstart={(event) => dragSource(event, card.node_id)}
      >
        <span
          class="min-w-0 flex-1 line-clamp-2 break-words text-xs"
          title={card.title}>{card.title}</span
        >
        {@render quickView(card.node_id, card.title)}
        <button
          class="shrink-0 rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]"
          onclick={() => addSource(card.node_id, card.title)}>Add</button
        >
      </li>
    {:else}
      <li class="text-sm text-[var(--color-muted)]">
        No active cards{sourceSearch ? " match that search" : ""}.
      </li>
    {/each}
  </ul>
{/snippet}

<!-- "I will forget what this title encompasses" — the slide-over answers that
     without leaving the page or losing the target day. -->
{#snippet quickView(nodeId: number, title: string)}
  <button
    class="shrink-0 rounded px-1 py-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-accent)]"
    title={`Preview ${title}`}
    aria-label={`Preview ${title}`}
    onclick={() => (previewNode = nodeId)}>👁</button
  >
{/snippet}

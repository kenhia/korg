<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type DailyPlanHistory,
    type DailyPlanItem,
    type HistoryPreset,
  } from "$lib/api";

  const presets: { value: HistoryPreset; label: string }[] = [
    { value: "week", label: "Week" },
    { value: "month", label: "Month" },
    { value: "90days", label: "90 days" },
    { value: "year", label: "Year" },
  ];
  let preset = $state<HistoryPreset>("month");
  let history = $state<DailyPlanHistory | null>(null);
  let allItems = $state<DailyPlanItem[]>([]);
  let sourceNodeId = $state("");
  let loading = $state(true);
  let error = $state<string | null>(null);
  const sourceOptions = $derived(
    [
      ...new Map(
        allItems.map((item) => [
          item.source_node_id,
          {
            id: item.source_node_id,
            label: item.source_title,
            kind: item.source_kind,
          },
        ]),
      ).values(),
    ].sort((a, b) => a.label.localeCompare(b.label)),
  );

  function kindLabel(kind: DailyPlanItem["source_kind"]): string {
    return kind === "workitem" ? "WI" : kind === "card" ? "Card" : "Topic";
  }
  async function load() {
    loading = true;
    error = null;
    try {
      const selected = sourceNodeId === "" ? undefined : Number(sourceNodeId);
      const [filtered, all] = await Promise.all([
        api.dailyPlanHistory(preset, selected),
        api.dailyPlanHistory(preset),
      ]);
      history = filtered;
      allItems = all.items;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      loading = false;
    }
  }
  async function toggle(item: DailyPlanItem, completed: boolean) {
    try {
      await api.setDailyPlanCompletion(item.node_id, completed);
      await load();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
  }
  function percent(rate: number): string {
    return new Intl.NumberFormat(undefined, {
      style: "percent",
      maximumFractionDigits: 0,
    }).format(rate);
  }
  function choosePreset(value: HistoryPreset) {
    preset = value;
    void load();
  }
  onMount(load);
</script>

<section class="space-y-5">
  <header>
    <p
      class="text-xs font-medium uppercase tracking-[0.18em] text-[var(--color-accent)]"
    >
      Daily plan
    </p>
    <h1 class="text-2xl font-semibold">History</h1>
    <p class="mt-1 text-sm text-[var(--color-muted)]">
      Completed and open items through yesterday.
    </p>
  </header>
  <div
    class="flex flex-wrap items-end justify-between gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
  >
    <div
      class="flex overflow-hidden rounded border border-[var(--color-border)]"
      aria-label="History range"
    >
      {#each presets as option (option.value)}<button
          class="px-3 py-1.5 text-sm"
          class:bg-[var(--color-accent-soft)]={preset === option.value}
          aria-pressed={preset === option.value}
          onclick={() => choosePreset(option.value)}>{option.label}</button
        >{/each}
    </div>
    <label class="text-xs text-[var(--color-muted)]"
      >Source<select
        class="ml-2 min-w-52 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm text-[var(--color-text)]"
        bind:value={sourceNodeId}
        onchange={() => void load()}
        ><option value="">All sources</option
        >{#each sourceOptions as source (source.id)}<option
            value={String(source.id)}
            >{kindLabel(source.kind)} · {source.label}</option
          >{/each}</select
      ></label
    >
  </div>
  {#if error}<p
      class="rounded bg-red-950 px-3 py-2 text-sm text-red-200"
      role="alert"
    >
      {error}
    </p>{/if}
  {#if loading || !history}<p class="text-[var(--color-muted)]">
      Loading history…
    </p>{:else}
    <div class="grid gap-3 sm:grid-cols-3" data-testid="history-stats">
      <div
        class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
      >
        <p class="text-xs uppercase tracking-wide text-[var(--color-muted)]">
          Completion
        </p>
        <p class="mt-1 text-3xl font-semibold text-[var(--color-accent)]">
          {percent(history.completion_rate)}
        </p>
      </div>
      <div
        class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
      >
        <p class="text-xs uppercase tracking-wide text-[var(--color-muted)]">
          Completed
        </p>
        <p class="mt-1 text-3xl font-semibold">
          {history.completed}<span
            class="text-base font-normal text-[var(--color-muted)]"
          >
            / {history.total}</span
          >
        </p>
      </div>
      <div
        class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
      >
        <p class="text-xs uppercase tracking-wide text-[var(--color-muted)]">
          Range
        </p>
        <p class="mt-2 text-sm">{history.from} → {history.to}</p>
      </div>
    </div>
    <ol
      class="divide-y divide-[var(--color-border)] overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
      data-testid="history-list"
    >
      {#each history.items as item (item.node_id)}<li
          class="flex items-start gap-3 p-3"
        >
          <input
            type="checkbox"
            class="mt-1 accent-sky-500"
            checked={item.completed_at !== null}
            aria-label={`Complete ${item.display}`}
            onchange={(event) => toggle(item, event.currentTarget.checked)}
          /><time
            class="w-24 shrink-0 text-xs text-[var(--color-muted)]"
            datetime={item.plan_date}
            >{new Date(`${item.plan_date}T12:00:00`).toLocaleDateString(
              undefined,
              { month: "short", day: "numeric", year: "numeric" },
            )}</time
          ><span
            class="rounded bg-[var(--color-bg)] px-1.5 py-0.5 text-[10px] uppercase text-[var(--color-muted)]"
            >{kindLabel(item.source_kind)}</span
          >
          <div class="min-w-0 flex-1">
            <p class="text-sm" class:line-through={item.completed_at !== null}>
              {item.display}
            </p>
            {#if item.source_title !== item.display}<p
                class="truncate text-xs text-[var(--color-muted)]"
              >
                Now: {item.source_title}
              </p>{/if}
          </div>
          <span
            class="text-xs"
            class:text-emerald-300={item.completed_at !== null}
            class:text-amber-300={item.completed_at === null}
            >{item.completed_at === null ? "Open" : "Complete"}</span
          >
        </li>{:else}<li
          class="p-8 text-center text-sm text-[var(--color-muted)]"
        >
          No planned items in this range.
        </li>{/each}
    </ol>
  {/if}
</section>

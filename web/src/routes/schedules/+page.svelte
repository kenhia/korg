<script lang="ts">
  import { api, type ScheduleList, type ScheduleRow } from "$lib/api";
  import NodePreview from "$lib/components/NodePreview.svelte";
  import { notify } from "$lib/toast.svelte";
  import { ID_CLASS, chip, scheduleDueLabel, scheduleDuePill } from "$lib/domain";

  // Work that a date makes appear (#581). korg runs no scheduler: `due` is
  // computed on every read, and Materialise is the explicit write that stands
  // in for the daily tick this feature deliberately does not have.
  let list = $state<ScheduleList | null>(null);
  let dueOnly = $state(false);
  let error = $state<string | null>(null);
  let busy = $state<number | null>(null);
  let previewNode = $state<number | null>(null);

  async function load() {
    try {
      list = await api.schedules({ due_only: dueOnly || undefined });
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function materialize(s: ScheduleRow, force: boolean) {
    busy = s.node_id;
    try {
      const out = await api.materializeSchedule(s.node_id, force);
      notify(`#${out.work_item.wi_number} — ${out.work_item.title}`);
      await load();
    } catch (e) {
      // The refusals are worth reading verbatim: they say what would lift them
      // (or, for an outstanding item, that nothing will).
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = null;
    }
  }

  async function setStatus(s: ScheduleRow, status: "active" | "paused") {
    try {
      await api.updateSchedule(s.node_id, { status });
      await load();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  /** `2026-10-08` — the date is the whole point; the time of day never is. */
  function day(ts: string): string {
    return ts.slice(0, 10);
  }

  load();
</script>

<svelte:head><title>korg — schedules</title></svelte:head>

<h1 class="mb-1 text-xl font-semibold">Schedules</h1>
<p class="mb-4 text-sm text-[var(--color-muted)]">
  Work a date makes appear — recurring maintenance and one-shots. Nothing runs on its
  own: <span class="font-medium">due</span> is computed when you look, and Materialise is
  the write. A schedule whose last item is still open reads
  <span class="font-medium">in flight</span> and stays quiet — that item is the surface.
</p>

<label class="mb-4 flex items-center gap-2 text-sm">
  <input
    type="checkbox"
    bind:checked={dueOnly}
    onchange={load}
    class="accent-[var(--color-accent)]"
  />
  Only what is due now
</label>

{#if error}
  <p class="mb-4 rounded border border-red-700 bg-red-900/30 px-3 py-2 text-sm text-red-300">
    {error}
  </p>
{/if}

{#if list}
  {#if list.items.length === 0}
    <p class="text-sm text-[var(--color-muted)]">
      {#if dueOnly && list.omitted.not_due > 0}
        Nothing due. {list.omitted.not_due}
        {list.omitted.not_due === 1 ? "schedule is" : "schedules are"} waiting.
      {:else}
        No schedules yet — an agent creates one with <code>create_schedule</code>.
      {/if}
    </p>
  {/if}

  <ul class="space-y-2">
    {#each list.items as s (s.node_id)}
      <li class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-3">
        <div class="flex flex-wrap items-center gap-2">
          <span class={`shrink-0 ${ID_CLASS}`}>#{s.node_id}</span>
          <span class={scheduleDuePill(s.due, s.outstanding)}>
            {scheduleDueLabel(s.due, s.outstanding)}
          </span>
          <!-- The rendered title, not the template: this is what Materialise
               would actually create right now. -->
          <span class="min-w-0 flex-1 truncate text-sm font-medium">{s.preview_title}</span>
          {#if s.project}
            <span class={chip.project}>{s.project}</span>
          {/if}
        </div>

        <div class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[var(--color-muted)]">
          <span>{s.cadence}{s.cadence === "once" ? "" : ` · from last ${s.anchor_mode}`}</span>
          <span>
            {s.due ? "due since" : "due"}
            <span class="font-mono tabular-nums">{day(s.due_at)}</span>
          </span>
          {#if s.last_wi_number}
            <button
              class="text-[var(--color-accent)] hover:underline"
              title={`Preview #${s.last_wi_number}`}
              onclick={() => (previewNode = s.last_wi_number)}
            >
              last: #{s.last_wi_number}
            </button>
          {/if}
          {#if s.materialized_count > 0}
            <span>
              {s.materialized_count} run{s.materialized_count === 1 ? "" : "s"}
            </span>
          {/if}
          {#if s.status !== "active"}
            <span class="uppercase tracking-wide">{s.status}</span>
          {/if}
        </div>

        <div class="mt-3 flex flex-wrap gap-2">
          <button
            class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)] disabled:opacity-40"
            disabled={busy === s.node_id || s.outstanding || s.status !== "active"}
            onclick={() => materialize(s, !s.due)}
          >
            {#if s.due}
              Materialise
            {:else}
              Materialise early
            {/if}
          </button>
          {#if s.status === "active"}
            <button
              class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]"
              onclick={() => setStatus(s, "paused")}
            >
              Pause
            </button>
          {:else if s.status === "paused"}
            <button
              class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]"
              onclick={() => setStatus(s, "active")}
            >
              Resume
            </button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  {#if list.omitted.done > 0 || list.omitted.archived > 0 || (dueOnly && list.omitted.not_due > 0)}
    <p class="mt-3 text-xs text-[var(--color-muted)]">
      Hidden by the defaults:
      {#if dueOnly && list.omitted.not_due > 0}{list.omitted.not_due} not yet due{/if}
      {#if list.omitted.done > 0}{dueOnly && list.omitted.not_due > 0 ? ", " : ""}{list.omitted
          .done} finished{/if}
      {#if list.omitted.archived > 0}{list.omitted.done > 0 || (dueOnly && list.omitted.not_due > 0)
          ? ", "
          : ""}{list.omitted.archived} archived{/if}.
    </p>
  {/if}
{:else if !error}
  <p class="text-sm text-[var(--color-muted)]">loading…</p>
{/if}

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

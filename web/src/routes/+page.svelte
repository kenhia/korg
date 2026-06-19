<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Slot, type Link } from "$lib/api";
  import {
    startOfWeek,
    addDays,
    isoDate,
    weekDays,
    WEEKDAY_LABELS,
    prettyDuration,
  } from "$lib/dates";

  let weekStart = $state(startOfWeek(new Date()));
  let slots = $state<Slot[]>([]);
  let links = $state<Link[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  const days = $derived(weekDays(weekStart));

  function slotsFor(day: Date): Slot[] {
    const key = isoDate(day);
    return slots
      .filter((s) => s.slot_date === key)
      .sort((a, b) => a.position - b.position);
  }

  async function load() {
    loading = true;
    error = null;
    try {
      const from = isoDate(weekStart);
      const to = isoDate(addDays(weekStart, 6));
      [slots, links] = await Promise.all([api.slots(from, to), api.links()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function generate() {
    await api.generateSlots(isoDate(weekStart), 7);
    await load();
  }

  function shiftWeek(delta: number) {
    weekStart = addDays(weekStart, delta * 7);
    load();
  }

  async function saveGoal(slot: Slot, value: string) {
    const goal = value.trim() === "" ? null : value.trim();
    await api.setSlotGoal(slot.node_id, goal);
    slot.goal = goal;
  }

  const toRead = $derived(
    links.filter((l) => l.disposition === "Unread" || l.disposition === "Revisit"),
  );

  onMount(load);
</script>

<section class="space-y-4">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">This week</h1>
    <div class="flex items-center gap-2 text-sm">
      <button class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]" onclick={() => shiftWeek(-1)}>← Prev</button>
      <button class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]" onclick={() => (weekStart = startOfWeek(new Date())) && load()}>Today</button>
      <button class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]" onclick={() => shiftWeek(1)}>Next →</button>
    </div>
  </div>

  {#if error}
    <p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else if slots.length === 0}
    <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-6 text-center">
      <p class="mb-3 text-[var(--color-muted)]">No timeboxes for this week yet.</p>
      <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]" onclick={generate}>
        Generate from weekly template
      </button>
    </div>
  {:else}
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-7">
      {#each days as day, i (isoDate(day))}
        <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
          <div class="mb-2 text-xs font-medium text-[var(--color-muted)]">
            {WEEKDAY_LABELS[i]} {day.getDate()}
          </div>
          <div class="space-y-2">
            {#each slotsFor(day) as slot (slot.node_id)}
              <div class="rounded bg-[var(--color-surface-hi)] p-2">
                <div class="mb-1 text-xs text-[var(--color-accent)]">
                  {slot.label ?? prettyDuration(slot.duration_minutes)}
                </div>
                <input
                  class="w-full rounded bg-transparent text-sm outline-none placeholder:text-[var(--color-muted)]"
                  placeholder="goal…"
                  value={slot.goal ?? ""}
                  onblur={(e) => saveGoal(slot, e.currentTarget.value)}
                />
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <h2 class="pt-4 text-lg font-semibold">Reading list</h2>
  {#if toRead.length === 0}
    <p class="text-sm text-[var(--color-muted)]">Nothing to read. Nice.</p>
  {:else}
    <ul class="divide-y divide-[var(--color-border)] rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
      {#each toRead as link (link.node_id)}
        <li class="flex items-center justify-between gap-3 px-3 py-2">
          <a
            href={link.url}
            target="_blank"
            rel="noopener noreferrer"
            class="truncate text-sm text-[var(--color-accent)] hover:underline"
          >
            {link.title ?? link.url}
          </a>
          {#if link.disposition === "Revisit"}
            <span class="shrink-0 rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">revisit</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <p class="text-sm"><a class="text-[var(--color-muted)] hover:underline" href="/reading-list">Manage reading list →</a></p>
</section>

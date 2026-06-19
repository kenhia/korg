<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Project, type WorkItem } from "$lib/api";

  let projects = $state<Project[]>([]);
  let current = $state<string | null>(null);
  let items = $state<WorkItem[]>([]);
  let selected = $state<number | null>(null); // wi_number
  let loading = $state(true);
  let error = $state<string | null>(null);

  const selectedItem = $derived(items.find((i) => i.wi_number === selected) ?? null);

  async function loadProjects() {
    const [ps, recent] = await Promise.all([api.projects(), api.recentProject()]);
    projects = ps;
    current = recent.project ?? ps[0]?.name ?? null;
  }

  async function loadItems() {
    if (!current) {
      items = [];
      return;
    }
    items = await api.workItems(current);
    selected = items[0]?.wi_number ?? null;
  }

  async function load() {
    loading = true;
    error = null;
    try {
      await loadProjects();
      await loadItems();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function pickProject(name: string) {
    current = name;
    await loadItems();
  }

  function move(delta: number) {
    if (items.length === 0) return;
    const idx = items.findIndex((i) => i.wi_number === selected);
    const next = Math.min(items.length - 1, Math.max(0, (idx < 0 ? 0 : idx) + delta));
    selected = items[next].wi_number;
  }

  // Keyboard nav: arrows move selection, Esc clears (back).
  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Escape") {
      selected = null;
    }
  }

  onMount(load);
</script>

<svelte:window onkeydown={onKey} />

<section class="space-y-4">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">Work items</h1>
    <select
      class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none"
      value={current ?? ""}
      onchange={(e) => pickProject(e.currentTarget.value)}
    >
      {#each projects as p (p.id)}
        <option value={p.name}>{p.name}</option>
      {/each}
    </select>
  </div>
  <p class="text-xs text-[var(--color-muted)]">↑/↓ select · Esc deselect</p>

  {#if error}
    <p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else}
    <div class="grid grid-cols-1 gap-4 md:grid-cols-[20rem_1fr]">
      <ul class="max-h-[70vh] divide-y divide-[var(--color-border)] overflow-auto rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
        {#each items as item (item.wi_number)}
          <li>
            <button
              class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
              class:bg-[var(--color-surface-hi)]={item.wi_number === selected}
              onclick={() => (selected = item.wi_number)}
            >
              <span class="font-mono text-xs text-[var(--color-muted)]">#{item.wi_number}</span>
              <span class="truncate">{item.title}</span>
            </button>
          </li>
        {:else}
          <li class="px-3 py-2 text-sm text-[var(--color-muted)]">No work items.</li>
        {/each}
      </ul>

      <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
        {#if selectedItem}
          <div class="mb-2 flex items-center gap-2">
            <span class="font-mono text-sm text-[var(--color-muted)]">#{selectedItem.wi_number}</span>
            <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{selectedItem.wi_type}</span>
            <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{selectedItem.wi_status}</span>
            <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{selectedItem.wi_tshirt}</span>
          </div>
          <h2 class="mb-2 text-lg font-semibold">{selectedItem.title}</h2>
          <p class="whitespace-pre-wrap text-sm text-[var(--color-text)]">{selectedItem.content}</p>
          {#if selectedItem.details}
            <pre class="mt-3 max-h-[40vh] overflow-auto whitespace-pre-wrap rounded bg-[var(--color-bg)] p-3 text-xs">{selectedItem.details}</pre>
          {/if}
          {#if selectedItem.tags.length > 0}
            <div class="mt-3 flex flex-wrap gap-1">
              {#each selectedItem.tags as tag (tag)}
                <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">#{tag}</span>
              {/each}
            </div>
          {/if}
        {:else}
          <p class="text-sm text-[var(--color-muted)]">Select a work item.</p>
        {/if}
      </div>
    </div>
  {/if}
</section>

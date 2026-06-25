<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Card, type WorkItem, type Link, type Project } from "$lib/api";

  let cards = $state<Card[]>([]);
  let workItems = $state<WorkItem[]>([]);
  let links = $state<Link[]>([]);
  let projects = $state<Project[]>([]);
  let loading = $state(true);

  // Filters
  const ALL = "\u0000all";
  let cardProject = $state(ALL);
  let wiProject = $state(ALL);
  let cardText = $state("");
  let wiText = $state("");
  let linkText = $state("");

  function matches(text: string, q: string): boolean {
    return q.trim() === "" || text.toLowerCase().includes(q.trim().toLowerCase());
  }

  const shownCards = $derived(
    cards.filter(
      (c) => (cardProject === ALL || c.project === cardProject) && matches(c.title, cardText),
    ),
  );
  const shownWorkItems = $derived(
    workItems.filter(
      (w) => (wiProject === ALL || w.project === wiProject) && matches(w.title, wiText),
    ),
  );
  const shownLinks = $derived(links.filter((l) => matches(l.title ?? l.url, linkText)));

  // Selection — node_ids are globally unique across kinds, so one set works.
  let selectedIds = $state<number[]>([]);
  const selectedCount = $derived(selectedIds.length);
  function isSelected(id: number): boolean {
    return selectedIds.includes(id);
  }
  function toggle(id: number): void {
    selectedIds = isSelected(id) ? selectedIds.filter((x) => x !== id) : [...selectedIds, id];
  }

  let linking = $state(false);
  let linkedMsg = $state("");

  async function linkSelected(): Promise<void> {
    if (selectedIds.length < 2) return;
    linking = true;
    linkedMsg = "";
    try {
      const ids = selectedIds;
      // Clique: relate every selected pair. relate() is idempotent + symmetric.
      for (let i = 0; i < ids.length; i++) {
        for (let j = i + 1; j < ids.length; j++) {
          await api.relate(ids[i], ids[j], "related-to");
        }
      }
      const n = ids.length;
      const edges = (n * (n - 1)) / 2;
      linkedMsg = `Linked ${n} items (${edges} relationship${edges === 1 ? "" : "s"}).`;
      selectedIds = [];
    } catch (e) {
      linkedMsg = `Failed to link: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      linking = false;
    }
  }

  async function load() {
    loading = true;
    [cards, workItems, links, projects] = await Promise.all([
      api.cards().catch(() => []),
      api.workItems().catch(() => []),
      api.links().catch(() => []),
      api.projects().catch(() => []),
    ]);
    loading = false;
  }

  onMount(load);
</script>

<div class="mb-4">
  <h1 class="text-xl font-semibold tracking-tight">Link Up</h1>
  <p class="text-sm text-[var(--color-muted)]">
    Select items across the lists and relate them to each other.
  </p>
</div>

<div class="grid grid-cols-1 gap-4 md:grid-cols-3">
  <!-- Cards -->
  <section
    class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
    aria-label="Cards"
  >
    <h2 class="mb-2 text-sm font-semibold">Cards</h2>
    <div class="mb-2 flex gap-2">
      <select
        class="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-xs"
        aria-label="Filter cards by project"
        bind:value={cardProject}
      >
        <option value={ALL}>All projects</option>
        {#each projects as p (p.id)}
          <option value={p.name}>{p.name}</option>
        {/each}
      </select>
      <input
        class="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-xs"
        placeholder="filter cards…"
        aria-label="Filter cards by text"
        bind:value={cardText}
      />
    </div>
    <ul class="space-y-1">
      {#each shownCards as c (c.node_id)}
        <li>
          <label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1 text-sm hover:bg-[var(--color-surface-hi)]">
            <input type="checkbox" checked={isSelected(c.node_id)} onchange={() => toggle(c.node_id)} />
            <span>{c.title}</span>
          </label>
        </li>
      {/each}
    </ul>
  </section>

  <!-- Work Items -->
  <section
    class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
    aria-label="Work Items"
  >
    <h2 class="mb-2 text-sm font-semibold">Work Items</h2>
    <div class="mb-2 flex gap-2">
      <select
        class="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-xs"
        aria-label="Filter work items by project"
        bind:value={wiProject}
      >
        <option value={ALL}>All projects</option>
        {#each projects as p (p.id)}
          <option value={p.name}>{p.name}</option>
        {/each}
      </select>
      <input
        class="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-xs"
        placeholder="filter work items…"
        aria-label="Filter work items by text"
        bind:value={wiText}
      />
    </div>
    <ul class="space-y-1">
      {#each shownWorkItems as w (w.node_id)}
        <li>
          <label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1 text-sm hover:bg-[var(--color-surface-hi)]">
            <input type="checkbox" checked={isSelected(w.node_id)} onchange={() => toggle(w.node_id)} />
            <span>{w.title}</span>
          </label>
        </li>
      {/each}
    </ul>
  </section>

  <!-- Reading List -->
  <section
    class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
    aria-label="Reading List"
  >
    <h2 class="mb-2 text-sm font-semibold">Reading List</h2>
    <div class="mb-2">
      <input
        class="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-xs"
        placeholder="filter reading list…"
        aria-label="Filter reading list by text"
        bind:value={linkText}
      />
    </div>
    <ul class="space-y-1">
      {#each shownLinks as l (l.node_id)}
        <li>
          <label class="flex cursor-pointer items-center gap-2 rounded px-2 py-1 text-sm hover:bg-[var(--color-surface-hi)]">
            <input type="checkbox" checked={isSelected(l.node_id)} onchange={() => toggle(l.node_id)} />
            <span>{l.title ?? l.url}</span>
          </label>
        </li>
      {/each}
    </ul>
  </section>
</div>

<div
  class="sticky bottom-0 mt-4 flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-3"
>
  <span class="text-sm text-[var(--color-muted)]">
    {#if linkedMsg}
      <span role="status">{linkedMsg}</span>
    {:else}
      {selectedCount} selected
    {/if}
  </span>
  <button
    class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm font-medium hover:bg-[var(--color-accent)] disabled:cursor-not-allowed disabled:opacity-50"
    disabled={selectedCount < 2 || linking}
    onclick={linkSelected}
  >
    Link {selectedCount} items
  </button>
</div>


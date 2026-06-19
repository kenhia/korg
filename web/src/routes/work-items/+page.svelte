<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    WI_TYPES,
    WI_STATUSES,
    TSHIRTS,
    type Project,
    type WorkItem,
  } from "$lib/api";

  const ALL = "\u0000all"; // sentinel for the "All" view

  let projects = $state<Project[]>([]);
  let current = $state<string>(ALL);
  let items = $state<WorkItem[]>([]);
  let selected = $state<number | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let newProject = $state("");
  let showNewWi = $state(false);
  let wiTitle = $state("");
  let wiContent = $state("");
  let wiType = $state<string>("task");
  let wiStatus = $state<string>("open");
  let wiTshirt = $state<string>("Unknown");

  const selectedItem = $derived(items.find((i) => i.wi_number === selected) ?? null);
  const currentProject = $derived(projects.find((p) => p.name === current) ?? null);

  async function loadProjects() {
    const [ps, recent] = await Promise.all([api.projects(), api.recentProject()]);
    projects = ps;
    if (current === ALL && recent.project) current = recent.project;
  }

  async function loadItems() {
    items = await api.workItems(current === ALL ? undefined : current);
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

  async function pick(name: string) {
    current = name;
    selected = null;
    await loadItems();
  }

  async function addProject() {
    const name = newProject.trim();
    if (name === "") return;
    await api.createProject(name);
    newProject = "";
    await loadProjects();
    await pick(name);
  }

  async function addWorkItem() {
    if (wiTitle.trim() === "") return;
    await api.createWorkItem({
      title: wiTitle.trim(),
      content: wiContent.trim(),
      wi_type: wiType,
      wi_status: wiStatus,
      wi_tshirt: wiTshirt,
      project_id: currentProject?.id,
    });
    wiTitle = "";
    wiContent = "";
    showNewWi = false;
    await loadItems();
  }

  function move(delta: number) {
    if (items.length === 0) return;
    const idx = items.findIndex((i) => i.wi_number === selected);
    const next = Math.min(items.length - 1, Math.max(0, (idx < 0 ? 0 : idx) + delta));
    selected = items[next].wi_number;
  }

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
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
    <button
      class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40"
      disabled={current === ALL}
      title={current === ALL ? "Pick a project first" : "New work item"}
      onclick={() => (showNewWi = !showNewWi)}
    >
      + Work item
    </button>
  </div>
  <p class="text-xs text-[var(--color-muted)]">↑/↓ select · Esc deselect</p>

  {#if error}
    <p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else}
    <div class="grid grid-cols-1 gap-4 md:grid-cols-[14rem_1fr]">
      <aside class="space-y-2">
        <nav class="overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
          <button
            class="block w-full px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
            class:bg-[var(--color-surface-hi)]={current === ALL}
            onclick={() => pick(ALL)}
          >
            All projects
          </button>
          {#each projects as p (p.id)}
            <button
              class="block w-full px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
              class:bg-[var(--color-surface-hi)]={current === p.name}
              class:text-[var(--color-accent)]={current === p.name}
              onclick={() => pick(p.name)}
            >
              {p.name}
            </button>
          {/each}
        </nav>
        <div class="flex gap-1">
          <input
            class="min-w-0 flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none"
            placeholder="new project…"
            bind:value={newProject}
            onkeydown={(e) => e.key === "Enter" && addProject()}
          />
          <button class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm hover:bg-[var(--color-accent-soft)]" onclick={addProject}>+</button>
        </div>
      </aside>

      <div class="space-y-3">
        {#if showNewWi}
          <div class="space-y-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
            <input
              class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
              placeholder="Title"
              bind:value={wiTitle}
            />
            <textarea
              class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
              rows="3"
              placeholder="Content"
              bind:value={wiContent}
            ></textarea>
            <div class="flex flex-wrap gap-2 text-sm">
              <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={wiType}>
                {#each WI_TYPES as t (t)}<option value={t}>{t}</option>{/each}
              </select>
              <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={wiStatus}>
                {#each WI_STATUSES as st (st)}<option value={st}>{st}</option>{/each}
              </select>
              <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={wiTshirt}>
                {#each TSHIRTS as ts (ts)}<option value={ts}>{ts}</option>{/each}
              </select>
              <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1 hover:bg-[var(--color-accent)]" onclick={addWorkItem}>Create</button>
            </div>
          </div>
        {/if}

        <div class="grid grid-cols-1 gap-4 lg:grid-cols-[18rem_1fr]">
          <ul class="max-h-[64vh] divide-y divide-[var(--color-border)] overflow-auto rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
            {#each items as item (item.wi_number)}
              <li>
                <button
                  class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
                  class:bg-[var(--color-surface-hi)]={item.wi_number === selected}
                  onclick={() => (selected = item.wi_number)}
                >
                  <span class="font-mono text-xs text-[var(--color-muted)]">#{item.wi_number}</span>
                  <span class="truncate">{item.title}</span>
                  {#if current === ALL && item.project}
                    <span class="ml-auto shrink-0 rounded bg-[var(--color-bg)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">{item.project}</span>
                  {/if}
                </button>
              </li>
            {:else}
              <li class="px-3 py-3 text-sm text-[var(--color-muted)]">No work items here.</li>
            {/each}
          </ul>

          <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
            {#if selectedItem}
              <div class="mb-2 flex flex-wrap items-center gap-2">
                <span class="font-mono text-sm text-[var(--color-muted)]">#{selectedItem.wi_number}</span>
                <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{selectedItem.wi_type}</span>
                <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{selectedItem.wi_status}</span>
                <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{selectedItem.wi_tshirt}</span>
                {#if selectedItem.project}
                  <span class="rounded bg-[var(--color-accent-soft)] px-1.5 py-0.5 text-xs">{selectedItem.project}</span>
                {/if}
              </div>
              <h2 class="mb-2 text-lg font-semibold">{selectedItem.title}</h2>
              <p class="whitespace-pre-wrap text-sm">{selectedItem.content}</p>
              {#if selectedItem.details}
                <pre class="mt-3 max-h-[36vh] overflow-auto whitespace-pre-wrap rounded bg-[var(--color-bg)] p-3 text-xs">{selectedItem.details}</pre>
              {/if}
            {:else}
              <p class="text-sm text-[var(--color-muted)]">Select a work item.</p>
            {/if}
          </div>
        </div>
      </div>
    </div>
  {/if}
</section>

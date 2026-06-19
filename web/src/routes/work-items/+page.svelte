<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    WI_TYPES,
    WI_STATUSES,
    TSHIRTS,
    type Project,
    type WorkItem,
    type Neighbor,
  } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";
  import MultiSelectFilter from "$lib/components/MultiSelectFilter.svelte";

  const ALL = "\u0000all";
  const UNASSIGNED = "Unassigned";

  let projects = $state<Project[]>([]);
  let current = $state<string>(ALL);
  let items = $state<WorkItem[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // master/detail
  let detail = $state<WorkItem | null>(null);
  let cursor = $state<number | null>(null); // highlighted wi_number in the list
  let related = $state<Neighbor[]>([]);

  // forms
  let newProject = $state("");
  let showNewWi = $state(false);
  let wiTitle = $state("");
  let wiContent = $state("");
  let wiType = $state("task");
  let wiStatus = $state("open");
  let wiTshirt = $state("Unknown");

  // filters
  let search = $state("");
  let showArchived = $state(false);
  let fTypes = $state<Set<string>>(new Set());
  let fStatuses = $state<Set<string>>(new Set());
  let fSizes = $state<Set<string>>(new Set());
  let fAreas = $state<Set<string>>(new Set());
  let fSprints = $state<Set<string>>(new Set());

  const currentProject = $derived(projects.find((p) => p.name === current) ?? null);

  function distinct(vals: (string | null)[]): string[] {
    const s = new Set<string>();
    for (const v of vals) if (v) s.add(v);
    return [...s];
  }
  const typeOptions = $derived(
    distinct(items.map((i) => i.wi_type)).sort(
      (a, b) => WI_TYPES.indexOf(a as never) - WI_TYPES.indexOf(b as never),
    ),
  );
  const statusOptions = $derived(distinct(items.map((i) => i.wi_status)));
  const sizeOptions = $derived(
    distinct(items.map((i) => i.wi_tshirt)).sort(
      (a, b) => TSHIRTS.indexOf(a as never) - TSHIRTS.indexOf(b as never),
    ),
  );
  const areaOptions = $derived(distinct(items.map((i) => i.area)));
  const sprintOptions = $derived(() => {
    const out = distinct(items.map((i) => i.sprint)).sort();
    if (items.some((i) => i.sprint == null)) out.push(UNASSIGNED);
    return out;
  });

  function resetFilters() {
    fTypes = new Set(typeOptions);
    fStatuses = new Set(statusOptions.filter((s) => s !== "closed"));
    fSizes = new Set(sizeOptions);
    fAreas = new Set(areaOptions);
    fSprints = new Set(sprintOptions());
    showArchived = false;
  }

  const filtered = $derived(
    items.filter((it) => {
      if (!showArchived && it.archived) return false;
      if (!fTypes.has(it.wi_type)) return false;
      if (!fStatuses.has(it.wi_status)) return false;
      if (!fSizes.has(it.wi_tshirt)) return false;
      if (it.area ? !fAreas.has(it.area) : false) return false;
      if (it.sprint ? !fSprints.has(it.sprint) : !fSprints.has(UNASSIGNED)) return false;
      if (search.trim() !== "") {
        const q = search.toLowerCase();
        if (!(it.title.toLowerCase().includes(q) || it.content.toLowerCase().includes(q)))
          return false;
      }
      return true;
    }),
  );

  async function loadProjects() {
    const [ps, recent] = await Promise.all([api.projects(), api.recentProject()]);
    projects = ps;
    if (current === ALL && recent.project) current = recent.project;
  }

  async function loadItems() {
    items = await api.workItems(current === ALL ? undefined : current);
    resetFilters();
    cursor = filtered[0]?.wi_number ?? null;
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
    detail = null;
    await loadItems();
  }

  async function open(item: WorkItem) {
    detail = item;
    cursor = item.wi_number;
    related = [];
    try {
      related = await api.neighbors(item.node_id);
    } catch {
      related = [];
    }
  }

  function relatedLabel(n: Neighbor): string {
    const wi = items.find((i) => i.node_id === n.node_id);
    if (wi) return `#${wi.wi_number} ${wi.title}`;
    return `${n.kind} #${n.node_id}`;
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

  function moveCursor(delta: number) {
    const list = filtered;
    if (list.length === 0) return;
    const idx = list.findIndex((i) => i.wi_number === cursor);
    const next = Math.min(list.length - 1, Math.max(0, (idx < 0 ? 0 : idx) + delta));
    cursor = list[next].wi_number;
  }

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (detail) {
      if (e.key === "Escape") detail = null;
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      moveCursor(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      moveCursor(-1);
    } else if (e.key === "Enter" && cursor != null) {
      const it = filtered.find((i) => i.wi_number === cursor);
      if (it) open(it);
    }
  }

  const modified = $derived({
    types: fTypes.size !== typeOptions.length,
    statuses: fStatuses.size !== statusOptions.filter((s) => s !== "closed").length,
    sizes: fSizes.size !== sizeOptions.length,
    areas: fAreas.size !== areaOptions.length,
    sprints: fSprints.size !== sprintOptions().length,
  });

  onMount(load);
</script>

<svelte:window onkeydown={onKey} />

<section class="space-y-4">
  <h1 class="text-xl font-semibold">Work items</h1>

  {#if error}
    <p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else}
    <div class="grid grid-cols-1 gap-4 md:grid-cols-[13rem_1fr]">
      <!-- project rail -->
      <aside class="space-y-2">
        <nav class="overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
          <button
            class="block w-full px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
            class:bg-[var(--color-surface-hi)]={current === ALL}
            onclick={() => pick(ALL)}>All projects</button>
          {#each projects as p (p.id)}
            <button
              class="block w-full px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
              class:bg-[var(--color-surface-hi)]={current === p.name}
              class:text-[var(--color-accent)]={current === p.name}
              onclick={() => pick(p.name)}>{p.name}</button>
          {/each}
        </nav>
        <div class="flex gap-1">
          <input
            class="min-w-0 flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none"
            placeholder="new project…"
            bind:value={newProject}
            onkeydown={(e) => e.key === "Enter" && addProject()} />
          <button class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm hover:bg-[var(--color-accent-soft)]" onclick={addProject}>+</button>
        </div>
      </aside>

      <!-- main: list or detail -->
      <div class="min-w-0">
        {#if detail}
          {@render detailView(detail)}
        {:else}
          {@render listView()}
        {/if}
      </div>
    </div>
  {/if}
</section>

{#snippet badge(text: string)}
  <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{text}</span>
{/snippet}

{#snippet listView()}
  <div class="space-y-3">
    <!-- toolbar -->
    <div class="flex flex-wrap items-center gap-2">
      <input
        class="w-40 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none"
        placeholder="search…"
        bind:value={search} />
      <MultiSelectFilter label="areas" options={areaOptions} selected={fAreas} modified={modified.areas} onchange={(v) => (fAreas = v)} />
      <MultiSelectFilter label="types" options={typeOptions} selected={fTypes} modified={modified.types} onchange={(v) => (fTypes = v)} />
      <MultiSelectFilter label="statuses" options={statusOptions} selected={fStatuses} modified={modified.statuses} onchange={(v) => (fStatuses = v)} />
      <MultiSelectFilter label="sizes" options={sizeOptions} selected={fSizes} modified={modified.sizes} onchange={(v) => (fSizes = v)} />
      <MultiSelectFilter label="sprints" options={sprintOptions()} selected={fSprints} modified={modified.sprints} onchange={(v) => (fSprints = v)} />
      <label class="flex items-center gap-1 text-xs text-[var(--color-muted)]">
        <input type="checkbox" bind:checked={showArchived} /> archived
      </label>
      <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]" onclick={resetFilters}>Clear</button>
      <div class="ml-auto">
        <button
          class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40"
          disabled={current === ALL}
          title={current === ALL ? "Pick a project first" : "New work item"}
          onclick={() => (showNewWi = !showNewWi)}>+ New Work Item</button>
      </div>
    </div>

    {#if showNewWi}
      <div class="space-y-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
        <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="Title" bind:value={wiTitle} />
        <textarea class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" rows="3" placeholder="Content (markdown)" bind:value={wiContent}></textarea>
        <div class="flex flex-wrap gap-2 text-sm">
          <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={wiType}>{#each WI_TYPES as t (t)}<option value={t}>{t}</option>{/each}</select>
          <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={wiStatus}>{#each WI_STATUSES as s (s)}<option value={s}>{s}</option>{/each}</select>
          <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={wiTshirt}>{#each TSHIRTS as ts (ts)}<option value={ts}>{ts}</option>{/each}</select>
          <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1 hover:bg-[var(--color-accent)]" onclick={addWorkItem}>Create</button>
        </div>
      </div>
    {/if}

    <p class="text-xs text-[var(--color-muted)]">{filtered.length} items · ↑/↓ select · Enter open</p>

    <div class="overflow-auto rounded border border-[var(--color-border)]">
      <table class="w-full text-sm">
        <thead class="sticky top-0 bg-[var(--color-surface)] text-left text-xs text-[var(--color-muted)]">
          <tr>
            <th class="px-3 py-2">ID</th>
            <th class="px-3 py-2">Area</th>
            <th class="px-3 py-2">Type</th>
            <th class="px-3 py-2">Status</th>
            <th class="px-3 py-2">Size</th>
            <th class="px-3 py-2">Sprint</th>
            <th class="px-3 py-2">Title</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as item (item.wi_number)}
            <tr
              class="cursor-pointer border-t border-[var(--color-border)] hover:bg-[var(--color-surface-hi)]"
              class:bg-[var(--color-surface-hi)]={item.wi_number === cursor}
              class:opacity-55={item.archived}
              class:italic={item.archived}
              tabindex="0"
              onclick={() => open(item)}
              onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), open(item))}
            >
              <td class="px-3 py-1.5 font-mono text-xs text-[var(--color-muted)]">{item.wi_number}</td>
              <td class="px-3 py-1.5">{item.area ?? "—"}</td>
              <td class="px-3 py-1.5">{@render badge(item.wi_type)}</td>
              <td class="px-3 py-1.5">{@render badge(item.wi_status)}</td>
              <td class="px-3 py-1.5">{item.wi_tshirt}</td>
              <td class="px-3 py-1.5">{item.sprint ?? "—"}</td>
              <td class="px-3 py-1.5 font-medium">{item.title}</td>
            </tr>
          {:else}
            <tr><td class="px-3 py-3 text-sm text-[var(--color-muted)]" colspan="7">No work items found.</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
{/snippet}

{#snippet detailView(item: WorkItem)}
  <article class="max-w-3xl space-y-4">
    <div class="flex items-center justify-between">
      <button class="rounded border border-[var(--color-border)] px-3 py-1 text-sm hover:bg-[var(--color-surface-hi)]" onclick={() => (detail = null)}>← Back</button>
    </div>

    <h2 class="text-2xl font-semibold">
      {item.title}
      {#if item.archived}<span class="ml-2 rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]">Archived</span>{/if}
    </h2>

    <dl class="flex flex-wrap gap-x-6 gap-y-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm">
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">ID</dt><dd class="font-mono">{item.wi_number}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Project</dt><dd>{item.project ?? "—"}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Area</dt><dd>{item.area ?? "—"}</dd></div>
      <div class="flex items-center gap-1"><dt class="text-[var(--color-muted)]">Type</dt><dd>{@render badge(item.wi_type)}</dd></div>
      <div class="flex items-center gap-1"><dt class="text-[var(--color-muted)]">Status</dt><dd>{@render badge(item.wi_status)}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Size</dt><dd>{item.wi_tshirt}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Sprint</dt><dd>{item.sprint ?? "—"}</dd></div>
      {#if item.parent != null}
        <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Parent</dt><dd>#{item.parent}</dd></div>
      {/if}
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Created</dt><dd>{new Date(item.created).toLocaleString()}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Updated</dt><dd>{new Date(item.updated).toLocaleString()}</dd></div>
    </dl>

    {#if item.tags.length > 0}
      <div class="flex flex-wrap gap-1">
        {#each item.tags as tag (tag)}<span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">#{tag}</span>{/each}
      </div>
    {/if}

    <section>
      <h3 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">Content</h3>
      <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
      <div class="prose prose-invert max-w-none text-sm">{@html renderMarkdown(item.content)}</div>
    </section>

    {#if item.details}
      <section>
        <h3 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">Details</h3>
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
        <div class="prose prose-invert max-w-none text-sm">{@html renderMarkdown(item.details)}</div>
      </section>
    {/if}

    {#if related.length > 0}
      <section>
        <h3 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">Related</h3>
        <ul class="space-y-1 text-sm">
          {#each related as n (n.node_id + n.label)}
            <li class="flex items-center gap-2">
              <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">{n.label}</span>
              <span>{relatedLabel(n)}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  </article>
{/snippet}

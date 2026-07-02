<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    WI_TYPES,
    TSHIRTS,
    type Project,
    type WorkItem,
    type Neighbor,
  } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";
  import MultiSelectFilter from "$lib/components/MultiSelectFilter.svelte";
  import WorkItemForm from "$lib/components/WorkItemForm.svelte";
  import Comments from "$lib/components/Comments.svelte";

  const ALL = "\u0000all";
  const UNASSIGNED = "Unassigned";

  // WI #83 — persist the chosen project so it survives navigating away and
  // back (the page component remounts on client-side nav). SSR-safe.
  const STICKY_KEY = "korg.workitems.project";
  function loadStickyProject(): string | null {
    try {
      return typeof localStorage !== "undefined" ? localStorage.getItem(STICKY_KEY) : null;
    } catch {
      return null;
    }
  }
  function saveStickyProject(name: string): void {
    try {
      if (typeof localStorage !== "undefined") localStorage.setItem(STICKY_KEY, name);
    } catch {
      /* storage unavailable — non-fatal */
    }
  }

  let projects = $state<Project[]>([]);
  let current = $state<string>(ALL);
  let items = $state<WorkItem[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let detail = $state<WorkItem | null>(null);
  let cursor = $state<number | null>(null);
  let related = $state<Neighbor[]>([]);
  let currentAreas = $state<{ id: number; name: string }[]>([]);
  let detailAreas = $state<{ id: number; name: string }[]>([]);

  let creating = $state(false);
  let editing = $state(false);

  // project rail: area-add
  let newProject = $state("");
  let areaAddFor = $state<string | null>(null);
  let areaName = $state("");
  let areaDesc = $state("");

  // relationships
  let relAdding = $state(false);
  let relTarget = $state("");
  let relLabel = $state("related-to");

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
    if (current === ALL) {
      const stored = loadStickyProject();
      if (stored !== null && (stored === ALL || ps.some((p) => p.name === stored))) {
        current = stored;
      } else if (recent.project) {
        current = recent.project;
      }
    }
  }

  async function loadItems() {
    items = await api.workItems(current === ALL ? undefined : current);
    currentAreas = current === ALL ? [] : await api.areas(current).catch(() => []);
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
    saveStickyProject(name);
    detail = null;
    creating = false;
    await loadItems();
  }

  async function open(item: WorkItem) {
    detail = item;
    cursor = item.wi_number;
    editing = false;
    related = [];
    detailAreas = [];
    related = await api.neighbors(item.node_id).catch(() => []);
    detailAreas = item.project ? await api.areas(item.project).catch(() => []) : [];
  }

  async function refreshDetail() {
    if (!detail) return;
    const u = await api.workItem(detail.wi_number).catch(() => null);
    if (u) {
      detail = u;
      related = await api.neighbors(u.node_id).catch(() => []);
    }
  }

  async function addProject() {
    const name = newProject.trim();
    if (name === "") return;
    await api.createProject(name);
    newProject = "";
    await loadProjects();
    await pick(name);
  }

  async function addArea() {
    if (!areaAddFor || areaName.trim() === "") return;
    await api.createArea(areaAddFor, areaName.trim(), areaDesc.trim() || undefined);
    if (areaAddFor === current) currentAreas = await api.areas(current).catch(() => []);
    areaName = "";
    areaDesc = "";
    areaAddFor = null;
  }

  async function toggleArchive() {
    if (!detail) return;
    await api.updateWorkItem(detail.wi_number, { archived: !detail.archived });
    await refreshDetail();
    await loadItems();
  }

  async function addRel() {
    if (!detail) return;
    const wn = parseInt(relTarget, 10);
    if (!wn) return;
    const target = await api.workItem(wn).catch(() => null);
    if (!target) {
      error = `No work item #${wn}`;
      return;
    }
    await api.relate(detail.node_id, target.node_id, relLabel.trim() || "related-to");
    relTarget = "";
    relAdding = false;
    related = await api.neighbors(detail.node_id).catch(() => []);
  }

  async function removeRel(relId: number) {
    if (!detail) return;
    await api.unrelate(relId);
    related = await api.neighbors(detail.node_id).catch(() => []);
  }

  function relatedLabel(n: Neighbor): string {
    const wi = items.find((i) => i.node_id === n.node_id);
    if (wi) return `#${wi.wi_number} ${wi.title}`;
    return `${n.kind} #${n.node_id}`;
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
    if (creating) {
      if (e.key === "Escape") creating = false;
      return;
    }
    if (detail) {
      if (e.key === "Escape") {
        if (editing) editing = false;
        else detail = null;
      }
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
    <div class="grid grid-cols-1 gap-4 md:grid-cols-[14rem_1fr]">
      <!-- project rail -->
      <aside class="space-y-2">
        <nav class="overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
          <button
            class="block w-full px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
            class:bg-[var(--color-surface-hi)]={current === ALL}
            aria-current={current === ALL ? "true" : "false"}
            onclick={() => pick(ALL)}>All projects</button>
          {#each projects as p (p.id)}
            <div class="flex items-center" class:bg-[var(--color-surface-hi)]={current === p.name}>
              <button
                class="flex-1 px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
                class:text-[var(--color-accent)]={current === p.name}
                aria-current={current === p.name ? "true" : "false"}
                onclick={() => pick(p.name)}>{p.name}</button>
              <button
                class="px-2 py-2 text-[var(--color-muted)] hover:text-[var(--color-accent)]"
                title={`Add area to ${p.name}`}
                aria-label={`Add area to ${p.name}`}
                onclick={() => { areaAddFor = areaAddFor === p.name ? null : p.name; areaName = ""; areaDesc = ""; }}
              >◇</button>
            </div>
          {/each}
        </nav>

        {#if areaAddFor}
          <div class="space-y-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-sm">
            <p class="text-xs text-[var(--color-muted)]">Add area to <span class="text-[var(--color-accent)]">{areaAddFor}</span></p>
            <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="area name" bind:value={areaName} onkeydown={(e) => e.key === "Enter" && addArea()} />
            <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none" placeholder="description (optional)" bind:value={areaDesc} />
            <div class="flex justify-end gap-2">
              <button class="rounded px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]" onclick={() => (areaAddFor = null)}>Cancel</button>
              <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={addArea}>Create</button>
            </div>
          </div>
        {/if}

        <div class="flex gap-1">
          <input class="min-w-0 flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="new project…" bind:value={newProject} onkeydown={(e) => e.key === "Enter" && addProject()} />
          <button class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm hover:bg-[var(--color-accent-soft)]" onclick={addProject}>+</button>
        </div>
      </aside>

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
    {#if current !== ALL && currentProject}
      <details class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5 text-sm">
        <summary class="cursor-pointer text-xs font-semibold text-[var(--color-muted)] hover:text-[var(--color-accent)]">Project Details</summary>
        <dl class="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
          <dt class="text-[var(--color-muted)]">Short Name</dt><dd>{currentProject.name}</dd>
          {#if currentProject.cn_path}<dt class="text-[var(--color-muted)]">CN Path</dt><dd>{currentProject.cn_path}</dd>{/if}
          {#if currentProject.gh_repo}<dt class="text-[var(--color-muted)]">GitHub Repo</dt><dd>{currentProject.gh_repo}</dd>{/if}
          {#if currentProject.description}<dt class="text-[var(--color-muted)]">Description</dt><dd>{currentProject.description}</dd>{/if}
        </dl>
      </details>
    {/if}

    <!-- toolbar -->
    <div class="flex flex-wrap items-center gap-2">
      <input class="w-40 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="search…" bind:value={search} />
      <MultiSelectFilter label="areas" options={areaOptions} selected={fAreas} modified={modified.areas} onchange={(v) => (fAreas = v)} />
      <MultiSelectFilter label="types" options={typeOptions} selected={fTypes} modified={modified.types} onchange={(v) => (fTypes = v)} />
      <MultiSelectFilter label="statuses" options={statusOptions} selected={fStatuses} modified={modified.statuses} onchange={(v) => (fStatuses = v)} />
      <MultiSelectFilter label="sizes" options={sizeOptions} selected={fSizes} modified={modified.sizes} onchange={(v) => (fSizes = v)} />
      <MultiSelectFilter label="sprints" options={sprintOptions()} selected={fSprints} modified={modified.sprints} onchange={(v) => (fSprints = v)} />
      <label class="flex items-center gap-1 text-xs text-[var(--color-muted)]"><input type="checkbox" bind:checked={showArchived} /> archived</label>
      <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]" onclick={resetFilters}>Clear</button>
      <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]" title="Refresh" aria-label="Refresh" onclick={loadItems}>↻</button>
      <div class="ml-auto">
        <button
          class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40"
          disabled={current === ALL}
          title={current === ALL ? "Pick a project first" : "New work item"}
          onclick={() => (creating = !creating)}>+ New Work Item</button>
      </div>
    </div>

    {#if creating}
      <WorkItemForm
        projectId={currentProject?.id}
        areas={currentAreas}
        onSaved={async () => { creating = false; await loadItems(); }}
        onCancel={() => (creating = false)}
      />
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
  <article class="space-y-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
    <div class="flex items-center justify-between">
      <button class="rounded border border-[var(--color-border)] px-3 py-1 text-sm hover:bg-[var(--color-surface-hi)]" onclick={() => (detail = null)}>← Back</button>
      {#if !editing}
        <div class="flex gap-2">
          <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1 text-sm hover:bg-[var(--color-accent)]" onclick={() => (editing = true)}>Edit</button>
          <button class="rounded border border-[var(--color-border)] px-3 py-1 text-sm hover:bg-[var(--color-surface-hi)]" onclick={toggleArchive}>{item.archived ? "Un-archive" : "Archive"}</button>
        </div>
      {/if}
    </div>

    {#if editing}
      <WorkItemForm
        editItem={item}
        projectId={currentProject?.id}
        areas={detailAreas}
        onSaved={async () => { editing = false; await refreshDetail(); await loadItems(); }}
        onCancel={() => (editing = false)}
      />
    {:else}
      <h2 class="text-2xl font-semibold">
        {item.title}
        {#if item.archived}<span class="ml-2 rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]">Archived</span>{/if}
      </h2>

      <dl class="flex flex-wrap gap-x-6 gap-y-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-hi)] p-3 text-sm">
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
          <div class="prose prose-invert max-w-none rounded p-2 text-sm" style="background: color-mix(in oklch, var(--color-surface) 75%, var(--color-accent) 25%)">{@html renderMarkdown(item.details)}</div>
        </section>
      {/if}
    {/if}

    <section>
      <div class="mb-1 flex items-center justify-between border-b border-[var(--color-border)] pb-1">
        <h3 class="text-sm font-semibold">Relationships</h3>
        <button class="rounded bg-[var(--color-surface-hi)] px-2 py-0.5 text-xs hover:bg-[var(--color-accent-soft)]" onclick={() => (relAdding = !relAdding)}>+ Add</button>
      </div>
      {#if relAdding}
        <div class="mb-2 flex flex-wrap items-center gap-2 text-sm">
          <input class="w-24 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none" placeholder="label" bind:value={relLabel} />
          <span class="text-xs text-[var(--color-muted)]">→ work item #</span>
          <input class="w-20 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none" placeholder="42" bind:value={relTarget} onkeydown={(e) => e.key === "Enter" && addRel()} />
          <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={addRel}>Add</button>
        </div>
      {/if}
      {#if related.length > 0}
        <ul class="space-y-1 text-sm">
          {#each related as n (n.rel_id)}
            <li class="flex items-center gap-2">
              <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">{n.label}</span>
              <span>{relatedLabel(n)}</span>
              <button class="ml-auto rounded px-1 text-xs text-[var(--color-muted)] hover:text-red-400" aria-label="Remove" title="Remove" onclick={() => removeRel(n.rel_id)}>✕</button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="text-xs text-[var(--color-muted)]">No relationships.</p>
      {/if}
    </section>

    <Comments node_id={item.node_id} />
  </article>
{/snippet}

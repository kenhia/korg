<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    api,
    WI_TYPES,
    WI_STATUSES,
    TSHIRTS,
    PROJECT_STATUSES,
    type Project,
    type WorkItem,
    type Neighbor,
  } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";
  import MultiSelectFilter from "$lib/components/MultiSelectFilter.svelte";
  import WorkItemForm from "$lib/components/WorkItemForm.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";
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

  // project rail: area-add + edit-project (WI #246)
  let newProject = $state("");
  let areaAddFor = $state<string | null>(null);
  let areaName = $state("");
  let areaDesc = $state("");
  let showAllProjects = $state(false);
  let editProject = $state<Project | null>(null);
  let eStatus = $state<string>("active");
  let eMachines = $state("");
  let eDeploy = $state("");
  let eCategory = $state("");
  let eDescription = $state("");
  let eGhRepo = $state("");
  let eCnPath = $state("");

  // Rail shows only active+maintenance unless "show all" (WI #246).
  const visibleProjects = $derived(
    projects.filter(
      (p) => showAllProjects || p.status === "active" || p.status === "maintenance",
    ),
  );

  // Deterministic per-project hue — Ken tunes by eye later; hash keeps a
  // name's color stable across sessions/machines.
  function projColor(name: string): string {
    let h = 0;
    for (const ch of name) h = (h * 31 + ch.codePointAt(0)!) >>> 0;
    return `hsl(${h % 360} 55% 62%)`;
  }

  function openEditProject(p: Project) {
    editProject = p;
    eStatus = p.status;
    eMachines = p.machines.join(", ");
    eDeploy = p.deploy_to.join(", ");
    eCategory = p.category ?? "";
    eDescription = p.description ?? "";
    eGhRepo = p.gh_repo ?? "";
    eCnPath = p.cn_path ?? "";
    areaAddFor = null;
  }

  function csv(v: string): string[] {
    return v.split(",").map((s) => s.trim()).filter(Boolean);
  }

  async function saveProject() {
    if (!editProject) return;
    await api.updateProject(editProject.name, {
      status: eStatus as (typeof PROJECT_STATUSES)[number],
      machines: csv(eMachines),
      deploy_to: csv(eDeploy),
      category: eCategory.trim() || null,
      description: eDescription.trim() || null,
      gh_repo: eGhRepo.trim() || null,
      cn_path: eCnPath.trim() || null,
    });
    editProject = null;
    projects = await api.projects();
  }

  // relationships
  let relAdding = $state(false);
  let relTarget = $state("");
  let relLabel = $state("related-to");

  // WI #260 — find any node by id. A work item resolves to a navigate +
  // highlight (jump to its project, flash the row); any other kind opens the
  // shared preview panel. forceShow keeps the jumped-to row visible even when
  // the current filters (e.g. "hide closed") would otherwise drop it.
  let findId = $state("");
  let previewNode = $state<number | null>(null);
  let flashWi = $state<number | null>(null);
  let forceShow = $state<Set<number>>(new Set());

  // filters
  let search = $state("");
  let showArchived = $state(false);
  let fTypes = $state<Set<string>>(new Set());
  let fStatuses = $state<Set<string>>(new Set());
  let fSizes = $state<Set<string>>(new Set());
  let fAreas = $state<Set<string>>(new Set());
  let fSprints = $state<Set<string>>(new Set());

  // WI #103 — Quick Edit: inline type/status/size/area dropdowns in the list
  // view. Items closed while quick-editing stay visible (despite the default
  // "don't show closed" filter) until Quick Edit is turned off, so the user
  // can keep tweaking (or undo) without the row vanishing mid-edit.
  let quickEdit = $state(false);
  let quickEditKeep = $state<Set<number>>(new Set());

  function toggleQuickEdit() {
    quickEdit = !quickEdit;
    if (!quickEdit) quickEditKeep = new Set();
  }

  async function quickUpdate(item: WorkItem, patch: Partial<{ wi_type: string; wi_status: string; wi_tshirt: string }>) {
    await api.updateWorkItem(item.wi_number, patch);
    Object.assign(item, patch);
    if (patch.wi_status === "closed") {
      quickEditKeep = new Set(quickEditKeep).add(item.wi_number);
    }
  }

  // Area is stored as area_id; only editable inline when one project is
  // selected (currentAreas is that project's areas). Viewing "All projects"
  // keeps area read-only in Quick Edit rather than fetching every row's
  // own project's areas just for this.
  async function quickUpdateArea(item: WorkItem, areaName: string) {
    const area = currentAreas.find((a) => a.name === areaName);
    await api.updateWorkItem(item.wi_number, { area_id: area ? area.id : null });
    item.area = area ? area.name : null;
  }

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
      if (forceShow.has(it.wi_number)) return true; // a find-by-ID jump always shows its target
      if (!showArchived && it.archived) return false;
      if (!fTypes.has(it.wi_type)) return false;
      if (!fStatuses.has(it.wi_status) && !(quickEdit && quickEditKeep.has(it.wi_number))) return false;
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
    // The rail filters archived client-side behind a toggle, so ask for both.
    items = (
      await api.workItems(current === ALL ? undefined : current, {
        archived: "all",
      })
    ).items;
    currentAreas = current === ALL ? [] : await api.areas(current).catch(() => []);
    forceShow = new Set();
    resetFilters();
    cursor = filtered[0]?.wi_number ?? null;
  }

  // WI #260 — resolve the entered id and either jump to the work item or open
  // the preview panel for any other node kind.
  async function findById() {
    const id = parseInt(findId.trim(), 10);
    if (!Number.isFinite(id)) return;
    let node;
    try {
      node = await api.node(id);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return;
    }
    if (!node) {
      error = `No node with id ${id}.`;
      return;
    }
    error = null;
    if (node.kind === "workitem" && node.wi_number != null) {
      findId = "";
      await gotoWorkItem(node.wi_number, node.project);
    } else {
      previewNode = id;
    }
  }

  async function gotoWorkItem(wi: number, project: string | null) {
    // From "All projects" stay there and highlight in place, so the project
    // column (WI #313) shows where the hit lives; otherwise jump to its project.
    const target = current === ALL ? ALL : (project ?? ALL);
    detail = null;
    creating = false;
    if (target !== current) await pick(target);
    forceShow = new Set([wi]);
    cursor = wi;
    flashWi = wi;
    await tick();
    document.getElementById(`wi-row-${wi}`)?.scrollIntoView({ block: "center" });
    setTimeout(() => {
      if (flashWi === wi) flashWi = null;
    }, 2200);
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
    related = (await api.neighbors(item.node_id).catch(() => null))?.items ?? [];
    detailAreas = item.project ? await api.areas(item.project).catch(() => []) : [];
  }

  async function refreshDetail() {
    if (!detail) return;
    const u = await api.workItem(detail.wi_number).catch(() => null);
    if (u) {
      detail = u;
      related = (await api.neighbors(u.node_id).catch(() => null))?.items ?? [];
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
    related = (await api.neighbors(detail.node_id).catch(() => null))?.items ?? [];
  }

  async function removeRel(relId: number) {
    if (!detail) return;
    await api.unrelate(relId);
    related = (await api.neighbors(detail.node_id).catch(() => null))?.items ?? [];
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
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h1 class="text-xl font-semibold">Work items</h1>
    <div class="flex items-center gap-1" title="Jump to a work item, or preview any node, by its id">
      <input
        class="w-32 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none"
        placeholder="find by ID…"
        inputmode="numeric"
        bind:value={findId}
        onkeydown={(e) => e.key === "Enter" && findById()}
      />
      <button
        class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-sm hover:bg-[var(--color-accent)]"
        onclick={findById}>Go</button>
    </div>
  </div>

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
          {#each visibleProjects as p (p.id)}
            <div class="flex items-center" class:bg-[var(--color-surface-hi)]={current === p.name}>
              <button
                class="flex-1 px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
                style="color: {projColor(p.name)}"
                class:font-semibold={current === p.name}
                aria-current={current === p.name ? "true" : "false"}
                onclick={() => pick(p.name)}>{p.name}{#if p.status !== "active"}<span class="ml-1 text-xs text-[var(--color-muted)]">({p.status})</span>{/if}</button>
              <button
                class="px-1 py-2 text-[var(--color-muted)] hover:text-[var(--color-accent)]"
                title={`Edit project ${p.name}`}
                aria-label={`Edit project ${p.name}`}
                onclick={() => (editProject?.id === p.id ? (editProject = null) : openEditProject(p))}
              >✎</button>
              <button
                class="px-2 py-2 text-[var(--color-muted)] hover:text-[var(--color-accent)]"
                title={`Add area to ${p.name}`}
                aria-label={`Add area to ${p.name}`}
                onclick={() => { areaAddFor = areaAddFor === p.name ? null : p.name; areaName = ""; areaDesc = ""; editProject = null; }}
              >◇</button>
            </div>
          {/each}
        </nav>
        <label class="flex items-center gap-1 px-1 text-xs text-[var(--color-muted)]">
          <input type="checkbox" bind:checked={showAllProjects} /> show all projects
        </label>

        {#if editProject}
          <div class="space-y-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-sm">
            <p class="text-xs text-[var(--color-muted)]">Edit project <span style="color: {projColor(editProject.name)}">{editProject.name}</span></p>
            <label class="block text-xs text-[var(--color-muted)]">status
              <select class="mt-0.5 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm" bind:value={eStatus}>
                {#each PROJECT_STATUSES as s (s)}<option value={s}>{s}</option>{/each}
              </select>
            </label>
            <label class="block text-xs text-[var(--color-muted)]">machines (comma-sep)
              <input class="mt-0.5 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="kai, kubs0" bind:value={eMachines} />
            </label>
            <label class="block text-xs text-[var(--color-muted)]">deploy to (comma-sep)
              <input class="mt-0.5 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="kubsdb" bind:value={eDeploy} />
            </label>
            <label class="block text-xs text-[var(--color-muted)]">category
              <input class="mt-0.5 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" bind:value={eCategory} />
            </label>
            <label class="block text-xs text-[var(--color-muted)]">description
              <input class="mt-0.5 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" bind:value={eDescription} />
            </label>
            <label class="block text-xs text-[var(--color-muted)]">github repo
              <input class="mt-0.5 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" bind:value={eGhRepo} />
            </label>
            <label class="block text-xs text-[var(--color-muted)]">cn path
              <input class="mt-0.5 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" bind:value={eCnPath} />
            </label>
            <div class="flex justify-end gap-2">
              <button class="rounded px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]" onclick={() => (editProject = null)}>Cancel</button>
              <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={saveProject}>Save</button>
            </div>
          </div>
        {/if}

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

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

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
          <dt class="text-[var(--color-muted)]">Status</dt><dd>{currentProject.status}</dd>
          {#if currentProject.machines.length > 0}<dt class="text-[var(--color-muted)]">Machines</dt><dd>{currentProject.machines.join(", ")}</dd>{/if}
          {#if currentProject.deploy_to.length > 0}<dt class="text-[var(--color-muted)]">Deploys To</dt><dd>{currentProject.deploy_to.join(", ")}</dd>{/if}
          {#if currentProject.category}<dt class="text-[var(--color-muted)]">Category</dt><dd>{currentProject.category}</dd>{/if}
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
      <button
        class="rounded border px-2 py-1 text-xs"
        class:border-[var(--color-accent)]={quickEdit}
        class:bg-[var(--color-accent-soft)]={quickEdit}
        class:border-[var(--color-border)]={!quickEdit}
        class:hover:bg-[var(--color-surface-hi)]={!quickEdit}
        onclick={toggleQuickEdit}
      >{quickEdit ? "✓ Quick Edit" : "Quick Edit"}</button>
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
            {#if current === ALL}<th class="px-3 py-2">Project</th>{/if}
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
              id={`wi-row-${item.wi_number}`}
              class="cursor-pointer border-t border-[var(--color-border)] hover:bg-[var(--color-surface-hi)]"
              class:bg-[var(--color-surface-hi)]={item.wi_number === cursor}
              class:ring-2={item.wi_number === flashWi}
              class:ring-inset={item.wi_number === flashWi}
              class:ring-[var(--color-accent)]={item.wi_number === flashWi}
              class:opacity-55={item.archived}
              class:italic={item.archived}
              tabindex="0"
              onclick={() => open(item)}
              onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), open(item))}
            >
              <td class="px-3 py-1.5 font-mono text-xs text-[var(--color-muted)]">{item.wi_number}</td>
              {#if current === ALL}<td class="px-3 py-1.5 text-xs text-[var(--color-muted)]">{item.project ?? "—"}</td>{/if}
              <td class="px-3 py-1.5">
                {#if quickEdit && current !== ALL}
                  <select
                    class="rounded bg-[var(--color-surface-hi)] px-1 py-0.5 text-xs outline-none"
                    value={item.area ?? ""}
                    onclick={(e) => e.stopPropagation()}
                    onchange={(e) => quickUpdateArea(item, e.currentTarget.value)}
                  >
                    <option value="">—</option>
                    {#each currentAreas as a (a.id)}<option value={a.name}>{a.name}</option>{/each}
                  </select>
                {:else}
                  {item.area ?? "—"}
                {/if}
              </td>
              <td class="px-3 py-1.5">
                {#if quickEdit}
                  <select
                    class="rounded bg-[var(--color-surface-hi)] px-1 py-0.5 text-xs outline-none"
                    value={item.wi_type}
                    onclick={(e) => e.stopPropagation()}
                    onchange={(e) => quickUpdate(item, { wi_type: e.currentTarget.value })}
                  >
                    {#each WI_TYPES as t (t)}<option value={t}>{t}</option>{/each}
                  </select>
                {:else}
                  {@render badge(item.wi_type)}
                {/if}
              </td>
              <td class="px-3 py-1.5">
                {#if quickEdit}
                  <select
                    class="rounded bg-[var(--color-surface-hi)] px-1 py-0.5 text-xs outline-none"
                    value={item.wi_status}
                    onclick={(e) => e.stopPropagation()}
                    onchange={(e) => quickUpdate(item, { wi_status: e.currentTarget.value })}
                  >
                    {#each WI_STATUSES as s (s)}<option value={s}>{s}</option>{/each}
                  </select>
                {:else}
                  {@render badge(item.wi_status)}
                {/if}
              </td>
              <td class="px-3 py-1.5">
                {#if quickEdit}
                  <select
                    class="rounded bg-[var(--color-surface-hi)] px-1 py-0.5 text-xs outline-none"
                    value={item.wi_tshirt}
                    onclick={(e) => e.stopPropagation()}
                    onchange={(e) => quickUpdate(item, { wi_tshirt: e.currentTarget.value })}
                  >
                    {#each TSHIRTS as t (t)}<option value={t}>{t}</option>{/each}
                  </select>
                {:else}
                  {item.wi_tshirt}
                {/if}
              </td>
              <td class="px-3 py-1.5">{item.sprint ?? "—"}</td>
              <td class="px-3 py-1.5 font-medium">{item.title}</td>
            </tr>
          {:else}
            <tr><td class="px-3 py-3 text-sm text-[var(--color-muted)]" colspan={current === ALL ? 8 : 7}>No work items found.</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
{/snippet}

{#snippet detailView(item: WorkItem)}
  <!-- The route is wide for the list table; cap the single-item view so long prose stays readable. -->
  <article class="max-w-5xl space-y-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
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

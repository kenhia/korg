<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Project, type WorkItem } from "$lib/api";

  let projects = $state<Project[]>([]);
  let selected = $state<string>("homelab-ai");
  let items = $state<WorkItem[]>([]);
  let edges = $state<[number, number][]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let showDone = $state(false);

  // depends_on: edge [left, right] = left depends on right (node id IS wi_number)
  const depsOf = $derived.by(() => {
    const m = new Map<number, number[]>();
    for (const [l, r] of edges) m.set(l, [...(m.get(l) ?? []), r]);
    return m;
  });
  const unlocksOf = $derived.by(() => {
    const m = new Map<number, number[]>();
    for (const [l, r] of edges) m.set(r, [...(m.get(r) ?? []), l]);
    return m;
  });
  const byNumber = $derived(new Map(items.map((w) => [w.wi_number, w])));

  // Terminal statuses: done and closed are finished; resolved is shipped
  // awaiting human confirmation — all three unblock dependents.
  const TERMINAL = new Set(["done", "closed", "resolved"]);
  function isDone(w: WorkItem): boolean {
    return TERMINAL.has(w.wi_status);
  }
  function blockedBy(w: WorkItem): number[] {
    return (depsOf.get(w.wi_number) ?? []).filter((d) => {
      const dep = byNumber.get(d);
      return dep ? !isDone(dep) : false;
    });
  }
  function isParked(w: WorkItem): boolean {
    return w.tags.includes("parked");
  }

  const open = $derived(items.filter((w) => !isDone(w) && !w.archived));
  const done = $derived(items.filter((w) => isDone(w)));
  const frontier = $derived(open.filter((w) => !isParked(w) && blockedBy(w).length === 0));
  const blocked = $derived(open.filter((w) => !isParked(w) && blockedBy(w).length > 0));
  const parked = $derived(open.filter(isParked));

  type AreaRow = { name: string; done: number; total: number };
  const areas = $derived.by(() => {
    const m = new Map<string, AreaRow>();
    for (const w of items) {
      const name = w.area ?? "(no area)";
      const row = m.get(name) ?? { name, done: 0, total: 0 };
      row.total += 1;
      if (isDone(w)) row.done += 1;
      m.set(name, row);
    }
    return [...m.values()].sort((a, b) => a.name.localeCompare(b.name));
  });

  async function load() {
    loading = true;
    error = null;
    try {
      const plan = await api.plan(selected);
      items = plan.items;
      edges = plan.edges;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    try {
      projects = await api.projects();
      if (!projects.some((p) => p.name === selected) && projects.length > 0) {
        selected = projects[0].name;
      }
    } catch {
      /* project list is a nicety; plan load reports real errors */
    }
    await load();
  });
</script>

<svelte:head><title>korg — plan</title></svelte:head>

<div class="space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">Plan</h1>
    <select
      class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-sm"
      bind:value={selected}
      onchange={load}
    >
      {#each projects as p (p.id)}
        <option value={p.name}>{p.name}</option>
      {/each}
    </select>
  </div>

  {#if loading}
    <p class="text-sm opacity-70">Loading…</p>
  {:else if error}
    <p class="text-sm text-red-500">{error}</p>
  {:else if items.length === 0}
    <p class="text-sm opacity-70">No work items in this project.</p>
  {:else}
    <!-- workstream progress -->
    <div class="grid gap-2 sm:grid-cols-2">
      {#each areas as a (a.name)}
        <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
          <div class="flex items-center justify-between text-sm">
            <span class="font-medium">{a.name}</span>
            <span class="opacity-70">{a.done}/{a.total}</span>
          </div>
          <div class="mt-1.5 h-1.5 overflow-hidden rounded bg-[var(--color-surface-hi)]">
            <div
              class="h-full rounded bg-[var(--color-accent)]"
              style="width: {a.total ? (100 * a.done) / a.total : 0}%"
            ></div>
          </div>
        </div>
      {/each}
    </div>

    <!-- frontier: startable right now -->
    <section>
      <h2 class="mb-2 text-sm font-semibold uppercase tracking-wide opacity-70">
        Frontier — startable now ({frontier.length})
      </h2>
      {#if frontier.length === 0}
        <p class="text-sm opacity-70">Nothing unblocked — check Blocked below.</p>
      {/if}
      <ul class="space-y-1.5">
        {#each frontier as w (w.wi_number)}
          <li class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
            <div class="flex items-baseline gap-2">
              <span class="font-mono text-xs opacity-60">#{w.wi_number}</span>
              <span class="text-sm">{w.title}</span>
              <span class="ml-auto shrink-0 rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs">{w.wi_tshirt}</span>
            </div>
            {#if (unlocksOf.get(w.wi_number) ?? []).length > 0}
              <div class="mt-1 text-xs opacity-60">
                unlocks {(unlocksOf.get(w.wi_number) ?? []).map((n) => `#${n}`).join(" ")}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </section>

    <!-- blocked -->
    <section>
      <h2 class="mb-2 text-sm font-semibold uppercase tracking-wide opacity-70">
        Blocked ({blocked.length})
      </h2>
      <ul class="space-y-1.5">
        {#each blocked as w (w.wi_number)}
          <li class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 opacity-75">
            <div class="flex items-baseline gap-2">
              <span class="font-mono text-xs opacity-60">#{w.wi_number}</span>
              <span class="text-sm">{w.title}</span>
              <span class="ml-auto shrink-0 text-xs opacity-60">
                blocked by {blockedBy(w).map((n) => `#${n}`).join(" ")}
              </span>
            </div>
          </li>
        {/each}
      </ul>
    </section>

    {#if parked.length > 0}
      <section>
        <h2 class="mb-2 text-sm font-semibold uppercase tracking-wide opacity-70">
          Parked on purpose ({parked.length})
        </h2>
        <ul class="space-y-1.5">
          {#each parked as w (w.wi_number)}
            <li class="rounded border border-dashed border-[var(--color-border)] px-3 py-2 opacity-60">
              <span class="font-mono text-xs">#{w.wi_number}</span>
              <span class="text-sm">{w.title}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <section>
      <button class="text-sm opacity-70 hover:opacity-100" onclick={() => (showDone = !showDone)}>
        {showDone ? "▾" : "▸"} Done ({done.length})
      </button>
      {#if showDone}
        <ul class="mt-2 space-y-1">
          {#each done as w (w.wi_number)}
            <li class="px-3 text-sm line-through opacity-50">#{w.wi_number} {w.title}</li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

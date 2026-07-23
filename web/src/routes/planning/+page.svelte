<script lang="ts">
  import { onMount } from "svelte";
  import { dndzone, type DndEvent } from "svelte-dnd-action";
  import { api, type ProposalRow } from "$lib/api";
  import type { ProposalStatus } from "$lib/generated/vocab";
  import { chip, midRank } from "$lib/domain";
  import NodePreview from "$lib/components/NodePreview.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import { attempt, reportError } from "$lib/toast.svelte";

  type DndItem = { id: number; proposal: ProposalRow };
  type Covered = { wi_number: number; title: string };

  // WI #565 — the queue spans repos, so scope it to the project you're in.
  // Sticky like the work-items rail (same convention, SSR-safe).
  const PROJECT_KEY = "korg.planning.project";
  const ALL_PROJECTS = "";
  function loadStickyProject(): string {
    try {
      return typeof localStorage !== "undefined"
        ? (localStorage.getItem(PROJECT_KEY) ?? ALL_PROJECTS)
        : ALL_PROJECTS;
    } catch {
      return ALL_PROJECTS;
    }
  }
  function saveStickyProject(name: string): void {
    try {
      if (typeof localStorage !== "undefined")
        localStorage.setItem(PROJECT_KEY, name);
    } catch {
      /* storage unavailable — non-fatal */
    }
  }

  let projectFilter = $state(ALL_PROJECTS);
  let proposalsRaw = $state<ProposalRow[]>([]);
  let covers = $state<Record<number, Covered[]>>({});
  let loading = $state(true);
  let loadError = $state<unknown>(null);
  let copiedId = $state<number | null>(null);
  // Covered items are work items, whose node id equals their wi_number — pass
  // it straight to the shared preview panel.
  let previewNode = $state<number | null>(null);
  const flip = 150;

  let pinnedBuf = $state<DndItem[]>([]);
  let queueBuf = $state<DndItem[]>([]);

  const active = $derived(
    proposalsRaw.filter((p) => p.status === "active").sort((a, b) => Number(a.rank) - Number(b.rank)),
  );
  const done = $derived(proposalsRaw.filter((p) => p.status === "done"));
  const declined = $derived(proposalsRaw.filter((p) => p.status === "declined"));
  let showDone = $state(false);
  let showDeclined = $state(false);

  // Every project the queue mentions, for the filter control.
  let knownProjects = $state<string[]>([]);

  async function pickProject(name: string) {
    projectFilter = name;
    saveStickyProject(name);
    await load();
  }

  function rebuild() {
    const proposed = proposalsRaw.filter((p) => p.status === "proposed");
    pinnedBuf = proposed
      .filter((p) => p.pinned)
      .sort((a, b) => Number(a.rank) - Number(b.rank))
      .map((p) => ({ id: p.node_id, proposal: p }));
    queueBuf = proposed
      .filter((p) => !p.pinned)
      .sort((a, b) => Number(a.rank) - Number(b.rank))
      .map((p) => ({ id: p.node_id, proposal: p }));
  }

  // One authoritative read per proposal (WI #536) — no neighbors call, no
  // client-side join against every work item in the instance.
  async function loadCovers(proposalNodeId: number) {
    // Best-effort: a proposal whose covers fail to load still renders, and the
    // page-level load already reports a broader failure.
    const detail = await attempt(
      () => api.proposal(proposalNodeId),
      "Load covered work items",
    );
    covers[proposalNodeId] = (detail?.covered ?? []).map((c) => ({
      wi_number: c.wi_number,
      title: c.title,
    }));
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      proposalsRaw = await api.proposals(
        undefined,
        projectFilter === ALL_PROJECTS ? undefined : projectFilter,
      );
      if (projectFilter === ALL_PROJECTS) {
        knownProjects = [
          ...new Set(proposalsRaw.map((p) => p.project).filter((n): n is string => !!n)),
        ].sort();
      }
      rebuild();
      // Only proposals that actually cover something need a detail read;
      // `covered_count` on the row answers the rest.
      await Promise.all(
        proposalsRaw.filter((p) => p.covered_count > 0).map((p) => loadCovers(p.node_id)),
      );
    } catch (e) {
      loadError = e;
    } finally {
      loading = false;
    }
  }

  function considerPinned(e: CustomEvent<DndEvent<DndItem>>) {
    pinnedBuf = e.detail.items;
  }
  function considerQueue(e: CustomEvent<DndEvent<DndItem>>) {
    queueBuf = e.detail.items;
  }
  async function finalizePinned(e: CustomEvent<DndEvent<DndItem>>) {
    pinnedBuf = e.detail.items;
    await settle(pinnedBuf, e.detail.info.id, true);
  }
  async function finalizeQueue(e: CustomEvent<DndEvent<DndItem>>) {
    queueBuf = e.detail.items;
    await settle(queueBuf, e.detail.info.id, false);
  }
  async function settle(items: DndItem[], movedId: string | number, pinned: boolean) {
    const idx = items.findIndex((it) => String(it.id) === String(movedId));
    if (idx < 0) return;
    const rank = midRank(items[idx - 1]?.proposal.rank, items[idx + 1]?.proposal.rank);
    const moved = items[idx].proposal;
    try {
      await api.updateProposal(moved.node_id, { rank, pinned });
      moved.rank = String(rank);
      moved.pinned = pinned;
    } catch (err) {
      // Resynchronise: the list is showing an order the server rejected.
      reportError(err, "Reorder proposal");
      await load();
    }
  }

  async function togglePin(p: ProposalRow) {
    const r = await attempt(
      () => api.updateProposal(p.node_id, { pinned: !p.pinned }),
      p.pinned ? "Unpin proposal" : "Pin proposal",
    );
    if (!r) return;
    p.pinned = !p.pinned;
    rebuild();
  }

  async function setStatus(p: ProposalRow, status: ProposalStatus) {
    const r = await attempt(
      () => api.updateProposal(p.node_id, { status }),
      "Set proposal status",
    );
    if (!r) return;
    p.status = status;
    proposalsRaw = [...proposalsRaw];
    rebuild();
  }

  async function copyStart(p: ProposalRow) {
    const text = `/start-sprint korg:${p.node_id}`;
    try {
      // navigator.clipboard requires a secure context (HTTPS or localhost);
      // korg is served over plain HTTP on the LAN, so it's often undefined.
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(text);
      } else {
        legacyCopy(text);
      }
      copiedId = p.node_id;
      setTimeout(() => (copiedId = null), 1500);
    } catch (err) {
      reportError(err, "Copy to clipboard");
    }
  }

  function legacyCopy(text: string) {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    if (!ok) throw new Error("Copy failed — clipboard access is unavailable in this context.");
  }

  function openPreview(wi_number: number) {
    previewNode = wi_number;
  }

  onMount(() => {
    projectFilter = loadStickyProject();
    void load();
  });
</script>

{#snippet card(p: ProposalRow)}
  <div class="cursor-grab rounded bg-[var(--color-surface-hi)] p-3 active:cursor-grabbing" data-testid={`proposal-${p.node_id}`}>
    <div class="flex items-start justify-between gap-2">
      <div class="text-sm font-medium">{p.title}</div>
      <div class="flex shrink-0 items-center gap-1">
        <button
          class="rounded px-1.5 py-0.5 text-xs hover:bg-[var(--color-surface)]"
          class:text-[var(--color-accent)]={p.pinned}
          title={p.pinned ? "Unpin" : "Pin to top"}
          onclick={() => togglePin(p)}
        >📌</button>
        <button
          class="rounded px-1.5 py-0.5 text-xs hover:bg-[var(--color-surface)]"
          title="Copy /start-sprint prompt"
          onclick={() => copyStart(p)}
        >{copiedId === p.node_id ? "✓" : "⧉"}</button>
      </div>
    </div>
    {#if p.project}
      <span class="mt-1 inline-block {chip.project}" data-testid="proposal-project">{p.project}</span>
    {/if}
    <p class="mt-1 text-xs text-[var(--color-muted)]">{p.summary}</p>
    {#if covers[p.node_id]?.length}
      <div class="mt-2 flex flex-wrap gap-1">
        {#each covers[p.node_id] as c (c.wi_number)}
          <button
            class="rounded bg-black px-1.5 py-0.5 text-[10px] text-white hover:bg-[var(--color-accent)]"
            title={`Preview #${c.wi_number} — ${c.title}`}
            onclick={() => openPreview(c.wi_number)}
          >#{c.wi_number} {c.title}</button>
        {/each}
      </div>
    {/if}
    <div class="mt-2 flex items-center gap-2">
      {#if p.status === "proposed"}
        <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={() => setStatus(p, "active")}>Start</button>
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, "declined")}>Decline</button>
      {:else if p.status === "active"}
        <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={() => setStatus(p, "done")}>Done</button>
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, "declined")}>Decline</button>
      {:else}
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, "proposed")}>Reopen</button>
      {/if}
      <span class="ml-auto text-xs font-bold text-blue-400">Sprint Plan ID: {p.node_id}</span>
    </div>
  </div>
{/snippet}

<section class="space-y-4">
  <!-- Wraps at narrow widths (WI #549). The header row pushed 41px past a
       390px viewport, which scrolls the whole page sideways — the same class of
       mobile failure as the nav, just less obvious because the overflowing part
       is a filter rather than a link. -->
  <div class="flex flex-wrap items-center justify-between gap-2">
    <h1 class="text-xl font-semibold">Planning</h1>
    <div class="flex flex-wrap items-center gap-3">
      <label class="flex items-center gap-1 text-xs text-[var(--color-muted)]">
        <span class="sr-only">Filter proposals by project</span>
        Project
        <select
          class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 text-xs"
          data-testid="planning-project-filter"
          value={projectFilter}
          onchange={(e) => pickProject((e.currentTarget as HTMLSelectElement).value)}
        >
          <option value="">All projects</option>
          {#each knownProjects as name (name)}<option value={name}>{name}</option>{/each}
        </select>
      </label>
      <p class="text-xs text-[var(--color-muted)]">Proposals agents (or you) can drag to reorder — pinned always sort first.</p>
    </div>
  </div>

  {#if loadError}
    <ErrorNotice error={loadError} what="the planning queue" retry={load} />
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else}
    {#if active.length > 0}
      <div>
        <h2 class="mb-2 text-sm font-semibold text-[var(--color-muted)]">Active</h2>
        <div class="space-y-2">{#each active as p (p.node_id)}{@render card(p)}{/each}</div>
      </div>
    {/if}

    <div>
      <h2 class="mb-2 text-sm font-semibold text-[var(--color-muted)]">📌 Pinned</h2>
      <div
        class="min-h-[3rem] space-y-2 rounded border border-dashed border-[var(--color-border)] p-2"
        data-testid="pinned-zone"
        use:dndzone={{ items: pinnedBuf, flipDurationMs: flip, dropTargetStyle: {} }}
        onconsider={(e) => considerPinned(e as CustomEvent<DndEvent<DndItem>>)}
        onfinalize={(e) => finalizePinned(e as CustomEvent<DndEvent<DndItem>>)}
      >
        {#each pinnedBuf as item (item.id)}{@render card(item.proposal)}{:else}<p class="p-2 text-xs text-[var(--color-muted)]">Drag a proposal here to pin it.</p>{/each}
      </div>
    </div>

    <div>
      <h2 class="mb-2 text-sm font-semibold text-[var(--color-muted)]">Queue</h2>
      <div
        class="min-h-[3rem] space-y-2 rounded border border-dashed border-[var(--color-border)] p-2"
        data-testid="queue-zone"
        use:dndzone={{ items: queueBuf, flipDurationMs: flip, dropTargetStyle: {} }}
        onconsider={(e) => considerQueue(e as CustomEvent<DndEvent<DndItem>>)}
        onfinalize={(e) => finalizeQueue(e as CustomEvent<DndEvent<DndItem>>)}
      >
        {#each queueBuf as item (item.id)}{@render card(item.proposal)}{:else}<p class="p-2 text-xs text-[var(--color-muted)]">No proposals waiting. Ask your agent to propose some.</p>{/each}
      </div>
    </div>

    <div>
      <button class="text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]" onclick={() => (showDone = !showDone)}>{showDone ? "▾" : "▸"} Done ({done.length})</button>
      {#if showDone}<div class="mt-2 space-y-2">{#each done as p (p.node_id)}{@render card(p)}{/each}</div>{/if}
    </div>
    <div>
      <button class="text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]" onclick={() => (showDeclined = !showDeclined)}>{showDeclined ? "▾" : "▸"} Declined ({declined.length})</button>
      {#if showDeclined}<div class="mt-2 space-y-2">{#each declined as p (p.node_id)}{@render card(p)}{/each}</div>{/if}
    </div>
  {/if}
</section>

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

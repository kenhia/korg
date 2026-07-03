<script lang="ts">
  import { onMount } from "svelte";
  import { dndzone, type DndEvent } from "svelte-dnd-action";
  import { api, type Proposal, type WorkItem } from "$lib/api";

  type DndItem = { id: number; proposal: Proposal };
  type Covered = { wi_number: number; title: string };

  let proposalsRaw = $state<Proposal[]>([]);
  let workItems = $state<WorkItem[]>([]);
  let covers = $state<Record<number, Covered[]>>({});
  let loading = $state(true);
  let error = $state<string | null>(null);
  let copiedId = $state<number | null>(null);
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

  async function loadCovers(proposalNodeId: number) {
    const ns = await api.neighbors(proposalNodeId).catch(() => []);
    const items: Covered[] = [];
    for (const n of ns) {
      if (n.label !== "covers" || n.kind !== "workitem") continue;
      const wi = workItems.find((w) => w.node_id === n.node_id);
      if (wi) items.push({ wi_number: wi.wi_number, title: wi.title });
    }
    covers[proposalNodeId] = items;
  }

  async function load() {
    loading = true;
    error = null;
    try {
      [proposalsRaw, workItems] = await Promise.all([api.proposals(), api.workItems()]);
      rebuild();
      await Promise.all(proposalsRaw.map((p) => loadCovers(p.node_id)));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function midRank(prev?: string, next?: string): number {
    const p = prev !== undefined ? Number(prev) : undefined;
    const n = next !== undefined ? Number(next) : undefined;
    if (p !== undefined && n !== undefined) return (p + n) / 2;
    if (n !== undefined) return n - 1;
    if (p !== undefined) return p + 1;
    return 1;
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
      error = err instanceof Error ? err.message : String(err);
      await load();
    }
  }

  async function togglePin(p: Proposal) {
    try {
      await api.updateProposal(p.node_id, { pinned: !p.pinned });
      p.pinned = !p.pinned;
      rebuild();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  async function setStatus(p: Proposal, status: Proposal["status"]) {
    try {
      await api.updateProposal(p.node_id, { status });
      p.status = status;
      proposalsRaw = [...proposalsRaw];
      rebuild();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  async function copyStart(p: Proposal) {
    await navigator.clipboard.writeText(`/start-sprint korg:${p.node_id}`);
    copiedId = p.node_id;
    setTimeout(() => (copiedId = null), 1500);
  }

  onMount(load);
</script>

{#snippet card(p: Proposal)}
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
    <p class="mt-1 text-xs text-[var(--color-muted)]">{p.summary}</p>
    {#if covers[p.node_id]?.length}
      <div class="mt-2 flex flex-wrap gap-1">
        {#each covers[p.node_id] as c (c.wi_number)}
          <span class="rounded bg-black px-1.5 py-0.5 text-[10px] text-white" title={c.title}>#{c.wi_number} {c.title}</span>
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
    </div>
  </div>
{/snippet}

<section class="space-y-4">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">Planning</h1>
    <p class="text-xs text-[var(--color-muted)]">Proposals agents (or you) can drag to reorder — pinned always sort first.</p>
  </div>

  {#if error}<p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>{/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else}
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

    {#if active.length > 0}
      <div>
        <h2 class="mb-2 text-sm font-semibold text-[var(--color-muted)]">Active</h2>
        <div class="space-y-2">{#each active as p (p.node_id)}{@render card(p)}{/each}</div>
      </div>
    {/if}

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

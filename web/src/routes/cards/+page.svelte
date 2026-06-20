<script lang="ts">
  import { onMount } from "svelte";
  import { dndzone, type DndEvent } from "svelte-dnd-action";
  import {
    api,
    CARD_STATUSES,
    type Card,
    type CardStatus,
    type Slot,
    type CardComment,
  } from "$lib/api";
  import {
    startOfWeek,
    addDays,
    isoDate,
    weekDays,
    WEEKDAY_LABELS,
    prettyDuration,
  } from "$lib/dates";

  type DndItem = { id: number; card: Card };

  const NON_CUT = CARD_STATUSES.filter((s) => s !== "Cut");
  const STATUS_IDX = (s: string) => CARD_STATUSES.indexOf(s as CardStatus);
  let cutExpanded = $state(false);

  let cardsRaw = $state<Card[]>([]);
  let board = $state<Record<CardStatus, DndItem[]>>(emptyBoard());
  let loading = $state(true);
  let error = $state<string | null>(null);
  let view = $state<"board" | "list">("board");
  let newTitle = $state("");
  const flip = 150;

  // --- filters ---
  let fSearch = $state("");
  let fProject = $state("");
  let fCategory = $state("");
  let fTags = $state<Set<string>>(new Set());
  let showCut = $state(false);
  let showArchived = $state(false);

  const projOptions = $derived(
    [...new Set(cardsRaw.map((c) => c.project).filter((p): p is string => !!p))].sort(),
  );
  const catOptions = $derived(
    [...new Set(cardsRaw.map((c) => c.category).filter((c): c is string => !!c))].sort(),
  );
  const tagOptions = $derived([...new Set(cardsRaw.flatMap((c) => c.tags))].sort());

  function passesCommon(c: Card): boolean {
    if (!showArchived && c.archived) return false;
    if (fProject && c.project !== fProject) return false;
    if (fCategory && c.category !== fCategory) return false;
    for (const t of fTags) if (!c.tags.includes(t)) return false;
    if (fSearch.trim() !== "") {
      const q = fSearch.toLowerCase();
      if (!(c.title.toLowerCase().includes(q) || c.description.toLowerCase().includes(q)))
        return false;
    }
    return true;
  }

  const listCards = $derived(
    cardsRaw
      .filter((c) => passesCommon(c) && (showCut || c.status !== "Cut"))
      .sort((a, b) => STATUS_IDX(a.status) - STATUS_IDX(b.status) || Number(a.rank) - Number(b.rank)),
  );

  function resetFilters() {
    fSearch = "";
    fProject = "";
    fCategory = "";
    fTags = new Set();
    showCut = false;
    showArchived = false;
    rebuild();
  }
  function toggleTag(t: string) {
    const s = new Set(fTags);
    if (s.has(t)) s.delete(t);
    else s.add(t);
    fTags = s;
    rebuild();
  }

  function emptyBoard(): Record<CardStatus, DndItem[]> {
    const b = {} as Record<CardStatus, DndItem[]>;
    for (const s of CARD_STATUSES) b[s] = [];
    return b;
  }
  function rebuild() {
    const b = emptyBoard();
    for (const c of cardsRaw) {
      if (!passesCommon(c)) continue;
      b[c.status].push({ id: c.node_id, card: c });
    }
    for (const s of CARD_STATUSES) b[s].sort((x, y) => Number(x.card.rank) - Number(y.card.rank));
    board = b;
  }

  async function load() {
    loading = true;
    error = null;
    try {
      cardsRaw = await api.cards();
      rebuild();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function add() {
    if (newTitle.trim() === "") return;
    await api.createCard({ title: newTitle.trim() });
    newTitle = "";
    await load();
  }

  function consider(status: CardStatus, e: CustomEvent<DndEvent<DndItem>>) {
    board[status] = e.detail.items;
  }
  async function finalize(status: CardStatus, e: CustomEvent<DndEvent<DndItem>>) {
    board[status] = e.detail.items;
    const movedId = e.detail.info.id;
    const items = board[status];
    const idx = items.findIndex((it) => String(it.id) === String(movedId));
    if (idx < 0) return;
    const rank = midRank(items[idx - 1]?.card.rank, items[idx + 1]?.card.rank);
    const moved = items[idx].card;
    try {
      await api.updateCard(moved.node_id, { status, rank });
      moved.status = status;
      moved.rank = String(rank);
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
      await load();
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

  // --- this-week slots strip ---
  let showSlots = $state(true);
  let weekStart = $state(startOfWeek(new Date()));
  let slots = $state<Slot[]>([]);
  let scheduled = $state<Record<number, { node_id: number; title: string }[]>>({});
  let slotBuf = $state<Record<number, DndItem[]>>({});
  const days = $derived(weekDays(weekStart));
  function slotsForDay(day: Date): Slot[] {
    const key = isoDate(day);
    return slots.filter((s) => s.slot_date === key).sort((a, b) => a.position - b.position);
  }
  function cardTitle(nodeId: number): string {
    return cardsRaw.find((c) => c.node_id === nodeId)?.title ?? `#${nodeId}`;
  }
  async function loadSlots() {
    const from = isoDate(weekStart);
    const to = isoDate(addDays(weekStart, 6));
    const fetched = await api.slots(from, to);
    const sched: Record<number, { node_id: number; title: string }[]> = {};
    const buf: Record<number, DndItem[]> = {};
    await Promise.all(
      fetched.map(async (s) => {
        buf[s.node_id] = [];
        const ns = await api.neighbors(s.node_id);
        sched[s.node_id] = ns
          .filter((n) => n.kind === "card")
          .map((n) => ({ node_id: n.node_id, title: cardTitle(n.node_id) }));
      }),
    );
    slots = fetched;
    scheduled = sched;
    slotBuf = buf;
  }
  async function generateWeek() {
    await api.generateSlots(isoDate(weekStart), 7);
    await loadSlots();
  }
  function shiftWeek(delta: number) {
    weekStart = addDays(weekStart, delta * 7);
    loadSlots();
  }
  function slotConsider(slotId: number, e: CustomEvent<DndEvent<DndItem>>) {
    slotBuf[slotId] = e.detail.items;
  }
  async function slotFinalize(slotId: number, e: CustomEvent<DndEvent<DndItem>>) {
    const dropped = e.detail.items[0];
    slotBuf[slotId] = [];
    if (!dropped) return;
    const cardNode = dropped.card.node_id;
    const already = (scheduled[slotId] ?? []).some((c) => c.node_id === cardNode);
    try {
      if (!already) await api.relate(slotId, cardNode, "scheduled");
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
    await load();
    await loadSlots();
  }

  // --- edit modal (+ comments) ---
  let editing = $state<Card | null>(null);
  let form = $state({ title: "", status: "Backlog" as CardStatus, project: "", category: "", description: "", tags: "" });
  let original = $state("");
  let showDiscard = $state(false);
  let comments = $state<CardComment[]>([]);
  let newComment = $state("");
  const dirty = $derived(editing !== null && JSON.stringify(form) !== original);

  async function openEdit(card: Card) {
    editing = card;
    form = {
      title: card.title,
      status: card.status,
      project: card.project ?? "",
      category: card.category ?? "",
      description: card.description,
      tags: card.tags.join(", "),
    };
    original = JSON.stringify(form);
    showDiscard = false;
    comments = [];
    newComment = "";
    comments = await api.cardComments(card.node_id).catch(() => []);
  }
  function requestClose() {
    if (dirty) showDiscard = true;
    else editing = null;
  }
  function discard() {
    showDiscard = false;
    editing = null;
  }
  async function saveEdit() {
    if (!editing) return;
    const tags = form.tags.split(",").map((t) => t.trim()).filter((t) => t !== "");
    await api.updateCard(editing.node_id, {
      title: form.title,
      status: form.status,
      description: form.description,
      project: form.project.trim() === "" ? null : form.project.trim(),
      category: form.category.trim() === "" ? null : form.category.trim(),
      tags,
    });
    editing = null;
    await load();
  }
  async function toggleArchiveCard() {
    if (!editing) return;
    await api.updateCard(editing.node_id, { archived: !editing.archived });
    editing.archived = !editing.archived;
    await load();
  }
  async function addComment() {
    if (!editing || newComment.trim() === "") return;
    const c = await api.addComment(editing.node_id, newComment.trim());
    comments = [...comments, c];
    newComment = "";
  }
  async function removeComment(id: number) {
    await api.deleteComment(id);
    comments = comments.filter((c) => c.id !== id);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && editing) requestClose();
  }

  onMount(async () => {
    await load();
    await loadSlots();
  });
</script>

<svelte:window onkeydown={onKey} />

{#snippet tile(item: DndItem)}
  <div
    class="cursor-grab rounded bg-[var(--color-surface-hi)] p-2 active:cursor-grabbing"
    data-testid={`card-${item.id}`}
    onclick={() => openEdit(item.card)}
    role="button"
    tabindex="0"
    onkeydown={(e) => e.key === "Enter" && openEdit(item.card)}
  >
    <div class="text-sm">{item.card.title}</div>
    {#if item.card.project || item.card.category}
      <div class="mt-1 flex flex-wrap gap-1">
        {#if item.card.project}<span class="rounded bg-teal-900/60 px-1.5 py-0.5 text-xs font-medium text-teal-300" title="Project">{item.card.project}</span>{/if}
        {#if item.card.category}<span class="rounded bg-purple-800/80 px-1.5 py-0.5 text-xs font-medium text-white" title="Category">{item.card.category}</span>{/if}
      </div>
    {/if}
    {#if item.card.tags.length > 0}
      <div class="mt-1 flex flex-wrap gap-1">
        {#each item.card.tags as tag (tag)}<span class="rounded bg-black px-1.5 py-0.5 text-xs text-white">{tag}</span>{/each}
      </div>
    {/if}
  </div>
{/snippet}

<section class="space-y-4">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">Cards</h1>
    <div class="flex overflow-hidden rounded border border-[var(--color-border)] text-sm">
      <button class="px-3 py-1" class:bg-[var(--color-surface-hi)]={view === "board"} onclick={() => (view = "board")}>Board</button>
      <button class="px-3 py-1" class:bg-[var(--color-surface-hi)]={view === "list"} onclick={() => (view = "list")}>List</button>
    </div>
  </div>

  <div class="flex items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
    <input class="flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="New card title…" bind:value={newTitle} onkeydown={(e) => e.key === "Enter" && add()} />
    <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]" onclick={add}>Add</button>
  </div>

  {#if error}<p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>{/if}

  <!-- This week's timeboxes (drop a card to schedule; card stays in its column) -->
  <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
    <div class="flex items-center justify-between px-3 py-2">
      <button class="flex items-center gap-2 text-sm font-medium hover:text-[var(--color-accent)]" data-testid="slots-toggle" onclick={() => (showSlots = !showSlots)}>
        <span>{showSlots ? "▾" : "▸"}</span> This week
      </button>
      {#if showSlots}
        <div class="flex items-center gap-1 text-xs">
          <button class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]" onclick={() => shiftWeek(-1)}>← Prev</button>
          <button class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]" onclick={() => { weekStart = startOfWeek(new Date()); loadSlots(); }}>Today</button>
          <button class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]" onclick={() => shiftWeek(1)}>Next →</button>
        </div>
      {/if}
    </div>
    {#if showSlots}
      {#if slots.length === 0}
        <div class="px-3 pb-3 text-sm text-[var(--color-muted)]">No timeboxes this week. <button class="ml-2 rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={generateWeek}>Generate</button></div>
      {:else}
        <div class="grid grid-cols-2 gap-2 p-3 sm:grid-cols-4 lg:grid-cols-7">
          {#each days as day, i (isoDate(day))}
            <div class="rounded bg-[var(--color-bg)] p-2">
              <div class="mb-1 text-xs text-[var(--color-muted)]">{WEEKDAY_LABELS[i]} {day.getDate()}</div>
              <div class="space-y-1">
                {#each slotsForDay(day) as slot (slot.node_id)}
                  <div class="rounded bg-[var(--color-surface)] p-1.5">
                    <div class="mb-1 flex items-center justify-between">
                      <span class="text-xs text-[var(--color-accent)]">{slot.label ?? prettyDuration(slot.duration_minutes)}</span>
                      {#if slot.goal}<span class="truncate text-[10px] text-[var(--color-muted)]">{slot.goal}</span>{/if}
                    </div>
                    <div
                      class="min-h-[2rem] rounded border border-dashed border-[var(--color-border)] px-1 py-0.5 text-center text-[9px] text-[var(--color-muted)]"
                      data-testid={`slot-${slot.node_id}`}
                      use:dndzone={{ items: slotBuf[slot.node_id] ?? [], flipDurationMs: flip, dropFromOthersDisabled: false, dropTargetStyle: { outline: "2px dashed var(--color-accent)" } }}
                      onconsider={(e) => slotConsider(slot.node_id, e as CustomEvent<DndEvent<DndItem>>)}
                      onfinalize={(e) => slotFinalize(slot.node_id, e as CustomEvent<DndEvent<DndItem>>)}
                    >
                      {#each slotBuf[slot.node_id] ?? [] as it (it.id)}<div class="rounded bg-[var(--color-surface-hi)] px-1 text-[10px]">{it.card.title}</div>{:else}drop card{/each}
                    </div>
                    {#each scheduled[slot.node_id] ?? [] as c (c.node_id)}<div class="mt-1 truncate rounded bg-[var(--color-accent-soft)] px-1 py-0.5 text-[10px]" data-testid={`sched-${slot.node_id}`}>{c.title}</div>{/each}
                  </div>
                {/each}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else if view === "board"}
          <p class="mb-2 text-xs text-[var(--color-muted)]">Drag cards within or across columns — drop anywhere to set order.</p>
          <div class="flex flex-wrap gap-3">
            {#each NON_CUT as status (status)}
              <div class="min-w-[8rem] flex-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
                <div class="mb-2 flex items-center justify-between">
                  <span class="text-xs font-medium text-[var(--color-muted)]">{status}</span>
                  <span class="text-xs text-[var(--color-muted)]">{board[status].length}</span>
                </div>
                <div class="min-h-[3rem] space-y-2" data-testid={`col-${status}`} use:dndzone={{ items: board[status], flipDurationMs: flip, dropTargetStyle: {} }} onconsider={(e) => consider(status, e as CustomEvent<DndEvent<DndItem>>)} onfinalize={(e) => finalize(status, e as CustomEvent<DndEvent<DndItem>>)}>
                  {#each board[status] as item (item.id)}{@render tile(item)}{/each}
                </div>
              </div>
            {/each}
            <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 transition-all" class:flex-1={cutExpanded} class:min-w-[8rem]={cutExpanded} class:w-12={!cutExpanded}>
              <button class="mb-2 flex w-full items-center justify-between gap-1 text-xs font-medium text-[var(--color-muted)] hover:text-[var(--color-text)]" data-testid="cut-toggle" onclick={() => (cutExpanded = !cutExpanded)} title={cutExpanded ? "Collapse Cut" : "Expand Cut"}>
                <span>{cutExpanded ? "Cut" : "CUT"}</span><span>{board["Cut"].length}</span>
              </button>
              <div class="min-h-[3rem] space-y-2" class:overflow-hidden={!cutExpanded} data-testid="col-Cut" use:dndzone={{ items: board["Cut"], flipDurationMs: flip, dropTargetStyle: {} }} onconsider={(e) => consider("Cut", e as CustomEvent<DndEvent<DndItem>>)} onfinalize={(e) => finalize("Cut", e as CustomEvent<DndEvent<DndItem>>)}>
                {#each board["Cut"] as item (item.id)}<div class:hidden={!cutExpanded}>{@render tile(item)}</div>{/each}
              </div>
            </div>
          </div>
        {:else}
          <div class="grid gap-4 lg:grid-cols-[15rem_1fr]">
            {@render filtersPanel()}
            <div class="overflow-auto rounded border border-[var(--color-border)]">
            <table class="w-full text-sm">
              <thead class="sticky top-0 bg-[var(--color-surface)] text-left text-xs text-[var(--color-muted)]">
                <tr><th class="px-3 py-2">Title</th><th class="px-3 py-2">Status</th><th class="px-3 py-2">Project</th><th class="px-3 py-2">Category</th><th class="px-3 py-2">Tags</th><th class="px-3 py-2">Updated</th></tr>
              </thead>
              <tbody>
                {#each listCards as card (card.node_id)}
                  <tr class="cursor-pointer border-t border-[var(--color-border)] hover:bg-[var(--color-surface-hi)]" class:opacity-55={card.archived} tabindex="0" onclick={() => openEdit(card)} onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), openEdit(card))}>
                    <td class="px-3 py-1.5 font-medium">{card.title}</td>
                    <td class="px-3 py-1.5">{card.status}</td>
                    <td class="px-3 py-1.5">{#if card.project}<span class="rounded bg-teal-900/60 px-1.5 py-0.5 text-xs text-teal-300">{card.project}</span>{/if}</td>
                    <td class="px-3 py-1.5">{#if card.category}<span class="rounded bg-purple-800/80 px-1.5 py-0.5 text-xs text-white">{card.category}</span>{/if}</td>
                    <td class="px-3 py-1.5"><div class="flex flex-wrap gap-1">{#each card.tags as t (t)}<span class="rounded bg-black px-1.5 py-0.5 text-xs text-white">{t}</span>{/each}</div></td>
                    <td class="px-3 py-1.5 text-[var(--color-muted)]">{new Date(card.updated).toLocaleDateString()}</td>
                  </tr>
                {:else}
                  <tr><td class="px-3 py-3 text-sm text-[var(--color-muted)]" colspan="6">No cards.</td></tr>
                {/each}
              </tbody>
            </table>
            </div>
          </div>
        {/if}

  {#snippet filtersPanel()}
      <aside class="space-y-3 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm">
        <div>
          <label class="mb-1 block text-xs text-[var(--color-muted)]" for="card-search">Search</label>
          <input id="card-search" type="search" data-testid="filter-search" class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" placeholder="title or description…" bind:value={fSearch} oninput={rebuild} />
        </div>
        <div>
          <label class="mb-1 block text-xs text-[var(--color-muted)]" for="card-project">Project</label>
          <select id="card-project" class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={fProject} onchange={rebuild}>
            <option value="">— any —</option>
            {#each projOptions as p (p)}<option value={p}>{p}</option>{/each}
          </select>
        </div>
        <div>
          <label class="mb-1 block text-xs text-[var(--color-muted)]" for="card-category">Category</label>
          <select id="card-category" class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={fCategory} onchange={rebuild}>
            <option value="">— any —</option>
            {#each catOptions as c (c)}<option value={c}>{c}</option>{/each}
          </select>
        </div>
        <fieldset class="border-0 p-0">
          <legend class="mb-1 text-xs text-[var(--color-muted)]">Tags (AND)</legend>
          <div class="flex flex-wrap gap-1" data-testid="filter-tags">
            {#each tagOptions as t (t)}
              <button type="button" class="rounded border px-2 py-0.5 text-xs" class:border-[var(--color-accent)]={fTags.has(t)} class:bg-[var(--color-accent-soft)]={fTags.has(t)} class:border-[var(--color-border)]={!fTags.has(t)} onclick={() => toggleTag(t)}>{t}</button>
            {:else}<span class="text-xs text-[var(--color-muted)]">no tags yet</span>{/each}
          </div>
        </fieldset>
        <label class="flex items-center gap-2 text-xs text-[var(--color-muted)]"><input type="checkbox" data-testid="toggle-cut" bind:checked={showCut} onchange={rebuild} /> Show Cut</label>
        <label class="flex items-center gap-2 text-xs text-[var(--color-muted)]"><input type="checkbox" data-testid="toggle-archived" bind:checked={showArchived} onchange={rebuild} /> Show Archived</label>
        <button type="button" class="w-full rounded border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]" onclick={resetFilters}>Reset filters</button>
      </aside>
  {/snippet}

  {#if editing}
    <div class="fixed inset-0 z-50 flex items-start justify-center overflow-auto p-4">
      <button class="absolute inset-0 bg-black/60" aria-label="Close" onclick={requestClose}></button>
      <div class="relative z-10 mt-8 w-full max-w-lg space-y-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl" data-testid="card-modal">
        <div class="flex items-center justify-between">
          <h2 class="text-lg font-semibold">Edit card {#if editing.archived}<span class="ml-2 rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]">archived</span>{/if}</h2>
          <button class="rounded px-2 py-1 text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)]" aria-label="Close" data-testid="modal-close" onclick={requestClose}>✕</button>
        </div>

        <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="Title" data-testid="edit-title" bind:value={form.title} />
        <div class="grid grid-cols-2 gap-2 text-sm">
          <label class="flex flex-col gap-1"><span class="text-xs text-[var(--color-muted)]">Status</span>
            <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={form.status}>{#each CARD_STATUSES as s (s)}<option value={s}>{s}</option>{/each}</select>
          </label>
          <label class="flex flex-col gap-1"><span class="text-xs text-[var(--color-muted)]">Project</span>
            <input class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={form.project} />
          </label>
          <label class="flex flex-col gap-1"><span class="text-xs text-[var(--color-muted)]">Category</span>
            <input class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" bind:value={form.category} />
          </label>
          <label class="flex flex-col gap-1"><span class="text-xs text-[var(--color-muted)]">Tags</span>
            <input class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none" placeholder="comma, separated" bind:value={form.tags} />
          </label>
        </div>
        <textarea class="min-h-[8rem] w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="Description (markdown)" bind:value={form.description}></textarea>

        <div class="flex items-center justify-between gap-2">
          <button class="rounded border border-[var(--color-border)] px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]" onclick={toggleArchiveCard}>{editing.archived ? "Unarchive" : "Archive"}</button>
          <div class="flex items-center gap-2">
            {#if dirty}<span class="text-xs text-[var(--color-muted)]">Unsaved</span>{/if}
            <button class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]" onclick={requestClose}>Close</button>
            <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]" onclick={saveEdit}>Save</button>
          </div>
        </div>

        <!-- comments -->
        <div class="border-t border-[var(--color-border)] pt-2">
          <p class="mb-1 text-xs font-semibold text-[var(--color-muted)]">Comments</p>
          <ul class="space-y-1" data-testid="comment-list">
            {#each comments as c (c.id)}
              <li class="flex items-start gap-2 rounded bg-[var(--color-bg)] px-2 py-1 text-sm">
                <span class="flex-1 whitespace-pre-wrap">{c.body}</span>
                <button class="text-xs text-[var(--color-muted)] hover:text-red-400" aria-label="Delete comment" onclick={() => removeComment(c.id)}>✕</button>
              </li>
            {:else}<li class="text-xs text-[var(--color-muted)]">No comments.</li>{/each}
          </ul>
          <div class="mt-2 flex gap-2">
            <input class="flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="Add a comment…" data-testid="comment-input" bind:value={newComment} onkeydown={(e) => e.key === "Enter" && addComment()} />
            <button class="rounded bg-[var(--color-surface-hi)] px-3 py-1 text-sm hover:bg-[var(--color-accent-soft)]" onclick={addComment}>Add</button>
          </div>
        </div>

        {#if showDiscard}
          <div class="rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-3" data-testid="discard-prompt">
            <p class="mb-2 text-sm">Discard unsaved changes?</p>
            <div class="flex justify-end gap-2">
              <button class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]" onclick={() => (showDiscard = false)}>Keep editing</button>
              <button class="rounded bg-red-900 px-3 py-1.5 text-sm hover:bg-red-800" data-testid="discard-confirm" onclick={discard}>Discard</button>
            </div>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</section>

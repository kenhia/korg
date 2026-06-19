<script lang="ts">
  import { onMount } from "svelte";
  import { dndzone, type DndEvent } from "svelte-dnd-action";
  import { api, CARD_STATUSES, type Card, type CardStatus } from "$lib/api";

  type DndItem = { id: number; card: Card };

  let cardsRaw = $state<Card[]>([]);
  let board = $state<Record<CardStatus, DndItem[]>>(emptyBoard());
  let loading = $state(true);
  let error = $state<string | null>(null);
  let view = $state<"board" | "list">("board");
  let newTitle = $state("");

  // --- edit modal with dirty-close guard ---
  let editing = $state<Card | null>(null);
  let form = $state({ title: "", description: "", status: "Backlog" as CardStatus, tags: "" });
  let original = $state("");
  let showDiscard = $state(false);
  const dirty = $derived(editing !== null && JSON.stringify(form) !== original);

  function openEdit(card: Card) {
    editing = card;
    form = {
      title: card.title,
      description: card.description,
      status: card.status,
      tags: card.tags.join(", "),
    };
    original = JSON.stringify(form);
    showDiscard = false;
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
    const tags = form.tags
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t !== "");
    await api.updateCard(editing.node_id, {
      title: form.title,
      description: form.description,
      status: form.status,
      tags,
    });
    editing = null;
    await load();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && editing) requestClose();
  }

  const flip = 150;

  function emptyBoard(): Record<CardStatus, DndItem[]> {
    const b = {} as Record<CardStatus, DndItem[]>;
    for (const s of CARD_STATUSES) b[s] = [];
    return b;
  }

  function rebuild() {
    const b = emptyBoard();
    for (const c of cardsRaw) {
      if (!c.archived) b[c.status].push({ id: c.node_id, card: c });
    }
    for (const s of CARD_STATUSES) {
      b[s].sort((x, y) => Number(x.card.rank) - Number(y.card.rank));
    }
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
    if (idx < 0) return; // moved out of this column; the destination persists it
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

  // A fractional rank that lands the card exactly where it was dropped.
  function midRank(prev?: string, next?: string): number {
    const p = prev !== undefined ? Number(prev) : undefined;
    const n = next !== undefined ? Number(next) : undefined;
    if (p !== undefined && n !== undefined) return (p + n) / 2;
    if (n !== undefined) return n - 1;
    if (p !== undefined) return p + 1;
    return 1;
  }

  onMount(load);
</script>

<svelte:window onkeydown={onKey} />

<section class="space-y-4">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">Cards</h1>
    <div class="flex overflow-hidden rounded border border-[var(--color-border)] text-sm">
      <button class="px-3 py-1" class:bg-[var(--color-surface-hi)]={view === "board"} onclick={() => (view = "board")}>Board</button>
      <button class="px-3 py-1" class:bg-[var(--color-surface-hi)]={view === "list"} onclick={() => (view = "list")}>List</button>
    </div>
  </div>

  <div class="flex items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
    <input
      class="flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="New card title…"
      bind:value={newTitle}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]" onclick={add}>Add</button>
  </div>

  {#if error}
    <p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else if view === "board"}
    <p class="text-xs text-[var(--color-muted)]">Drag cards within or across columns — drop anywhere to set order.</p>
    <div class="grid grid-cols-2 gap-3 lg:grid-cols-6">
      {#each CARD_STATUSES as status (status)}
        <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
          <div class="mb-2 flex items-center justify-between">
            <span class="text-xs font-medium text-[var(--color-muted)]">{status}</span>
            <span class="text-xs text-[var(--color-muted)]">{board[status].length}</span>
          </div>
          <div
            class="min-h-[3rem] space-y-2"
            data-testid={`col-${status}`}
            use:dndzone={{ items: board[status], flipDurationMs: flip, dropTargetStyle: {} }}
            onconsider={(e) => consider(status, e as CustomEvent<DndEvent<DndItem>>)}
            onfinalize={(e) => finalize(status, e as CustomEvent<DndEvent<DndItem>>)}
          >
            {#each board[status] as item (item.id)}
              <div
                class="cursor-grab rounded bg-[var(--color-surface-hi)] p-2 active:cursor-grabbing"
                data-testid={`card-${item.id}`}
                onclick={() => openEdit(item.card)}
                role="button"
                tabindex="0"
                onkeydown={(e) => e.key === "Enter" && openEdit(item.card)}
              >
                <div class="text-sm">{item.card.title}</div>
                {#if item.card.tags.length > 0}
                  <div class="mt-1 flex flex-wrap gap-1">
                    {#each item.card.tags as tag (tag)}
                      <span class="rounded bg-[var(--color-bg)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">#{tag}</span>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <table class="w-full text-sm">
      <thead class="text-left text-xs text-[var(--color-muted)]">
        <tr>
          <th class="px-2 py-1">Title</th>
          <th class="px-2 py-1">Status</th>
          <th class="px-2 py-1">Project</th>
        </tr>
      </thead>
      <tbody>
        {#each cardsRaw.filter((c) => !c.archived) as card (card.node_id)}
          <tr class="border-t border-[var(--color-border)]">
            <td class="px-2 py-1.5">{card.title}</td>
            <td class="px-2 py-1.5">
              <select
                class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs outline-none"
                value={card.status}
                onchange={async (e) => {
                  await api.updateCard(card.node_id, { status: e.currentTarget.value as CardStatus });
                  await load();
                }}
              >
                {#each CARD_STATUSES as s (s)}
                  <option value={s}>{s}</option>
                {/each}
              </select>
            </td>
            <td class="px-2 py-1.5 text-[var(--color-muted)]">{card.project ?? "—"}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  {#if editing}
    <div class="fixed inset-0 z-50 flex items-center justify-center p-4">
      <button
        class="absolute inset-0 bg-black/60"
        aria-label="Close"
        onclick={requestClose}
      ></button>
      <div
        class="relative z-10 w-full max-w-lg space-y-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl"
        data-testid="card-modal"
      >
        <div class="flex items-center justify-between">
          <h2 class="text-lg font-semibold">Edit card</h2>
          <button
            class="rounded px-2 py-1 text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)]"
            aria-label="Close"
            data-testid="modal-close"
            onclick={requestClose}>✕</button>
        </div>

        <input
          class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
          placeholder="Title"
          data-testid="edit-title"
          bind:value={form.title}
        />
        <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" bind:value={form.status}>
          {#each CARD_STATUSES as s (s)}<option value={s}>{s}</option>{/each}
        </select>
        <textarea
          class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
          rows="5"
          placeholder="Description"
          bind:value={form.description}
        ></textarea>
        <input
          class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none"
          placeholder="tags, comma, separated"
          bind:value={form.tags}
        />

        <div class="flex items-center justify-end gap-2">
          {#if dirty}<span class="mr-auto text-xs text-[var(--color-muted)]">Unsaved changes</span>{/if}
          <button class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]" onclick={requestClose}>Close</button>
          <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]" onclick={saveEdit}>Save</button>
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

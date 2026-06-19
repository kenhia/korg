<script lang="ts">
  import { onMount } from "svelte";
  import { api, CARD_STATUSES, type Card, type CardStatus } from "$lib/api";

  let cards = $state<Card[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let view = $state<"board" | "list">("board");
  let newTitle = $state("");

  function inStatus(status: CardStatus): Card[] {
    return cards
      .filter((c) => c.status === status && !c.archived)
      .sort((a, b) => Number(a.rank) - Number(b.rank));
  }

  async function load() {
    loading = true;
    error = null;
    try {
      cards = await api.cards();
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

  async function moveTo(card: Card, status: CardStatus) {
    await api.updateCard(card.node_id, { status });
    card.status = status;
  }

  onMount(load);
</script>

<section class="space-y-4">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">Cards</h1>
    <div class="flex items-center gap-2 text-sm">
      <div class="flex overflow-hidden rounded border border-[var(--color-border)]">
        <button
          class="px-3 py-1"
          class:bg-[var(--color-surface-hi)]={view === "board"}
          onclick={() => (view = "board")}>Board</button>
        <button
          class="px-3 py-1"
          class:bg-[var(--color-surface-hi)]={view === "list"}
          onclick={() => (view = "list")}>List</button>
      </div>
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
    <div class="grid grid-cols-2 gap-3 lg:grid-cols-6">
      {#each CARD_STATUSES as status (status)}
        <div class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
          <div class="mb-2 flex items-center justify-between">
            <span class="text-xs font-medium text-[var(--color-muted)]">{status}</span>
            <span class="text-xs text-[var(--color-muted)]">{inStatus(status).length}</span>
          </div>
          <div class="space-y-2">
            {#each inStatus(status) as card (card.node_id)}
              <div class="rounded bg-[var(--color-surface-hi)] p-2">
                <div class="text-sm">{card.title}</div>
                {#if card.tags.length > 0}
                  <div class="mt-1 flex flex-wrap gap-1">
                    {#each card.tags as tag (tag)}
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
        {#each cards.filter((c) => !c.archived) as card (card.node_id)}
          <tr class="border-t border-[var(--color-border)]">
            <td class="px-2 py-1.5">{card.title}</td>
            <td class="px-2 py-1.5">
              <select
                class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs outline-none"
                value={card.status}
                onchange={(e) => moveTo(card, e.currentTarget.value as CardStatus)}
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
</section>

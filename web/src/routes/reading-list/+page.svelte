<script lang="ts">
  import { onMount } from "svelte";
  import { api, DISPOSITIONS, type Disposition, type Link } from "$lib/api";

  let links = $state<Link[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let editing = $state<number | null>(null);

  let newUrl = $state("");
  let newTitle = $state("");

  async function load() {
    loading = true;
    error = null;
    try {
      links = await api.links();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function add() {
    if (newUrl.trim() === "") return;
    await api.createLink({ url: newUrl.trim(), title: newTitle.trim() || undefined });
    newUrl = "";
    newTitle = "";
    await load();
  }

  async function setDisposition(link: Link, d: Disposition) {
    await api.updateLink(link.node_id, { disposition: d });
    link.disposition = d;
  }

  async function saveTags(link: Link, value: string) {
    const tags = value
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t !== "");
    await api.updateLink(link.node_id, { tags });
    link.tags = tags;
  }

  // Reserve plain click for editing; ctrl/cmd-click opens the URL.
  function onTitleClick(e: MouseEvent, link: Link) {
    if (e.ctrlKey || e.metaKey) {
      window.open(link.url, "_blank", "noopener,noreferrer");
      return;
    }
    e.preventDefault();
    editing = editing === link.node_id ? null : link.node_id;
  }

  onMount(load);
</script>

<section class="space-y-4">
  <h1 class="text-xl font-semibold">Reading list</h1>
  <p class="text-xs text-[var(--color-muted)]">
    Click a title to edit · Ctrl/⌘-click to open in a new tab
  </p>

  <div class="flex flex-wrap items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
    <input
      class="min-w-[16rem] flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="https://…"
      bind:value={newUrl}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <input
      class="w-48 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="title (optional)"
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
  {:else if links.length === 0}
    <p class="text-sm text-[var(--color-muted)]">Empty.</p>
  {:else}
    <ul class="divide-y divide-[var(--color-border)] rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
      {#each links as link (link.node_id)}
        <li class="px-3 py-2">
          <div class="flex items-center justify-between gap-3">
            <a
              href={link.url}
              class="truncate text-sm text-[var(--color-accent)] hover:underline"
              onclick={(e) => onTitleClick(e, link)}
            >
              {link.title ?? link.url}
            </a>
            <span
              class="shrink-0 rounded px-1.5 py-0.5 text-xs"
              class:bg-[var(--color-surface-hi)]={link.disposition !== "Done"}
              class:text-[var(--color-muted)]={link.disposition !== "Done"}
              class:bg-[var(--color-accent-soft)]={link.disposition === "Done"}
            >
              {link.disposition}
            </span>
          </div>

          {#if editing === link.node_id}
            <div class="mt-2 space-y-2">
              <div class="flex flex-wrap gap-1">
                {#each DISPOSITIONS as d (d)}
                  <button
                    class="rounded px-2 py-0.5 text-xs hover:bg-[var(--color-surface-hi)]"
                    class:bg-[var(--color-accent-soft)]={link.disposition === d}
                    onclick={() => setDisposition(link, d)}
                  >
                    {d}
                  </button>
                {/each}
              </div>
              <input
                class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none"
                placeholder="tags, comma, separated"
                value={link.tags.join(", ")}
                onblur={(e) => saveTags(link, e.currentTarget.value)}
              />
            </div>
          {:else if link.tags.length > 0}
            <div class="mt-1 flex flex-wrap gap-1">
              {#each link.tags as tag (tag)}
                <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs text-[var(--color-muted)]">#{tag}</span>
              {/each}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

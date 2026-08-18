<script lang="ts">
  import { onMount } from "svelte";
  import { api, type LinkRow } from "$lib/api";
  import { LINK_DISPOSITIONS, type Disposition } from "$lib/generated/vocab";
  import { ID_CLASS, chip } from "$lib/domain";
  import { attempt } from "$lib/toast.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";

  let links = $state<LinkRow[]>([]);
  let loading = $state(true);
  let loadError = $state<unknown>(null);
  let editing = $state<number | null>(null);
  let previewNode = $state<number | null>(null);

  let newUrl = $state("");
  let newTitle = $state("");

  async function load() {
    loading = true;
    loadError = null;
    try {
      links = (await api.links()).items;
    } catch (e) {
      loadError = e;
    } finally {
      loading = false;
    }
  }

  async function add() {
    if (newUrl.trim() === "") return;
    const url = newUrl.trim();
    const title = newTitle.trim() || undefined;
    const created = await attempt(
      () => api.createLink({ url, title }),
      "Add link",
    );
    if (!created) return;
    newUrl = "";
    newTitle = "";
    await load();
  }

  async function setDisposition(link: LinkRow, d: Disposition) {
    const updated = await attempt(
      () => api.updateLink(link.node_id, { disposition: d }),
      "Set disposition",
    );
    if (updated) link.disposition = d;
  }

  async function saveTags(link: LinkRow, value: string) {
    const tags = value
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t !== "");
    const updated = await attempt(
      () => api.updateLink(link.node_id, { tags }),
      "Save tags",
    );
    if (updated) link.tags = tags;
  }

  onMount(load);
</script>

<section class="space-y-4">
  <h1 class="text-xl font-semibold">Reading list</h1>
  <p class="text-xs text-[var(--color-muted)]">
    A title opens the link · the id opens its detail and comments · use Edit to
    change disposition and tags
  </p>

  <div class="flex flex-wrap items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
    <label class="sr-only" for="new-link-url">Link URL</label>
    <input
      id="new-link-url"
      type="url"
      class="min-w-[16rem] flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="https://…"
      bind:value={newUrl}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <label class="sr-only" for="new-link-title">Link title (optional)</label>
    <input
      id="new-link-title"
      class="w-48 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="title (optional)"
      bind:value={newTitle}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]" onclick={add}>Add</button>
  </div>

  {#if loadError}
    <ErrorNotice error={loadError} what="the reading list" retry={load} />
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else if links.length === 0}
    <p class="text-sm text-[var(--color-muted)]">Empty.</p>
  {:else}
    <ul class="divide-y divide-[var(--color-border)] rounded border border-[var(--color-border)] bg-[var(--color-surface)]">
      {#each links as link (link.node_id)}
        <li class="px-3 py-2">
          <!-- No `justify-between` (WI #1115). Each child sized to its content
               and the leftover width was spread *between* all four, so a row's
               Edit button landed wherever that row's title length left it — a
               different x on every row. The anchor absorbs the slack instead
               (below), which leaves nothing to distribute. -->
          <div class="flex items-center gap-3">
            <!-- #980: a link is a node an agent cites by id (mark_link_read,
                 update_link both take node_id), so the id is on the row.
                 #1107: and now it opens the link's preview, which is where its
                 comments live. A link is the kind where a note ("skimmed, the
                 useful part is §3") is most obviously wanted and the one kind
                 with nowhere to put one. The id is the affordance for the same
                 reason it is on Schedules — it is what find-by-ID takes — and
                 it leaves the title doing the one thing a reading list exists
                 for, which is following the link (#549). -->
            <button
              class={`shrink-0 ${ID_CLASS} hover:text-[var(--color-accent)]`}
              title="Open this link's detail and comments"
              data-testid="link-open-detail"
              onclick={() => (previewNode = link.node_id)}>#{link.node_id}</button
            >
            <!-- The link navigates (WI #549). It used to preventDefault() and
                 open the editor instead, reserving ctrl/⌘-click for actually
                 following it — so the one control that looks like a link, is
                 announced as a link, and is the entire point of a reading list
                 did the one thing it did not say it would do. Editing is now
                 its own button. -->
            <a
              href={link.url}
              target="_blank"
              rel="noreferrer"
              class="min-w-0 flex-1 truncate text-sm text-[var(--color-accent)] hover:underline"
            >
              {link.title ?? link.url}
            </a>
            <button
              class="shrink-0 rounded px-1.5 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)] hover:text-[var(--color-accent)]"
              aria-expanded={editing === link.node_id}
              aria-label={`Edit ${link.title ?? link.url}`}
              onclick={() =>
                (editing = editing === link.node_id ? null : link.node_id)}
              >Edit</button
            >
            <!-- Fixed width, not `px-1.5` alone: the chip is the last item, so
                 its width sets where Edit ends up, and `Unread` / `Summarized`
                 / `VaultSaved` are three different widths. Without this Edit is
                 consistent for a given disposition and still wobbles between
                 rows — a column only by coincidence. `w-24` fits the longest
                 label (`Summarized`) at this size with room to spare. -->
            <span
              class="w-24 shrink-0 rounded px-1.5 py-0.5 text-center text-xs"
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
                {#each LINK_DISPOSITIONS as d (d)}
                  <button
                    class="rounded px-2 py-0.5 text-xs hover:bg-[var(--color-surface-hi)]"
                    class:bg-[var(--color-accent-soft)]={link.disposition === d}
                    onclick={() => setDisposition(link, d)}
                  >
                    {d}
                  </button>
                {/each}
              </div>
              <label class="sr-only" for={`link-tags-${link.node_id}`}
                >Tags, comma separated</label
              >
              <input
                id={`link-tags-${link.node_id}`}
                class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none"
                placeholder="tags, comma, separated"
                value={link.tags.join(", ")}
                onblur={(e) => saveTags(link, e.currentTarget.value)}
              />
            </div>
          {:else if link.tags.length > 0}
            <div class="mt-1 flex flex-wrap gap-1">
              {#each link.tags as tag (tag)}
                <span class={chip.tag}>#{tag}</span>
              {/each}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

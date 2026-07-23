<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Topic } from "$lib/api";
  import { chip } from "$lib/domain";
  import { attempt, notify } from "$lib/toast.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";

  let topics = $state<Topic[]>([]);
  let query = $state("");
  let loading = $state(true);
  let loadError = $state<unknown>(null);
  let creating = $state(false);
  let editing = $state<Topic | null>(null);
  let form = $state({ name: "", description: "", category: "", tags: "" });

  async function load() {
    loading = true;
    loadError = null;
    try {
      topics = (await api.topics(query.trim() === "" ? undefined : query.trim()))
        .items;
    } catch (cause) {
      loadError = cause;
    } finally {
      loading = false;
    }
  }
  function blankForm() {
    form = { name: "", description: "", category: "", tags: "" };
  }
  function tags(): string[] {
    return form.tags
      .split(",")
      .map((tag) => tag.trim())
      .filter((tag) => tag !== "");
  }
  function startCreate() {
    editing = null;
    blankForm();
    creating = true;
  }
  function startEdit(topic: Topic) {
    creating = false;
    editing = topic;
    form = {
      name: topic.name,
      description: topic.description ?? "",
      category: topic.category ?? "",
      tags: topic.tags.join(", "),
    };
  }
  function cancel() {
    creating = false;
    editing = null;
    blankForm();
  }
  async function save() {
    const name = form.name.trim();
    if (name === "") return;
    const target = editing;
    const saved = await attempt(
      () =>
        target
          ? api.updateTopic(target.node_id, {
              name,
              description: form.description.trim() || null,
              category: form.category.trim() || null,
              tags: tags(),
            })
          : api.createTopic({
              name,
              description: form.description.trim() || undefined,
              category: form.category.trim() || undefined,
              tags: tags(),
            }),
      target ? "Save topic" : "Create topic",
    );
    // Keep the form open on failure so the input is not thrown away.
    if (!saved) return;
    notify(target ? `Updated ${name}` : `Created ${name}`);
    cancel();
    await load();
  }

  // Archiving is reversible (archiveTopic takes a flag), so undo, not confirm
  // (WI #549).
  async function archive(topic: Topic) {
    const r = await attempt(
      () => api.archiveTopic(topic.node_id),
      "Archive topic",
    );
    if (!r) return;
    if (editing?.node_id === topic.node_id) cancel();
    await load();
    notify(`Archived ${topic.name}.`, async () => {
      const undone = await attempt(
        () => api.archiveTopic(topic.node_id, false),
        "Undo",
      );
      if (undone) await load();
    });
  }
  onMount(load);
</script>

<section class="space-y-4">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <p
        class="text-xs font-medium uppercase tracking-[0.18em] text-[var(--color-accent)]"
      >
        Reusable planning sources
      </p>
      <h1 class="text-2xl font-semibold">Topics</h1>
    </div>
    <button
      class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]"
      onclick={startCreate}>New topic</button
    >
  </header>
  <div
    class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
  >
    <label class="sr-only" for="topic-search">Search active topics</label><input
      id="topic-search"
      type="search"
      class="w-full rounded bg-[var(--color-surface-hi)] px-3 py-2 text-sm outline-none"
      placeholder="Search active topics…"
      bind:value={query}
      oninput={() => void load()}
    />
  </div>
  <!-- Successes and mutation failures go to the shared toaster; only a failed
       load replaces content, so only that renders here. -->
  {#if loadError}
    <ErrorNotice error={loadError} what="topics" retry={load} />
  {/if}

  {#if creating || editing}
    <form
      class="space-y-3 rounded-lg border border-[var(--color-accent)] bg-[var(--color-surface)] p-4"
      onsubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <h2 class="font-semibold">{editing ? "Edit topic" : "Create topic"}</h2>
      <label class="block text-xs text-[var(--color-muted)]"
        >Name<input
          required
          class="mt-1 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm text-[var(--color-text)] outline-none"
          bind:value={form.name}
        /></label
      >
      <label class="block text-xs text-[var(--color-muted)]"
        >Description<textarea
          class="mt-1 min-h-20 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm text-[var(--color-text)] outline-none"
          bind:value={form.description}
        ></textarea></label
      >
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="block text-xs text-[var(--color-muted)]"
          >Category<input
            class="mt-1 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm text-[var(--color-text)] outline-none"
            bind:value={form.category}
          /></label
        ><label class="block text-xs text-[var(--color-muted)]"
          >Tags <span>(comma-separated)</span><input
            class="mt-1 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm text-[var(--color-text)] outline-none"
            bind:value={form.tags}
          /></label
        >
      </div>
      <div class="flex justify-end gap-2">
        <button
          type="button"
          class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]"
          onclick={cancel}>Cancel</button
        ><button
          type="submit"
          class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]"
          >Save topic</button
        >
      </div>
    </form>
  {/if}

  {#if loading}<p class="text-[var(--color-muted)]">Loading topics…</p>{:else}
    <ul
      class="divide-y divide-[var(--color-border)] overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
    >
      {#each topics as topic (topic.node_id)}
        <li
          class="flex flex-wrap items-start justify-between gap-3 p-4"
          data-testid={`topic-${topic.node_id}`}
        >
          <div class="min-w-0 flex-1">
            <h2 class="font-medium">{topic.name}</h2>
            {#if topic.description}<p
                class="mt-1 text-sm text-[var(--color-muted)]"
              >
                {topic.description}
              </p>{/if}
            <div class="mt-2 flex flex-wrap gap-1">
              {#if topic.category}<span class={chip.category}
                  >{topic.category}</span
                >{/if}{#each topic.tags as tag (tag)}<span class={chip.tag}
                  >{tag}</span
                >{/each}
            </div>
          </div>
          <div class="flex gap-2">
            <button
              class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]"
              aria-label={`Edit ${topic.name}`}
              onclick={() => startEdit(topic)}>Edit</button
            ><button
              class="rounded border border-red-900 px-2 py-1 text-xs text-red-300 hover:bg-red-950"
              aria-label={`Archive ${topic.name}`}
              onclick={() => archive(topic)}>Archive</button
            >
          </div>
        </li>
      {:else}<li class="p-6 text-center text-sm text-[var(--color-muted)]">
          No active topics match.
        </li>{/each}
    </ul>
  {/if}
</section>

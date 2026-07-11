<script lang="ts">
  import { api, type Topic } from "$lib/api";

  let {
    topics,
    planDate,
    disabled = false,
    onadded,
  }: {
    topics: Topic[];
    planDate: string;
    disabled?: boolean;
    onadded: (message: string) => void | Promise<void>;
  } = $props();

  let query = $state("");
  let open = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let activeIndex = $state(0);

  const matches = $derived(
    topics
      .filter((topic) => {
        const needle = query.trim().toLocaleLowerCase();
        return (
          needle === "" ||
          topic.name.toLocaleLowerCase().includes(needle) ||
          (topic.description?.toLocaleLowerCase().includes(needle) ?? false)
        );
      })
      .slice(0, 8),
  );
  const exactMatch = $derived(
    topics.find(
      (topic) =>
        topic.name.toLocaleLowerCase() === query.trim().toLocaleLowerCase(),
    ),
  );
  const canCreate = $derived(query.trim() !== "" && exactMatch === undefined);
  const optionCount = $derived(matches.length + (canCreate ? 1 : 0));

  async function addTopic(topic: Topic) {
    busy = true;
    error = null;
    try {
      await api.createDailyPlanItem(topic.node_id, planDate);
      query = "";
      open = false;
      await onadded(`Added ${topic.name}`);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy = false;
    }
  }

  async function createAndAdd() {
    const name = query.trim();
    if (name === "") return;
    busy = true;
    error = null;
    try {
      const created = await api.createTopic({ name });
      await api.createDailyPlanItem(created.node_id, planDate);
      query = "";
      open = false;
      await onadded(`Created and added ${name}`);
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy = false;
    }
  }

  async function choose(index: number) {
    if (index < matches.length) {
      const topic = matches[index];
      if (topic) await addTopic(topic);
    } else if (canCreate) {
      await createAndAdd();
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      open = true;
      activeIndex = Math.min(activeIndex + 1, Math.max(optionCount - 1, 0));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      activeIndex = Math.max(activeIndex - 1, 0);
    } else if (event.key === "Enter" && open && optionCount > 0) {
      event.preventDefault();
      void choose(activeIndex);
    } else if (event.key === "Escape") {
      open = false;
    }
  }
</script>

<div class="relative" data-testid={`topic-picker-${planDate}`}>
  <label class="sr-only" for={`topic-${planDate}`}
    >Add topic to {planDate}</label
  >
  <input
    id={`topic-${planDate}`}
    type="search"
    role="combobox"
    aria-label={`Add topic to ${planDate}`}
    aria-autocomplete="list"
    aria-expanded={open}
    aria-controls={`topic-options-${planDate}`}
    autocomplete="off"
    class="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-xs outline-none placeholder:text-[var(--color-muted)]"
    placeholder="Add a topic…"
    bind:value={query}
    disabled={disabled || busy}
    onfocus={() => (open = true)}
    oninput={() => {
      open = true;
      activeIndex = 0;
    }}
    onkeydown={handleKeydown}
  />
  {#if open && !disabled}
    <div
      id={`topic-options-${planDate}`}
      role="listbox"
      class="absolute z-30 mt-1 max-h-64 w-full min-w-52 overflow-auto rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-1 shadow-xl"
    >
      {#each matches as topic, index (topic.node_id)}
        <button
          type="button"
          role="option"
          aria-selected={activeIndex === index}
          class="block w-full rounded px-2 py-1.5 text-left text-xs hover:bg-[var(--color-surface-hi)]"
          class:bg-[var(--color-surface-hi)]={activeIndex === index}
          onmousedown={(event) => event.preventDefault()}
          onclick={() => addTopic(topic)}
        >
          <span class="block font-medium">{topic.name}</span>
          {#if topic.description}<span
              class="block truncate text-[var(--color-muted)]"
              >{topic.description}</span
            >{/if}
        </button>
      {/each}
      {#if canCreate}
        <button
          type="button"
          role="option"
          aria-selected={activeIndex === matches.length}
          class="block w-full rounded px-2 py-1.5 text-left text-xs text-[var(--color-accent)] hover:bg-[var(--color-surface-hi)]"
          class:bg-[var(--color-surface-hi)]={activeIndex === matches.length}
          onmousedown={(event) => event.preventDefault()}
          onclick={createAndAdd}
        >
          Create topic “{query.trim()}”
        </button>
      {:else if matches.length === 0}
        <p class="px-2 py-1.5 text-xs text-[var(--color-muted)]">
          Type a name to create a topic.
        </p>
      {/if}
    </div>
  {/if}
  {#if error}<p class="mt-1 text-xs text-red-300" role="alert">{error}</p>{/if}
</div>

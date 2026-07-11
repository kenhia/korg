<script lang="ts">
  // Shared slide-over preview for any node kind (WI #231 + #260). Give it a
  // node id; it fetches GET /api/nodes/:id and renders a uniform preview —
  // work item, card, link, report, sprint proposal, topic, or daily plan item. Used by Planning,
  // Daily Reports and the Work Items find-by-ID box so the panel lives once.
  import { api, type NodePreview } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";

  let { nodeId, onClose }: { nodeId: number; onClose: () => void } = $props();

  let node = $state<NodePreview | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let missing = $state(false);

  // Re-fetch whenever the requested id changes.
  $effect(() => {
    const id = nodeId;
    node = null;
    error = null;
    missing = false;
    loading = true;
    api
      .node(id)
      .then((n) => {
        if (n === null) missing = true;
        else node = n;
      })
      .catch((e) => (error = e instanceof Error ? e.message : String(e)))
      .finally(() => (loading = false));
  });

  // Human label for the id line: #260 for work items, node #12 otherwise.
  const idLabel = $derived(
    node?.wi_number != null ? `#${node.wi_number}` : `node #${nodeId}`,
  );
</script>

<div class="fixed inset-0 z-50 flex justify-end">
  <button
    class="absolute inset-0 bg-black/60"
    aria-label="Close preview"
    onclick={onClose}
  ></button>
  <div
    class="relative z-10 h-full w-full max-w-md overflow-auto border-l border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl"
    data-testid="node-preview-panel"
  >
    <div class="mb-3 flex items-center justify-between">
      <h2 class="text-lg font-semibold">
        {#if node}
          <span
            class="rounded bg-[var(--color-accent-soft)] px-1.5 py-0.5 text-xs uppercase tracking-wide text-[var(--color-accent)]"
            >{node.kind}</span
          >
        {:else}
          Preview
        {/if}
      </h2>
      <button
        class="rounded px-2 py-1 text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)]"
        aria-label="Close"
        onclick={onClose}>✕</button
      >
    </div>

    {#if loading}
      <p class="text-[var(--color-muted)]">Loading…</p>
    {:else if error}
      <p class="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
    {:else if missing}
      <p class="text-sm text-[var(--color-muted)]">No node with id {nodeId}.</p>
    {:else if node}
      <h3 class="text-base font-medium">
        <span class="font-mono text-[var(--color-muted)]">{idLabel}</span>
        {node.title}
        {#if node.archived}<span
            class="ml-1 rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]"
            >archived</span
          >{/if}
      </h3>

      <div class="mt-2 flex flex-wrap gap-1 text-xs">
        {#each node.badges as b (b)}
          <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5"
            >{b}</span
          >
        {/each}
        {#if node.project}<span
            class="rounded bg-teal-900/60 px-1.5 py-0.5 text-teal-300"
            >{node.project}</span
          >{/if}
        {#each node.tags as t (t)}<span
            class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-[var(--color-muted)]"
            >#{t}</span
          >{/each}
      </div>

      {#if node.fields.length > 0}
        <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
          {#each node.fields as f (f.label)}
            <dt class="text-[var(--color-muted)]">{f.label}</dt>
            <dd class="break-words">
              {#if f.label === "URL"}
                <a
                  class="text-[var(--color-accent)] hover:underline"
                  href={f.value}
                  target="_blank"
                  rel="noreferrer">{f.value}</a
                >
              {:else}
                {f.value}
              {/if}
            </dd>
          {/each}
        </dl>
      {/if}

      {#if node.body}
        <section class="mt-4">
          {#if node.body_label}<h4
              class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold"
            >
              {node.body_label}
            </h4>{/if}
          <div class="prose prose-invert max-w-none text-sm">
            <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
            {@html renderMarkdown(node.body)}
          </div>
        </section>
      {/if}

      {#if node.details}
        <section class="mt-4">
          <h4
            class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold"
          >
            Details
          </h4>
          <div
            class="prose prose-invert max-w-none rounded p-2 text-sm"
            style="background: color-mix(in oklch, var(--color-surface) 75%, var(--color-accent) 25%)"
          >
            <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
            {@html renderMarkdown(node.details)}
          </div>
        </section>
      {/if}
    {/if}
  </div>
</div>

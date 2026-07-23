<script lang="ts">
  // Shared slide-over preview for any node kind (WI #231 + #260). Give it a
  // node id; it fetches GET /api/nodes/:id and renders a uniform preview —
  // work item, card, link, report, sprint proposal, topic, or daily plan item. Used by Planning,
  // Daily Reports and the Work Items find-by-ID box so the panel lives once.
  import { api, type NodePreview } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";
  import { chip } from "$lib/domain";
  import Dialog from "./Dialog.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";

  let { nodeId, onClose }: { nodeId: number; onClose: () => void } = $props();

  let node = $state<NodePreview | null>(null);
  let loading = $state(true);
  let error = $state<unknown>(null);
  let missing = $state(false);

  function load(id: number) {
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
      .catch((e) => (error = e))
      .finally(() => (loading = false));
  }

  // Re-fetch whenever the requested id changes.
  $effect(() => {
    load(nodeId);
  });

  // Human label for the id line: #260 for work items, node #12 otherwise.
  const idLabel = $derived(
    node?.wi_number != null ? `#${node.wi_number}` : `node #${nodeId}`,
  );
</script>

<!-- Was a hand-built overlay whose scrim was a full-screen
     `<button aria-label="Close preview">` — a viewport-sized control in the tab
     order and the a11y tree — with no focus trap, no focus restore and no
     Escape. `<dialog>` gives all three, and `::backdrop` gives the scrim
     without the phantom button (WI #548). -->
<Dialog
  open={true}
  {onClose}
  placement="side"
  title={node ? `${node.kind} preview` : "Preview"}
  titleHidden
  class="max-w-md"
>
  <div data-testid="node-preview-panel">
    <div class="mb-3">
      {#if node}
        <span
          class="rounded bg-[var(--color-accent-soft)] px-1.5 py-0.5 text-xs uppercase tracking-wide text-[var(--color-accent)]"
          >{node.kind}</span
        >
      {/if}
    </div>

    {#if loading}
      <p class="text-[var(--color-muted)]">Loading…</p>
    {:else if error}
      <ErrorNotice
        {error}
        what="this node"
        retry={() => load(nodeId)}
      />
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

      <!-- These were hand-written copies of the project and tag chip styles —
           the seventh variant F-15 counted, and the one site B4's consolidation
           into domain.ts missed. They use the shared map now, which is also how
           the tag contrast fix (WI #571) reaches this panel for free. -->
      <div class="mt-2 flex flex-wrap gap-1 text-xs">
        {#each node.badges as b (b)}
          <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5"
            >{b}</span
          >
        {/each}
        {#if node.project}<span class={chip.project}>{node.project}</span>{/if}
        {#each node.tags as t (t)}<span class={chip.tag}>#{t}</span>{/each}
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
</Dialog>

<script lang="ts">
  // One node, full width, for every kind that has no bespoke page of its own
  // (WI #1467, sprint 070).
  //
  // The six pages this backs — cards, links, reports, schedules, attachments
  // and work items — are each four lines: a `<BackTo>` destination and this.
  // That is deliberate, and it is what made #1467 an M rather than six separate
  // page builds. `get_node_preview` has been a uniform, kind-agnostic payload
  // since #260 and has had an arm for every kind since #870's audit; what korg
  // was missing was never the *rendering*, only a URL to render it at.
  //
  // So this is the slide-over's payload at a width you can read prose in. The
  // slide-over stays what it is — a peek you open without leaving the page you
  // are on — exactly as #1162 kept it when proposals got a page.
  //
  // Per-kind routes over a generic `/nodes/:id` (#621) is upheld by the URL,
  // not by the renderer: `/cards/812` says what it points at, and the day a
  // card wants a real board-aware page it replaces this component behind the
  // same address. A kind that outgrows the uniform view graduates; it does not
  // get a new URL.
  import { api, type NodePreview, type Neighbor } from "$lib/api";
  import { chip, nodePage, stamp } from "$lib/domain";
  import { imgIdFromNodeId, imgUrl } from "$lib/img";
  import Comments from "./Comments.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import MarkdownView from "./MarkdownView.svelte";

  let {
    nodeId,
    expect,
  }: {
    nodeId: number;
    /** The kind this route is for, so a mismatched id says so rather than
     *  rendering a schedule under the Cards heading. */
    expect: string;
  } = $props();

  let node = $state<NodePreview | null>(null);
  let related = $state<Neighbor[]>([]);
  let relatedError = $state<unknown>(null);
  let loading = $state(true);
  let error = $state<unknown>(null);
  let missing = $state(false);

  function load(id: number) {
    node = null;
    related = [];
    relatedError = null;
    error = null;
    missing = false;
    loading = true;
    api
      .node(id)
      .then((n) => {
        if (id !== nodeId) return;
        if (n === null) missing = true;
        else node = n;
      })
      .catch((e) => {
        if (id === nodeId) error = e;
      })
      .finally(() => {
        if (id === nodeId) loading = false;
      });
    // Separate call because `get_node_preview` is a preview: it answers "what
    // is this", and the edges are a different question with its own bound. Its
    // failure is reported beside the edges rather than replacing the node —
    // an unreachable edge list still leaves a node worth reading — but it *is*
    // reported: a silently empty Related section reads as "no edges", which is
    // a different and wrong answer.
    api
      .neighbors(id, { limit: 25 })
      .then((p) => {
        if (id === nodeId) related = p.items;
      })
      .catch((e) => {
        if (id === nodeId) relatedError = e;
      });
  }

  $effect(() => {
    if (Number.isFinite(nodeId)) load(nodeId);
  });

  // `wi_number` is the handle for a work item and `node #n` for everything
  // else — the same line the slide-over prints, and the id an agent cites.
  const idLabel = $derived(
    node?.wi_number != null ? `#${node.wi_number}` : `node #${nodeId}`,
  );

  const wrongKind = $derived(node != null && node.kind !== expect);
</script>

<svelte:head>
  <title>{node ? `${node.title} — korg` : "korg"}</title>
</svelte:head>

{#if error}
  <ErrorNotice {error} what="this node" retry={() => load(nodeId)} />
{:else if missing}
  <p class="text-[var(--color-muted)]">No node with id {nodeId}.</p>
{:else if loading}
  <p class="text-[var(--color-muted)]">Loading…</p>
{:else if node}
  {#if wrongKind}
    <!-- Node ids are one sequence across every kind, so `/cards/1469` is a
         typo away from a real proposal. Saying so beats rendering it under the
         wrong heading, and the link is the URL this node actually has. -->
    <p
      class="mb-4 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm"
      data-testid="wrong-kind"
    >
      Node #{nodeId} is a <strong>{node.kind}</strong>, not a {expect}.
      {#if nodePage(node.kind, nodeId)}
        <a class="underline" href={nodePage(node.kind, nodeId)}>Open it where it lives ↗</a>
      {/if}
    </p>
  {/if}

  <article class="space-y-5" data-testid="node-detail">
    <header class="space-y-2 border-b border-[var(--color-border)] pb-3">
      <div class="flex flex-wrap items-baseline gap-2">
        <span class="font-mono text-sm text-[var(--color-muted)]" data-testid="node-detail-id"
          >{idLabel}</span
        >
        <h1 class="text-2xl font-semibold">{node.title}</h1>
        {#if node.archived}
          <span
            class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]"
            >archived</span
          >
        {/if}
        <span class="ml-auto text-xs text-[var(--color-muted)]" data-testid="node-detail-stamps"
          >created {stamp(node.created)}{#if node.updated !== node.created}
            · updated {stamp(node.updated)}{/if}</span
        >
      </div>
      <div class="flex flex-wrap gap-1 text-xs">
        {#each node.badges as b (b)}
          <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5">{b}</span>
        {/each}
        {#if node.project}<span class={chip.project}>{node.project}</span>{/if}
        {#each node.tags as t (t)}<span class={chip.tag}>#{t}</span>{/each}
      </div>
    </header>

    {#if node.kind === "attachment"}
      <!-- The one kind whose answer to "what is this" is not text. The preview
           payload leaves `body` null for an attachment on purpose — that struct
           carries markdown and a screenshot is not markdown — so the picture
           is the page's own job, and a metadata table with no image on it would
           be the least useful attachment page possible. -->
      <a href={imgUrl(imgIdFromNodeId(nodeId) ?? "")} target="_blank" rel="noreferrer">
        <img
          class="max-h-[60vh] max-w-full rounded border border-[var(--color-border)] object-contain"
          src={imgUrl(imgIdFromNodeId(nodeId) ?? "", "agent")}
          alt={node.title}
          data-testid="attachment-image"
        />
      </a>
    {/if}

    {#if node.fields.length > 0}
      <dl class="grid max-w-3xl grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
        {#each node.fields as f (f.label)}
          <dt class="text-[var(--color-muted)]">{f.label}</dt>
          <dd class="break-words">
            {#if f.value.startsWith("http") || f.value.startsWith("/api/")}
              <!-- A link's URL and an attachment's variant paths are the
                   payload's only genuinely clickable field values; the preview
                   panel special-cased "URL" by label, which missed the four
                   variant rows an attachment carries. -->
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
      <section class="max-w-3xl">
        {#if node.body_label}
          <h2 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">
            {node.body_label}
          </h2>
        {/if}
        <MarkdownView src={node.body} />
      </section>
    {/if}

    {#if node.details}
      <section class="max-w-3xl">
        <h2 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">
          Details
        </h2>
        <MarkdownView src={node.details} />
      </section>
    {/if}

    {#if relatedError}
      <ErrorNotice error={relatedError} what="related nodes" retry={() => load(nodeId)} />
    {:else if related.length > 0}
      <section>
        <h2 class="mb-2 text-sm font-semibold">Related</h2>
        <!-- Ids rather than titles: `neighbors` returns the edge, not the
             neighbour's content (LB-3's bound is on edges), and resolving 25
             titles would be 25 reads to decorate a list most nodes do not have.
             Every entry is a link because #1467 is exactly the change that made
             every kind linkable — this list was impossible to build before it. -->
        <ul class="space-y-1 text-sm" data-testid="node-detail-related">
          {#each related as r (r.rel_id)}
            <li class="flex flex-wrap items-baseline gap-2">
              <span class="text-xs text-[var(--color-muted)]"
                >{r.directed && r.direction === "in" ? `← ${r.label}` : r.label}</span
              >
              {#if nodePage(r.kind, r.node_id)}
                <a class="hover:underline" href={nodePage(r.kind, r.node_id)}
                  >{r.kind} #{r.node_id}</a
                >
              {:else}
                <span>{r.kind} #{r.node_id}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <Comments node_id={nodeId} />
  </article>
{/if}

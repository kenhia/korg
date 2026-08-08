<script lang="ts">
  // Shared slide-over preview for any node kind (WI #231 + #260). Give it a
  // node id; it fetches GET /api/nodes/:id and renders a uniform preview —
  // work item, card, link, report, sprint proposal, program, schedule or
  // handoff. Used by Planning, Schedules, Reading list, Daily Reports and the
  // Work Items find-by-ID box so the panel lives once.
  //
  // Sprint 055 (#870's punch list) made that "lives once" literal for the two
  // node-level columns every kind has and almost no kind rendered: comments and
  // timestamps. The data model was already generic — `add_comment`/
  // `list_comments` take any node id, `node.tags`/`created`/`updated` exist for
  // every kind — and only the UI was per-kind. That mismatch is what filed
  // #1104, #1106 and #1107 as three separate items, and `schedule` is the proof
  // it regenerates: it landed in sprint 051 brand new and arrived with all
  // three gaps. Rendering them here means the *next* kind gets them for free
  // rather than filing punch-list items 6-10.
  import { api, type NodePreview } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";
  import { chip, nodePage, stamp } from "$lib/domain";
  import Comments from "./Comments.svelte";
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
    <div class="mb-3 flex items-center gap-2">
      {#if node}
        <span
          class="rounded bg-[var(--color-accent-soft)] px-1.5 py-0.5 text-xs uppercase tracking-wide text-[var(--color-accent)]"
          >{node.kind}</span
        >
        <!-- Handoffs get a full page (WI #621). The slide-over stays the quick
             peek and becomes its entry point: a long handoff is unreadable at
             max-w-md, and commenting on one needs somewhere to put the thread.
             This was hardcoded to `handoff` because that was the only detail
             route at the time; programs have had one since 044, so the button
             asks `nodePage` instead of naming a kind (WI #982). -->
        {#if nodePage(node.kind, nodeId)}
          <a
            class="ml-auto rounded border border-[var(--color-border)] px-2 py-0.5 text-xs hover:bg-[var(--color-accent-soft)]"
            href={nodePage(node.kind, nodeId)}
            data-testid="open-node-page"
            title={`Open this ${node.kind} on its own page`}
            onclick={onClose}>Open full page ↗</a
          >
        {/if}
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

      <!-- WI #1106. `created`/`updated` were in this payload from the start and
           rendered by nothing, so "when was this queued?" needed an MCP call.
           They sit below the body rather than in the `fields` list because they
           are node-level and every kind has them, where `fields` is the
           per-kind metadata each arm chose — mixing the two would make the
           uniform part look like another arm's decision. `updated` only when it
           differs: on the many nodes never edited since creation, a second
           identical timestamp is noise that reads as information. -->
      <p
        class="mt-4 text-xs text-[var(--color-muted)]"
        data-testid="node-preview-stamps"
      >
        created {stamp(node.created)}{#if node.updated !== node.created}
          · updated {stamp(node.updated)}{/if}
      </p>

      <!-- WI #1104 + #1107 — the punch list's largest real gap. Agents comment
           on proposals routinely (the pre-sprint premise check does exactly
           that) and there was no way to read one in the web app: information
           written into korg on the assumption someone would see it, that nobody
           could. Mounting `Comments` here rather than on Planning and Schedules
           separately is the whole architectural move — it covers links (#1107)
           and every future kind in the same change.

           Keyed on `nodeId` for the state `Comments` does *not* guard. Its
           fetch is already safe across a reused instance — the `$effect` clears
           the list and every callback re-checks `id === node_id`. Its editor is
           not: `newComment` and `editingId`/`editBuf` are plain state, and this
           panel changes node in place (Planning switches straight from one
           chip to another without closing). Without the key, a half-typed draft
           follows you to the next node and "Save" on an in-progress edit
           PATCHes a comment that belongs to the node you just left. -->
      {#key nodeId}
        <div class="mt-4">
          <Comments node_id={nodeId} />
        </div>
      {/key}
    {/if}
  </div>
</Dialog>

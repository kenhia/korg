<script lang="ts">
  // The handoff reading page (WI #621) — korg's FIRST per-node detail route.
  //
  // Why /handoffs/:node_id and not a generic /nodes/:id, which was the other
  // candidate on the WI: the payloads genuinely differ. `GET /api/handoffs/:id`
  // returns the full Markdown body plus the LB-3 `related` block, which is
  // strictly more than the `/api/nodes/:id` preview shape carries — a generic
  // page would either render the thin preview (losing the whole point of a
  // wider read surface) or branch on kind internally, which is a kind-specific
  // page wearing a generic URL. A generic route would also promise something
  // korg cannot deliver today: only handoffs have a full-page read surface, so
  // /nodes/123 for a card would 404 or render a stub. Per-kind routes can be
  // added as each kind earns one, and a generic viewer later is still open —
  // /nodes/:id can redirect by kind. Nothing here is a one-way door.
  //
  // The body is READ-ONLY on purpose: handoffs are authored through the API and
  // the `handoff` skill, and a human reviewing one should be able to respond
  // without risking the document. Responding means a comment.
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import { api, type HandoffFull, type RelatedRef } from "$lib/api";
  import { renderMarkdown } from "$lib/markdown";
  import { chip } from "$lib/domain";
  import Comments from "$lib/components/Comments.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";

  const nodeId = $derived(Number($page.params.node_id));

  let handoff = $state<HandoffFull | null>(null);
  let loading = $state(true);
  let error = $state<unknown>(null);
  let missing = $state(false);
  let previewNode = $state<number | null>(null);

  function load(id: number) {
    handoff = null;
    error = null;
    missing = false;
    loading = true;
    api
      .handoff(id)
      .then((h) => {
        // Guard against a stale response landing after the id changed.
        if (id !== nodeId) return;
        if (h === null) missing = true;
        else handoff = h;
      })
      .catch((e) => {
        if (id === nodeId) error = e;
      })
      .finally(() => {
        if (id === nodeId) loading = false;
      });
  }

  $effect(() => {
    if (Number.isFinite(nodeId)) load(nodeId);
  });

  function refLabel(r: RelatedRef): string {
    return r.wi_number != null ? `#${r.wi_number} ${r.title}` : r.title;
  }

  // The handoff's own timestamps say how current the context is, which is the
  // first thing you want to know when you are about to act on it.
  function stamp(iso: string): string {
    const d = new Date(iso);
    return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
  }

  // Getting *out* was browser-back only (Ken, 2026-07-29). Escape now leaves,
  // matching the slide-over this page is reached from — the two are the same
  // gesture at different sizes, so they should exit the same way. The visible
  // control matters just as much: a keyboard-only fix would leave the mouse
  // path exactly as it was.
  //
  // Back rather than a fixed destination, because this page is reached from
  // several places (a work item's related block, a Planning card). On a cold
  // URL there is nothing to go back to, so fall back to Work Items instead of
  // leaving the button dead.
  const arrivedDirectly =
    typeof history !== "undefined" && history.length <= 1;

  function leave() {
    if (arrivedDirectly) void goto("/work-items");
    else history.back();
  }

  function onKeyDown(e: KeyboardEvent) {
    // Not while typing a comment — Escape belongs to the textarea then.
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (e.key === "Escape") leave();
  }
</script>

<svelte:window onkeydown={onKeyDown} />

<svelte:head>
  <title>{handoff ? `${handoff.title} — korg` : "Handoff — korg"}</title>
</svelte:head>

<section class="space-y-4">
  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else if error}
    <ErrorNotice {error} what="this handoff" retry={() => load(nodeId)} />
  {:else if missing}
    <p class="text-sm text-[var(--color-muted)]">
      No handoff with node id {nodeId}.
    </p>
  {:else if handoff}
    <header class="space-y-2 border-b border-[var(--color-border)] pb-3">
      <div class="flex flex-wrap items-center gap-2">
        <button
          class="rounded border border-[var(--color-border)] px-2 py-0.5 text-xs hover:bg-[var(--color-surface-hi)]"
          data-testid="handoff-back"
          title="Back (Esc)"
          onclick={leave}>← Back</button
        >
        <span
          class="rounded bg-[var(--color-accent-soft)] px-1.5 py-0.5 text-xs uppercase tracking-wide text-[var(--color-accent)]"
          >handoff</span
        >
        <span class="font-mono text-xs text-[var(--color-muted)]"
          >node #{handoff.node_id}</span
        >
        {#if handoff.archived}
          <span
            class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]"
            >archived</span
          >
        {/if}
      </div>

      <h1 class="text-2xl font-semibold tracking-tight">{handoff.title}</h1>
      <p class="text-sm text-[var(--color-muted)]">{handoff.summary}</p>

      <div class="flex flex-wrap items-center gap-1 text-xs">
        {#if handoff.project}<span class={chip.project}>{handoff.project}</span
          >{/if}
        {#if handoff.category}<span class={chip.category}
            >{handoff.category}</span
          >{/if}
        {#each handoff.tags as t (t)}<span class={chip.tag}>#{t}</span>{/each}
        <span class="ml-auto text-[var(--color-muted)]"
          >updated {stamp(handoff.updated)}</span
        >
      </div>
    </header>

    <!-- The work this handoff belongs to. A handoff is only useful next to the
         thing it hands off, and arriving here from a bookmark or a pasted URL
         gives you no other way back to it. -->
    {#if handoff.related.length > 0}
      <section>
        <h2 class="mb-1 text-xs font-semibold text-[var(--color-muted)]">
          Attached to
        </h2>
        <ul class="flex flex-wrap gap-1">
          {#each handoff.related as r (r.rel_id)}
            <li>
              <button
                type="button"
                class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-accent-soft)]"
                title={`Preview ${r.kind} — ${refLabel(r)}`}
                onclick={() => (previewNode = r.node_id)}>{refLabel(r)}</button
              >
            </li>
          {/each}
        </ul>
        {#if handoff.related_truncated}
          <p class="mt-1 text-xs text-[var(--color-muted)]">
            More attachments exist than are shown.
          </p>
        {/if}
      </section>
    {/if}

    <article class="prose prose-invert max-w-none">
      <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
      {@html renderMarkdown(handoff.body)}
    </article>

    <!-- Nodes are comment-generic (0007), so the shared thread works here with
         no handoff-specific backend. This is the one thing a reader can write. -->
    <Comments node_id={handoff.node_id} />
  {/if}
</section>

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

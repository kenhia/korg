<script lang="ts">
  // One program: its aim, its notes, and its ordered slices with the work-item
  // rollup each one carries (#968, D-5).
  //
  // The whole page is ONE fetch. That is the design constraint the rollup read
  // exists to satisfy — the pre-044 way to render this would have been
  // program → neighbors → per-proposal get_proposal → per-proposal work items,
  // which is exactly the crawl `get_program` replaces.
  import { page } from "$app/stores";
  import { api, type ProgramDetail } from "$lib/api";
  import type { ProgramStatus } from "$lib/generated/vocab";
  import { renderMarkdown } from "$lib/markdown";
  import { chip } from "$lib/domain";
  import Comments from "$lib/components/Comments.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";

  const nodeId = $derived(Number($page.params.node_id));

  let program = $state<ProgramDetail | null>(null);
  let loading = $state(true);
  let error = $state<unknown>(null);
  let missing = $state(false);

  function load(id: number) {
    program = null;
    error = null;
    missing = false;
    loading = true;
    api
      .program(id)
      .then((p) => {
        // Guard against a stale response landing after the id changed.
        if (id !== nodeId) return;
        if (p === null) missing = true;
        else program = p;
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

  const statusPill: Record<ProgramStatus, string> = {
    active: "bg-emerald-900/60 text-emerald-300",
    holding: "bg-amber-900/60 text-amber-300",
    done: "bg-neutral-800 text-neutral-400",
  };

  // A slice is finished when every work item it covers is; a slice with no work
  // items is not "complete", it is empty, and the bar says so by staying blank.
  function progress(s: ProgramDetail["slices"][number]): number {
    if (s.covered_count === 0) return 0;
    return Math.round(((s.done + s.closed) / s.covered_count) * 100);
  }
</script>

<section class="space-y-5">
  <a class="text-sm text-[var(--color-muted)] hover:underline" href="/programs"
    >← Programs</a
  >

  {#if error}
    <ErrorNotice error={error} what="this program" retry={() => load(nodeId)} />
  {:else if missing}
    <p class="text-[var(--color-muted)]">No program with node_id {nodeId}.</p>
  {:else if loading}
    <p class="text-[var(--color-muted)]">Loading program…</p>
  {:else if program}
    <header class="space-y-2">
      <div class="flex flex-wrap items-baseline gap-2">
        <h1 class="text-2xl font-semibold">{program.title}</h1>
        <span
          class={`rounded px-1.5 py-0.5 text-xs ${statusPill[program.status as ProgramStatus] ?? ""}`}
          >{program.status}</span
        >
        <!-- The derived span (D-6). A program has no project of its own; this
             is the union of its slices' projects, and it is the honest answer
             to "which repos does this touch". -->
        {#each program.span as project (project)}
          <span class={chip.project}>{project}</span>
        {/each}
      </div>
      <p class="text-[var(--color-muted)]">{program.aim}</p>
    </header>

    {#if program.notes}
      <article class="prose prose-invert max-w-none text-sm">
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
        {@html renderMarkdown(program.notes)}
      </article>
    {/if}

    <div>
      <h2 class="mb-2 text-sm font-semibold">
        Slices <span class="font-normal text-[var(--color-muted)]"
          >· in program order</span
        >
      </h2>
      {#if program.slices.length === 0}
        <p class="text-sm text-[var(--color-muted)]">
          No slices yet. A program includes sprint proposals — link one with the
          <code>includes</code> label.
        </p>
      {:else}
        <ol class="space-y-2">
          {#each program.slices as s, i (s.node_id)}
            <li
              class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
            >
              <div class="flex flex-wrap items-baseline gap-2">
                <span class="text-xs tabular-nums text-[var(--color-muted)]"
                  >{i + 1}</span
                >
                <span class="font-medium">{s.title}</span>
                {#if s.project}<span class={chip.project}>{s.project}</span>{/if}
                <span class="text-xs text-[var(--color-muted)]">{s.status}</span>
                <span class="ml-auto text-xs tabular-nums text-[var(--color-muted)]">
                  {#if s.covered_count === 0}
                    no work items
                  {:else}
                    {s.done + s.closed}/{s.covered_count} done
                    {#if s.open > 0}· {s.open} open{/if}
                    {#if s.resolved > 0}· {s.resolved} resolved{/if}
                  {/if}
                </span>
              </div>
              {#if s.covered_count > 0}
                <div
                  class="mt-2 h-1 overflow-hidden rounded bg-[var(--color-surface-hi)]"
                >
                  <div
                    class="h-full bg-[var(--color-accent)]"
                    style={`width: ${progress(s)}%`}
                  ></div>
                </div>
              {/if}
            </li>
          {/each}
        </ol>
      {/if}
    </div>

    {#if program.related.length > 0}
      <div>
        <h2 class="mb-2 text-sm font-semibold">Related</h2>
        <ul class="space-y-1 text-sm">
          {#each program.related as r (r.rel_id)}
            <li class="flex flex-wrap items-baseline gap-2">
              <span class="text-xs text-[var(--color-muted)]">{r.label}</span>
              {#if r.kind === "handoff"}
                <a class="hover:underline" href={`/handoffs/${r.node_id}`}
                  >{r.title}</a
                >
              {:else}
                <span>{r.title}</span>
              {/if}
            </li>
          {/each}
        </ul>
        {#if program.related_truncated}
          <p class="mt-1 text-xs text-[var(--color-muted)]">
            More edges exist than are shown.
          </p>
        {/if}
      </div>
    {/if}

    <Comments node_id={program.node_id} />
  {/if}
</section>

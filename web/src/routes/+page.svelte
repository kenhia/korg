<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import {
    api,
    type CardRow,
    type ProposalRow,
    type ReportRow,
  } from "$lib/api";
  import { ID_CLASS, chip, isCut, reportStatusPill } from "$lib/domain";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";
  import AwaitingLane from "$lib/components/AwaitingLane.svelte";

  // Today is a planning overview: "which sprint am I on" is the question it
  // should answer at a glance (Ken, 2026-07-29). It carried the daily-plan
  // week board until sprint 050 removed the slots feature (WI #965); the
  // question outlived the grid, so the page stays — the Awaiting lane, the
  // report health pill, and the two trays, minus the planner.
  let cards = $state<CardRow[]>([]);
  let proposals = $state<ProposalRow[]>([]);
  // The most recent daily report, for the health pill beside the heading.
  // `list_reports` orders by report_date DESC, so the head of the list is it.
  let latestReport = $state<ReportRow | null>(null);
  let loading = $state(true);
  let loadError = $state<unknown>(null);
  let sourceSearch = $state("");
  let previewNode = $state<number | null>(null);

  // Proposals open, cards collapsed (Ken, 2026-07-29): the queue is the
  // answer to the page's question, and the card list is a long tray you open
  // when you want it.
  let showProposals = $state(true);
  let showCards = $state(false);

  function matches(title: string): boolean {
    return title
      .toLocaleLowerCase()
      .includes(sourceSearch.toLocaleLowerCase());
  }

  const cardSources = $derived(
    cards
      .filter((card) => !card.archived && !isCut(card.status))
      .filter((card) => matches(card.title))
      .slice(0, 40),
  );

  // Only the live queue: a done or declined proposal is not part of "which
  // sprint am I on".
  const proposalSources = $derived(
    proposals
      .filter((p) => !p.archived && (p.status === "proposed" || p.status === "active"))
      .filter((p) => matches(p.title))
      .slice(0, 40),
  );

  async function load() {
    loadError = null;
    try {
      const [cardPage, proposalRows, reportRows] = await Promise.all([
        api.cards(),
        api.proposals(),
        api.reports(),
      ]);
      cards = cardPage.items;
      proposals = proposalRows;
      latestReport = reportRows[0] ?? null;
    } catch (cause) {
      loadError = cause;
    } finally {
      loading = false;
    }
  }
  onMount(load);
</script>

<section class="space-y-5">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <p
        class="text-xs font-medium uppercase tracking-[0.18em] text-[var(--color-accent)]"
      >
        Today
      </p>
      <div class="flex flex-wrap items-center gap-4">
        <h1 class="text-2xl font-semibold">
          {new Date().toLocaleDateString(undefined, {
            weekday: "long",
            month: "long",
            day: "numeric",
          })}
        </h1>
        <!-- Latest daily-report status (Ken, 2026-07-29). Today is meant to be
             the page you open for an overview, and "is anything wrong?" is part
             of that overview. Links through to the report itself — the status
             is the question, the report is the answer. Same pill as the Daily
             Reports page, from one definition, so the colours cannot drift. -->
        {#if latestReport}
          <a
            href="/daily-reports"
            class={reportStatusPill(latestReport.status)}
            data-testid="latest-report-status"
            title={`Daily report ${latestReport.report_date} — ${latestReport.summary}`}
            >{latestReport.status}</a
          >
        {/if}
      </div>
    </div>
    <input
      type="search"
      aria-label="Search proposals and cards"
      class="w-64 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="Search…"
      bind:value={sourceSearch}
    />
  </header>
  <!-- The Commander's Call lane (#969). Above the trays because it is the one
       thing on this page that cannot move without Ken — an unanswered decision
       blocks an agent. Renders nothing at all when the lane is empty, so it
       stays a signal rather than furniture. -->
  <AwaitingLane />

  {#if loadError}
    <ErrorNotice error={loadError} what="the overview" retry={load} />
  {/if}
  {#if loading}<p class="text-[var(--color-muted)]">Loading…</p>{:else}
    {@render tray(
      "Proposals",
      showProposals,
      () => (showProposals = !showProposals),
      proposalSources.length,
      proposalList,
    )}
    {@render tray(
      "Cards",
      showCards,
      () => (showCards = !showCards),
      cardSources.length,
      cardList,
    )}
  {/if}
</section>

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

<!-- Both trays are the same object: a collapsible panel with a count in its
     header, so the count is readable while collapsed. -->
{#snippet tray(
  title: string,
  open: boolean,
  toggle: () => void,
  count: number,
  body: Snippet,
)}
  <section
    class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
  >
    <button
      class="flex w-full items-center gap-2 px-4 py-3 text-left"
      aria-expanded={open}
      data-testid={`tray-toggle-${title.toLowerCase()}`}
      onclick={toggle}
    >
      <span class="text-xs text-[var(--color-muted)]">{open ? "▾" : "▸"}</span>
      <h2 class="font-semibold">{title}</h2>
      <span class="text-xs text-[var(--color-muted)]">{count}</span>
    </button>
    {#if open}
      <div class="px-4 pb-4">{@render body()}</div>
    {/if}
  </section>
{/snippet}

{#snippet proposalList()}
  <ul class="grid max-h-72 gap-2 overflow-auto sm:grid-cols-2">
    {#each proposalSources as p (p.node_id)}
      <li
        class="flex items-start gap-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2"
      >
        {#if p.project}<span class={`${chip.project} shrink-0`}>{p.project}</span
          >{/if}
        <span class={`shrink-0 ${ID_CLASS}`}>#{p.node_id}</span>
        {#if p.status === "active"}<span
            class="shrink-0 rounded bg-sky-950 px-1 py-0.5 text-[9px] font-semibold uppercase text-sky-300"
            >active</span
          >{/if}
        <a
          class="min-w-0 flex-1 line-clamp-2 break-words text-xs hover:text-[var(--color-accent)]"
          href="/planning"
          title={p.title}>{p.title}</a
        >
        {@render quickView(p.node_id, p.title)}
      </li>
    {:else}
      <li class="text-sm text-[var(--color-muted)]">
        No proposals in the queue{sourceSearch ? " match that search" : ""}.
      </li>
    {/each}
  </ul>
{/snippet}

{#snippet cardList()}
  <ul class="grid max-h-72 gap-2 overflow-auto sm:grid-cols-2 lg:grid-cols-3">
    {#each cardSources as card (card.node_id)}
      <li
        class="flex items-start gap-2 rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-2"
      >
        <span class={`shrink-0 ${ID_CLASS}`}>#{card.node_id}</span>
        <a
          class="min-w-0 flex-1 line-clamp-2 break-words text-xs hover:text-[var(--color-accent)]"
          href="/cards"
          title={card.title}>{card.title}</a
        >
        {@render quickView(card.node_id, card.title)}
      </li>
    {:else}
      <li class="text-sm text-[var(--color-muted)]">
        No active cards{sourceSearch ? " match that search" : ""}.
      </li>
    {/each}
  </ul>
{/snippet}

<!-- "I will forget what this title encompasses" — the slide-over answers that
     without leaving the page. -->
{#snippet quickView(nodeId: number, title: string)}
  <button
    class="shrink-0 rounded px-1 py-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-accent)]"
    title={`Preview ${title}`}
    aria-label={`Preview ${title}`}
    onclick={() => (previewNode = nodeId)}>👁</button
  >
{/snippet}

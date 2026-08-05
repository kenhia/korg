<script lang="ts">
  // The Commander's Call lane (#969): everything that moves only when Ken acts.
  //
  // One `GET /api/awaiting` renders it — the rows already carry a resolved
  // title, the node's own status and its project, so there is no follow-up read
  // per row. The clear button is `PUT /api/nodes/:id/awaiting {awaiting:false}`,
  // the same core call an agent makes when it retracts its own marker; the UI
  // does not get a second path that could drift from the one agents use.
  //
  // Deliberately silent when empty: an "awaiting you" panel that renders an
  // empty state every day is a panel Ken learns to skip, and this one needs to
  // be read on the days it *does* have rows.
  import { api, type AwaitingRow } from "$lib/api";
  import { attempt } from "$lib/toast.svelte";
  import { chip } from "$lib/domain";

  let rows = $state<AwaitingRow[]>([]);
  let clearing = $state<number | null>(null);

  export async function load(): Promise<void> {
    const r = await attempt(() => api.awaiting(), "Loading what's awaiting you");
    if (r) rows = r;
  }

  async function clear(row: AwaitingRow) {
    clearing = row.node_id;
    const done = await attempt(
      () => api.setAwaiting(row.node_id, false),
      `Clearing "${row.title}"`,
    );
    if (done) rows = rows.filter((r) => r.node_id !== row.node_id);
    clearing = null;
  }

  // "waiting 9 days" is the whole reason the marker is a timestamp rather than
  // a flag, so it is the most prominent thing on the row after the title.
  function waitedFor(since: string | null): string {
    if (!since) return "";
    const days = Math.floor(
      (Date.now() - new Date(since).getTime()) / 86_400_000,
    );
    if (days < 1) return "today";
    if (days === 1) return "1 day";
    return `${days} days`;
  }

  // A work item is reachable by number; a program has its own page. Everything
  // else has no detail route yet, so it renders unlinked rather than pointing
  // at a 404.
  function href(row: AwaitingRow): string | null {
    if (row.kind === "workitem" && row.wi_number !== null)
      return `/work-items?wi=${row.wi_number}`;
    if (row.kind === "program") return `/programs/${row.node_id}`;
    if (row.kind === "sprint_proposal") return "/planning";
    return null;
  }

  $effect(() => {
    load();
  });
</script>

{#if rows.length > 0}
  <section
    class="rounded border border-amber-700/60 bg-amber-950/20 p-3"
    data-testid="awaiting-lane"
  >
    <h2 class="mb-2 text-xs font-medium uppercase tracking-[0.18em] text-amber-400">
      Awaiting you · {rows.length}
    </h2>
    <ul class="space-y-1.5">
      {#each rows as row (row.node_id)}
        <li class="flex flex-wrap items-baseline gap-2 text-sm">
          <span class="text-xs tabular-nums text-amber-500/90"
            >{waitedFor(row.awaiting_since)}</span
          >
          {#if href(row)}
            <a class="font-medium hover:underline" href={href(row)}
              >{row.title}</a
            >
          {:else}
            <span class="font-medium">{row.title}</span>
          {/if}
          {#if row.awaiting_note}
            <span class="text-[var(--color-muted)]">— {row.awaiting_note}</span>
          {/if}
          {#if row.project}<span class={chip.project}>{row.project}</span>{/if}
          {#if row.status}<span class="text-xs text-[var(--color-muted)]"
              >{row.status}</span
            >{/if}
          <button
            class="ml-auto rounded px-2 py-0.5 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)] disabled:opacity-50"
            disabled={clearing === row.node_id}
            onclick={() => clear(row)}
            title="Clear — you have dealt with this">clear</button
          >
        </li>
      {/each}
    </ul>
  </section>
{/if}

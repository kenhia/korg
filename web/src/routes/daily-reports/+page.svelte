<script lang="ts">
  import { api, type ReportFull, type ReportRow } from "$lib/api";
  import Comments from "$lib/components/Comments.svelte";
  import { renderMarkdown } from "$lib/markdown";

  let rows = $state<ReportRow[]>([]);
  let expanded = $state<Set<number>>(new Set());
  let full = $state<Record<number, ReportFull>>({});
  let error = $state<string | null>(null);

  const statusStyle: Record<string, string> = {
    ok: "bg-emerald-900/40 text-emerald-300 border-emerald-700",
    attention: "bg-amber-900/40 text-amber-300 border-amber-700",
    problem: "bg-red-900/40 text-red-300 border-red-700",
  };

  async function load() {
    try {
      rows = await api.reports();
      // Leaderboard UX: the latest report arrives pre-expanded.
      if (rows.length > 0) await toggle(rows[0].node_id, true);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function toggle(node_id: number, open?: boolean) {
    const next = new Set(expanded);
    const willOpen = open ?? !next.has(node_id);
    if (willOpen) {
      next.add(node_id);
      if (!full[node_id]) {
        try {
          full[node_id] = await api.report(node_id);
        } catch (e) {
          error = e instanceof Error ? e.message : String(e);
        }
      }
    } else {
      next.delete(node_id);
    }
    expanded = next;
  }

  load();
</script>

<svelte:head><title>korg — daily reports</title></svelte:head>

<h1 class="mb-1 text-xl font-semibold">Daily reports</h1>
<p class="mb-4 text-sm text-[var(--color-muted)]">
  What the monitors saw, newest first. Findings link to work items; comments stick to the
  report.
</p>

{#if error}
  <p class="mb-4 rounded border border-red-700 bg-red-900/30 px-3 py-2 text-sm text-red-300">
    {error}
  </p>
{/if}

{#if rows.length === 0 && !error}
  <p class="text-sm text-[var(--color-muted)]">No reports yet — run `just report` in kmon.</p>
{/if}

<ul class="space-y-2">
  {#each rows as r (r.node_id)}
    <li class="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]">
      <button
        class="flex w-full items-center gap-3 px-4 py-3 text-left"
        onclick={() => toggle(r.node_id)}
        aria-expanded={expanded.has(r.node_id)}
      >
        <span class="text-[var(--color-muted)]">{expanded.has(r.node_id) ? "▾" : "▸"}</span>
        <span class="font-mono text-sm tabular-nums">{r.report_date}</span>
        <span
          class="rounded-full border px-2 py-0.5 text-xs font-medium uppercase tracking-wide {statusStyle[
            r.status
          ]}"
        >
          {r.status}
        </span>
        <span class="min-w-0 flex-1 truncate text-sm">{r.summary}</span>
        <span class="hidden shrink-0 text-xs text-[var(--color-muted)] sm:inline">
          {r.source}{r.model ? ` · ${r.model}` : ""}{r.escalated ? " · ESCALATED" : ""}
        </span>
      </button>

      {#if expanded.has(r.node_id)}
        <div class="border-t border-[var(--color-border)] px-4 py-4">
          {#if full[r.node_id]}
            {@const f = full[r.node_id]}
            <article class="prose prose-invert prose-sm max-w-none">
              <!-- eslint-disable-next-line svelte/no-at-html-tags — sanitized in renderMarkdown -->
              {@html renderMarkdown(f.body)}
            </article>

            {#if f.findings.length > 0}
              <div class="mt-4">
                <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-[var(--color-muted)]">
                  Findings
                </h3>
                <ul class="space-y-1">
                  {#each f.findings as w (w.wi_number)}
                    <li class="text-sm">
                      <a class="text-[var(--color-accent)] hover:underline" href="/work-items?wi={w.wi_number}">
                        #{w.wi_number}
                      </a>
                      {w.title}
                      <span class="text-xs text-[var(--color-muted)]">({w.wi_status})</span>
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}

            <div class="mt-4">
              <Comments node_id={r.node_id} />
            </div>
          {:else}
            <p class="text-sm text-[var(--color-muted)]">loading…</p>
          {/if}
        </div>
      {/if}
    </li>
  {/each}
</ul>

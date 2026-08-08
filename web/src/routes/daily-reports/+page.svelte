<script lang="ts">
  import { api, type ReportFull, type ReportRow, type SourceHealth } from "$lib/api";
  import Comments from "$lib/components/Comments.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";
  import { renderMarkdown } from "$lib/markdown";
  import { ID_CLASS, reportStatusPill, sourceFreshnessPill } from "$lib/domain";

  let rows = $state<ReportRow[]>([]);
  // #950: the sources themselves, above the reports. A source that filed daily
  // and stopped is the alert, and it has to sit on the surface a real problem
  // would use — a channel nobody looks at is not a channel.
  let sources = $state<SourceHealth[]>([]);
  let expanded = $state<Set<number>>(new Set());
  let full = $state<Record<number, ReportFull>>({});
  let error = $state<string | null>(null);
  // Findings open the shared preview panel (WI #231) rather than navigating
  // away. A finding is a work item, so its node id equals its wi_number.
  let previewNode = $state<number | null>(null);

  async function load() {
    try {
      [rows, sources] = await Promise.all([api.reports(), api.reportSources()]);
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
  report. A source that stopped filing is itself an alert — anything not
  <span class="font-medium">fresh</span> reports <span class="font-medium">unknown</span>,
  never its last known status.
</p>

{#if error}
  <p class="mb-4 rounded border border-red-700 bg-red-900/30 px-3 py-2 text-sm text-red-300">
    {error}
  </p>
{/if}

{#if sources.length > 0}
  <section class="mb-5">
    <h2 class="mb-2 text-xs font-semibold uppercase tracking-wide text-[var(--color-muted)]">
      Sources
    </h2>
    <ul class="space-y-1">
      {#each sources as s (s.source)}
        <li
          class="flex flex-wrap items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm"
        >
          <span class="font-mono">{s.source}</span>
          <span class={sourceFreshnessPill(s.freshness)}>{s.freshness}</span>
          <!-- The #950 rule, rendered: a source that is not fresh asserts
               `unknown`, and the API gives us nothing else to show. Restating a
               stale GREEN here is the failure this whole panel exists to end. -->
          <span class={reportStatusPill(s.asserts)}>{s.asserts}</span>
          <span class="flex-1"></span>
          {#if s.freshness === "stale"}
            <span class="text-xs text-red-300">
              nothing since {s.last_report_date} — {s.overdue_days}
              {s.overdue_days === 1 ? "day" : "days"} past due
            </span>
          {:else if s.freshness === "unrated"}
            <!-- Say WHY korg declined (#1097). "unrated" with no reason reads as
                 a bug; with the span it reads as a source that has not yet shown
                 a pattern — which is exactly what it is. -->
            <span class="text-xs text-[var(--color-muted)]">
              {s.report_count}
              {s.report_count === 1 ? "report" : "reports"}{s.history_span_days
                ? ` over ${s.history_span_days} days`
                : ""} — no cadence korg will believe yet; declare one to rate it
            </span>
          {:else if s.freshness === "retired"}
            <span class="text-xs text-[var(--color-muted)]">
              retired{s.note ? ` — ${s.note}` : ""}
            </span>
          {:else}
            <span class="text-xs text-[var(--color-muted)]">
              last filed {s.last_report_date} · every {s.cadence_days}
              {s.cadence_days === 1 ? "day" : "days"}{s.cadence_declared ? " (declared)" : ""}
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
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
        <!-- #980 — a report is addressed by node_id (get_report, and it is what
             a finding's `related` block cites). -->
        <span class={`shrink-0 ${ID_CLASS}`}>#{r.node_id}</span>
        <span class="font-mono text-sm tabular-nums">{r.report_date}</span>
        <span class={reportStatusPill(r.status)}>
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
                      <button
                        class="text-[var(--color-accent)] hover:underline"
                        title={`Preview #${w.wi_number}`}
                        onclick={() => (previewNode = w.wi_number)}
                      >
                        #{w.wi_number}
                      </button>
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

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

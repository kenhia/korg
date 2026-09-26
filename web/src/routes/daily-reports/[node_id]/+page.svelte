<script lang="ts">
  // One report, at its own URL (WI #1467), graduated to a page of its own
  // (WI #3294). NodeDetail's own header says how: "a kind that outgrows the
  // uniform view graduates; it does not get a new URL."
  //
  // What outgrew it was Ken reading #3202: the list row's blurb is the stored
  // `summary`, which the writer caps at 200 characters, so it stops mid-word
  // and nothing on screen had the rest. The whole sentence is the body's own
  // first paragraph, so the page shows the body and nothing in front of it —
  // the cap stays where kmon put it. A standfirst repeating that paragraph was
  // tried and ruled out as noise (overseer, korg:3310). The layout is the
  // proposal page's: the id and title, the one control the kind has (reviewed,
  // where a proposal has status), then the long text at a width you can read
  // prose in, then the linked work.
  import { page } from "$app/stores";
  import { api, type NodePreview as NodePreviewT, type ReportFull } from "$lib/api";
  import { ID_CLASS, docTitle, nodePage, reportStatusPill } from "$lib/domain";
  import BackTo from "$lib/components/BackTo.svelte";
  import Comments from "$lib/components/Comments.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import MarkdownView from "$lib/components/MarkdownView.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";
  import { attempt } from "$lib/toast.svelte";

  const nodeId = $derived(Number($page.params.node_id));

  let report = $state<ReportFull | null>(null);
  let loading = $state(true);
  let error = $state<unknown>(null);
  let missing = $state(false);
  // Node ids are one sequence across every kind, so `/daily-reports/<id>` is a
  // typo away from a real proposal. The generic view this page replaced said
  // so, and a page that graduates should not answer that case less well.
  let otherKind = $state<NodePreviewT | null>(null);
  let saving = $state(false);
  // Findings are work items, whose node id equals their wi_number — the same
  // peek-without-leaving gesture as the list page and the proposal page.
  let previewNode = $state<number | null>(null);

  function load(id: number) {
    report = null;
    otherKind = null;
    error = null;
    missing = false;
    loading = true;
    api
      .reportMaybe(id)
      .then(async (r) => {
        if (id !== nodeId) return;
        if (r !== null) {
          report = r;
          return;
        }
        missing = true;
        const n = await api.node(id).catch(() => null);
        if (id === nodeId) otherKind = n;
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

  // #2154's control, on the page you read the report in. Reconciled from the
  // response rather than optimistic: this page has one row, not a list that
  // wants to get shorter, so the list page's reason for optimism is absent.
  async function toggleReviewed() {
    if (!report || saving) return;
    saving = true;
    const want = !report.reviewed;
    const r = await attempt(
      () => api.reviewReport(report!.node_id, want),
      want ? "Mark report reviewed" : "Put report back",
    );
    if (r) report = { ...report, reviewed: r.reviewed };
    saving = false;
  }
</script>

<svelte:head>
  <title>{docTitle("report", nodeId)}</title>
</svelte:head>

<section class="space-y-5">
  <BackTo href="/daily-reports" label="← Reports" />

  {#if error}
    <ErrorNotice {error} what="this report" retry={() => load(nodeId)} />
  {:else if missing}
    {#if otherKind}
      <p
        class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm"
        data-testid="wrong-kind"
      >
        Node #{nodeId} is a <strong>{otherKind.kind}</strong>, not a report.
        {#if nodePage(otherKind.kind, nodeId)}
          <a class="underline" href={nodePage(otherKind.kind, nodeId)}>Open it where it lives ↗</a>
        {/if}
      </p>
    {:else}
      <p class="text-[var(--color-muted)]">No report with node id {nodeId}.</p>
    {/if}
  {:else if loading}
    <p class="text-[var(--color-muted)]">Loading report…</p>
  {:else if report}
    <header class="space-y-2 border-b border-[var(--color-border)] pb-3">
      <div class="flex flex-wrap items-baseline gap-2">
        <span class={ID_CLASS} data-testid="report-detail-id">#{report.node_id}</span>
        <h1 class="text-2xl font-semibold">{report.source} — {report.report_date}</h1>
        <span class={reportStatusPill(report.status)}>{report.status}</span>
        {#if report.escalated}
          <span
            class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]"
            >escalated</span
          >
        {/if}
      </div>
      <div class="flex flex-wrap items-center gap-2 text-xs">
        <button
          class={`rounded px-2 py-0.5 ${
            report.reviewed
              ? "bg-emerald-900/60 text-emerald-300"
              : "bg-[var(--color-surface-hi)] text-[var(--color-muted)]"
          }`}
          data-testid="report-detail-reviewed"
          aria-pressed={report.reviewed}
          disabled={saving}
          title={report.reviewed
            ? "Somebody has acted on this report — click to put it back"
            : "Mark this reviewed once you have acted on what it says"}
          onclick={toggleReviewed}>{report.reviewed ? "reviewed" : "mark reviewed"}</button
        >
        {#if report.model}
          <span class="text-[var(--color-muted)]">model {report.model}</span>
        {/if}
      </div>
    </header>

    <!-- `overflow-wrap: anywhere` because report bodies carry long unbreakable
         tokens — systemd unit names like `app-nvidia\x2dsettings\x2d…`,
         release paths — which pushed #3202 35px past a phone's width. -->
    <div class="max-w-3xl [overflow-wrap:anywhere]" data-testid="report-body">
      <h2 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">Report</h2>
      <MarkdownView src={report.body} />
    </div>

    {#if report.findings.length > 0}
      <div>
        <h2 class="mb-2 text-sm font-semibold">
          Findings
          <span class="font-normal text-[var(--color-muted)]"
            >· {report.findings.length}
            {report.findings.length === 1 ? "item" : "items"}</span
          >
        </h2>
        <ul class="space-y-1" data-testid="report-findings">
          {#each report.findings as w (w.wi_number)}
            <li
              class="flex flex-wrap items-baseline gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5"
            >
              <span class={ID_CLASS}>#{w.wi_number}</span>
              <button
                class="text-left text-sm hover:text-[var(--color-accent)]"
                title={`Preview #${w.wi_number}`}
                onclick={() => (previewNode = w.wi_number)}>{w.title}</button
              >
              <span class="text-xs text-[var(--color-muted)]">{w.wi_status}</span>
              <a
                class="ml-auto text-xs text-[var(--color-muted)] hover:text-[var(--color-accent)]"
                href={`/work-items/${w.wi_number}`}>open work item ↗</a
              >
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <!-- Comments on a report quote the same tokens (#3202's pushed the page
         23px wide once the body wrapped). Wrapped here, not in the shared
         Comments, so only Daily Reports changes. -->
    <div class="[overflow-wrap:anywhere]">
      <Comments node_id={report.node_id} />
    </div>
  {/if}
</section>

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

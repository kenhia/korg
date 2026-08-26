<script lang="ts">
  // One sprint proposal, full width (WI #1162).
  //
  // The slide-over the title opens is deliberately kept — Ken's ask names it as
  // the thing that works: "this is good for quick looks". What it is bad at is
  // the other half of the job. A proposal's `notes` is the unbounded analysis
  // field (#860), routinely a few hundred lines of measurements and sequencing,
  // and at `max-w-md` reading one is "a scroll and scroll some more". So the
  // pop-out stays on the title and this page gets its own affordance beside pin
  // and copy, exactly as the WI describes.
  //
  // Nothing new is *rendered* here that korg could not render before: the
  // Planning card and the generic preview both do Markdown already. This is
  // layout and a route — the same read (`GET /api/proposals/:node_id`) at a
  // width you can read prose in, with the covered items as a list rather than
  // a wrap of chips.
  import { page } from "$app/stores";
  import { api, type ProposalDetail, type RelatedRef } from "$lib/api";
  import { PROPOSAL_STATUSES, type ProposalStatus } from "$lib/generated/vocab";
  import { ID_CLASS, chip, nodePage, proposalStatusPill, stamp } from "$lib/domain";
  import BackTo from "$lib/components/BackTo.svelte";
  import Comments from "$lib/components/Comments.svelte";
  import CopyStart from "$lib/components/CopyStart.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import MarkdownView from "$lib/components/MarkdownView.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";
  import { attempt } from "$lib/toast.svelte";

  const nodeId = $derived(Number($page.params.node_id));

  let proposal = $state<ProposalDetail | null>(null);
  let loading = $state(true);
  let error = $state<unknown>(null);
  let missing = $state(false);
  let saving = $state(false);
  // Covered items are work items, whose node id equals their wi_number, so the
  // shared preview takes the number straight. Same gesture as the Planning
  // card's chips: peek without leaving the proposal you are reading.
  let previewNode = $state<number | null>(null);

  function load(id: number) {
    proposal = null;
    error = null;
    missing = false;
    loading = true;
    api
      .proposal(id)
      .then((p) => {
        // Guard against a stale response landing after the id changed.
        if (id !== nodeId) return;
        if (p === null) missing = true;
        else proposal = p;
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

  // The same lifecycle the Planning card drives, on the page you are reading
  // the proposal in — the program detail page set this precedent (#980's third
  // finding): a status you can read but not change from the page you are on
  // sends you back to a list to do it.
  async function setStatus(status: ProposalStatus) {
    if (!proposal || proposal.status === status) return;
    saving = true;
    const r = await attempt(
      () => api.updateProposal(proposal!.node_id, { status }),
      `Set proposal status to ${status}`,
    );
    if (r) proposal = { ...proposal, status: r.status };
    saving = false;
  }

  function refLabel(r: RelatedRef): string {
    return r.wi_number != null ? `#${r.wi_number} ${r.title}` : r.title;
  }
</script>

<svelte:head>
  <title>{proposal ? `${proposal.title} — korg` : "Proposal — korg"}</title>
</svelte:head>

<section class="space-y-5">
  <BackTo href="/planning" label="← Planning" />

  {#if error}
    <ErrorNotice {error} what="this proposal" retry={() => load(nodeId)} />
  {:else if missing}
    <p class="text-[var(--color-muted)]">No sprint proposal with node id {nodeId}.</p>
  {:else if loading}
    <p class="text-[var(--color-muted)]">Loading proposal…</p>
  {:else if proposal}
    <header class="space-y-2 border-b border-[var(--color-border)] pb-3">
      <div class="flex flex-wrap items-baseline gap-2">
        <!-- The id an agent is cited with: `/start-sprint korg:<node_id>` takes
             exactly this number, and matching it against agent output is why it
             sits beside the title on every other detail surface. -->
        <span class={ID_CLASS} data-testid="proposal-detail-id">#{proposal.node_id}</span>
        <h1 class="text-2xl font-semibold">{proposal.title}</h1>
        {#if proposal.project}<span class={chip.project}>{proposal.project}</span>{/if}
        {#each proposal.tags as t (t)}<span class={chip.tag}>#{t}</span>{/each}
        {#if proposal.pinned}<span class="text-xs" title="Pinned to the top of the queue">📌</span>{/if}
        <span class="ml-auto text-xs text-[var(--color-muted)]" data-testid="proposal-detail-stamps"
          >queued {stamp(proposal.created)}{#if proposal.updated !== proposal.created}
            · updated {stamp(proposal.updated)}{/if}</span
        >
      </div>
      <div class="flex flex-wrap items-center gap-1" data-testid="proposal-status-control">
        {#each PROPOSAL_STATUSES as s (s)}
          <button
            class={proposal.status === s
              ? proposalStatusPill(s)
              : "rounded border border-[var(--color-border)] px-2 py-0.5 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)]"}
            aria-pressed={proposal.status === s}
            disabled={saving || proposal.status === s}
            title={proposal.status === s ? `Status is ${s}` : `Set status to ${s}`}
            onclick={() => setStatus(s)}>{s}</button
          >
        {/each}
        <!-- WI #1550 — kfdc's pane opens this page directly, so the decision to
             start a proposal is made here and the button that starts it was on
             the other page. It sits with the status controls rather than up in
             the title row because starting a sprint *is* the lifecycle: the row
             now reads "what state is this in, and take it". -->
        <CopyStart
          nodeId={proposal.node_id}
          label
          class="ml-auto rounded border border-[var(--color-border)] px-2 py-0.5 text-xs hover:bg-[var(--color-surface-hi)]"
        />
      </div>
    </header>

    <!-- Summary and notes are the #860 split, and the page shows it as one:
         the ≤500-char routing contract reads as the standfirst it is, and the
         unbounded analysis below gets the full column. On the card these are a
         grey paragraph and a collapsed `<details>` — which is the right shape
         for a queue and the wrong one for reading. -->
    <p class="max-w-3xl text-[var(--color-muted)]" data-testid="proposal-summary">
      {proposal.summary}
    </p>

    {#if proposal.notes}
      <div class="max-w-3xl" data-testid="proposal-notes">
        <h2 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">Notes</h2>
        <MarkdownView src={proposal.notes} />
      </div>
    {/if}

    <div>
      <h2 class="mb-2 text-sm font-semibold">
        Covered work items
        <span class="font-normal text-[var(--color-muted)]"
          >· {proposal.covered.length}
          {proposal.covered.length === 1 ? "item" : "items"}</span
        >
      </h2>
      {#if proposal.covered.length === 0}
        <p class="text-sm text-[var(--color-muted)]">
          This proposal covers no work items yet — an agent relates them with the
          <code>covers</code> label.
        </p>
      {:else}
        <ul class="space-y-1" data-testid="proposal-covered">
          {#each proposal.covered as c (c.wi_number)}
            <li class="flex flex-wrap items-baseline gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5">
              <span class={ID_CLASS}>#{c.wi_number}</span>
              <button
                class="text-left text-sm hover:text-[var(--color-accent)]"
                title={`Preview #${c.wi_number}`}
                onclick={() => (previewNode = c.node_id)}>{c.title}</button
              >
              <span class="text-xs text-[var(--color-muted)]">{c.wi_status}</span>
              <span class="text-xs text-[var(--color-muted)]">{c.wi_tshirt}</span>
              {#if c.project && c.project !== proposal.project}
                <!-- A proposal is single-project since 0022, so a covered item
                     from elsewhere is worth seeing rather than assuming. -->
                <span class={chip.project}>{c.project}</span>
              {/if}
              {#if c.comment_count > 0}
                <span class="text-xs text-[var(--color-muted)]" title={`${c.comment_count} comments`}
                  >💬 {c.comment_count}</span
                >
              {/if}
              <!-- Was `/work-items?wi=<n>`, a URL the Work Items page never
                   read: it landed on the unfiltered list and did nothing with
                   the parameter. #1467 gave work items a page of their own, so
                   this now opens the item. -->
              <a
                class="ml-auto text-xs text-[var(--color-muted)] hover:text-[var(--color-accent)]"
                href={`/work-items/${c.wi_number}`}>open work item ↗</a
              >
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if proposal.related.length > 0}
      <!-- The LB-3 block, minus `covers` (that is `covered`) — so this is
           has_handoff, depends_on, related-to. A handoff hanging off a proposal
           is required reading before starting the sprint, which is exactly why
           it gets a link rather than a mention. -->
      <div>
        <h2 class="mb-2 text-sm font-semibold">Related</h2>
        <ul class="space-y-1 text-sm">
          {#each proposal.related as r (r.rel_id)}
            <li class="flex flex-wrap items-baseline gap-2">
              <span class="text-xs text-[var(--color-muted)]">{r.label}</span>
              {#if nodePage(r.kind, r.node_id)}
                <a class="hover:underline" href={nodePage(r.kind, r.node_id)}
                  >{refLabel(r)}</a
                >
              {:else}
                <span>{refLabel(r)}</span>
              {/if}
            </li>
          {/each}
        </ul>
        {#if proposal.related_truncated}
          <p class="mt-1 text-xs text-[var(--color-muted)]">More edges exist than are shown.</p>
        {/if}
      </div>
    {/if}

    <Comments node_id={proposal.node_id} />
  {/if}
</section>

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

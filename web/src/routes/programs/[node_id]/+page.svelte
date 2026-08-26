<script lang="ts">
  // One program: its aim, its notes, and its ordered slices with the work-item
  // rollup each one carries (#968, D-5).
  //
  // The whole page is ONE fetch. That is the design constraint the rollup read
  // exists to satisfy — the pre-044 way to render this would have been
  // program → neighbors → per-proposal get_proposal → per-proposal work items,
  // which is exactly the crawl `get_program` replaces.
  import { page } from "$app/stores";
  import { api, type ProgramDetail, type ProgramSlice } from "$lib/api";
  import type { ProgramStatus } from "$lib/generated/vocab";
  import MarkdownView from "$lib/components/MarkdownView.svelte";
  import {
    ID_CLASS,
    PROGRESS_VERIFIED_CLASS,
    PROGRESS_WORK_CLASS,
    chip,
    nodePage,
    programStatusStyle,
    sliceProgress,
    sliceUnfinished,
    stamp,
  } from "$lib/domain";
  import { PROGRAM_STATUSES } from "$lib/generated/vocab";
  import { attempt } from "$lib/toast.svelte";
  import Comments from "$lib/components/Comments.svelte";
  import Dialog from "$lib/components/Dialog.svelte";
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


  // #980's third finding (filed as a comment, not its own WI): this page
  // rendered status as a read-only pill while `PATCH /api/programs/:node_id`
  // sat unused, so Ken could not mark 979 done from the browser — it was done
  // via REST on his behalf. Same shape as the proposal status buttons on
  // Planning. With D-7 in play this control IS the "answer the ask" gesture for
  // a program row: marking one done clears its awaiting marker server-side,
  // which is why it sits beside the awaiting note rather than in a menu.
  let saving = $state(false);
  async function commitStatus(status: ProgramStatus) {
    if (!program || program.status === status) return;
    saving = true;
    const r = await attempt(
      () => api.updateProgram(program!.node_id, { status }),
      `Set program status to ${status}`,
    );
    if (r) program = { ...program, status: r.status };
    saving = false;
  }

  // #1168 — the close-out confirmation. The bare buttons above shipped in 049;
  // what Ken asked for and did not get was the gate on the one transition that
  // means "this is over".
  //
  // Enabled always, with a confirm — his own preferred half of the ask, and the
  // right one: a control disabled until every slice is finished cannot close
  // the program that is *actually* finished but has a slice nobody will ever
  // pick up, which is the case he hits. Marking done is also reversible (the
  // `active` button is right there), so this is a "did you mean it" and not a
  // destructive-action confirm — the difference ConfirmButton's scope note
  // draws, and why this is a dialog that names the unfinished slices rather
  // than an inline two-step that just says "Sure?".
  const unfinished = $derived(
    (program?.slices ?? [])
      .map((s) => ({ slice: s, why: sliceUnfinished(s) }))
      .filter((u): u is { slice: ProgramSlice; why: string } => u.why !== null),
  );

  let confirming = $state(false);

  async function setStatus(status: ProgramStatus) {
    if (status === "done" && unfinished.length > 0) {
      confirming = true;
      return;
    }
    await commitStatus(status);
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
        <!-- #980: agents cite this program as `korg:979`, so the number Ken is
             matching against agent output belongs beside the title. -->
        <span class={ID_CLASS} data-testid="program-id">#{program.node_id}</span>
        <h1 class="text-2xl font-semibold">{program.title}</h1>
        <!-- The derived span (D-6). A program has no project of its own; this
             is the union of its slices' projects, and it is the honest answer
             to "which repos does this touch". -->
        {#each program.span as project (project)}
          <span class={chip.project}>{project}</span>
        {/each}
        <!-- WI #1106 — tags and the "how current is this" stamp, the same pair
             the handoff page has carried since #621 and for the same reason: a
             program is long-lived, so how recently it moved is part of reading
             it. Not on the list rows, which have no room and are a routing
             surface rather than a reading one. -->
        {#each program.tags as t (t)}<span class={chip.tag}>#{t}</span>{/each}
        <span
          class="ml-auto text-xs text-[var(--color-muted)]"
          data-testid="program-stamps">updated {stamp(program.updated)}</span
        >
      </div>
      <p class="text-[var(--color-muted)]">{program.aim}</p>
      <!-- The status control, not a pill (#980's third finding). The current
           status keeps the pill styling so the header still reads at a glance;
           the other two are plain buttons beside it. -->
      <div
        class="flex flex-wrap items-center gap-1"
        data-testid="program-status-control"
      >
        {#each PROGRAM_STATUSES as s (s)}
          <button
            class={program.status === s
              ? `rounded px-2 py-0.5 text-xs ${programStatusStyle(s)}`
              : "rounded border border-[var(--color-border)] px-2 py-0.5 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)]"}
            aria-pressed={program.status === s}
            disabled={saving || program.status === s}
            title={program.status === s
              ? `Status is ${s}`
              : `Set status to ${s}`}
            onclick={() => setStatus(s)}>{s}</button
          >
        {/each}
      </div>
    </header>

    {#if program.notes}
      <MarkdownView src={program.notes} />
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
        <!-- The legend, once, above the list. Three bare numbers with no key
             are what produced this bug in the first place — Ken read the left
             figure as "work remaining" — so the meaning ships next to the
             figures rather than in a tooltip nobody hovers. -->
        <p class="mb-1 text-[0.65rem] text-[var(--color-muted)]">
          <span class="font-mono text-amber-400">work complete</span> /
          <span class="font-mono text-emerald-400">verified by you</span> /
          <span class="font-mono">total</span>
        </p>
        <ol class="space-y-2">
          {#each program.slices as s, i (s.node_id)}
            {@const p = sliceProgress(s)}
            <li
              class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
              data-testid={`slice-${s.node_id}`}
            >
              <div class="flex flex-wrap items-baseline gap-2">
                <span class="text-xs tabular-nums text-[var(--color-muted)]"
                  >{i + 1}</span
                >
                <!-- The slice's proposal node_id — what `/start-sprint
                     korg:<id>` takes, and what an agent names when it reports
                     on a slice (#980). -->
                <span class={ID_CLASS}>#{s.node_id}</span>
                <span class="font-medium">{s.title}</span>
                {#if s.project}<span class={chip.project}>{s.project}</span>{/if}
                <span class="text-xs text-[var(--color-muted)]">{s.status}</span>
                <span
                  class="ml-auto text-xs tabular-nums text-[var(--color-muted)]"
                  data-testid="slice-progress"
                  title={`${p.workComplete} of ${p.total} implemented (resolved, done or closed) · ${p.verified} verified by you (closed)`}
                >
                  {#if p.total === 0}
                    no work items
                  {:else}
                    <span class="text-amber-400">{p.workComplete}</span>
                    / <span class="text-emerald-400">{p.verified}</span>
                    / {p.total}
                  {/if}
                </span>
              </div>
              {#if p.total > 0}
                <!-- The bar encodes the SAME three states as the triplet, from
                     the same `sliceProgress` call, so the two cannot disagree —
                     which is the failure #980 is about. The verified run is
                     drawn over the work-complete one rather than beside it, so
                     "all amber" reads as "all of this is waiting on you". -->
                <div
                  class="relative mt-2 h-1 overflow-hidden rounded bg-[var(--color-surface-hi)]"
                >
                  <div
                    class={`absolute inset-y-0 left-0 ${PROGRESS_WORK_CLASS}`}
                    style={`width: ${p.workPct}%`}
                  ></div>
                  <div
                    class={`absolute inset-y-0 left-0 ${PROGRESS_VERIFIED_CLASS}`}
                    style={`width: ${p.verifiedPct}%`}
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
              <span class={ID_CLASS}>#{r.wi_number ?? r.node_id}</span>
              <!-- Was `kind === "handoff"` hardcoded; `nodePage` covers every
                   kind with a route of its own, so a related program links too
                   (#982). -->
              {#if nodePage(r.kind, r.node_id)}
                <a class="hover:underline" href={nodePage(r.kind, r.node_id)}
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

{#if confirming}
  <Dialog
    open={true}
    onClose={() => (confirming = false)}
    placement="center"
    title="Close out this program?"
    class="max-w-lg"
  >
    <div class="space-y-3" data-testid="program-close-confirm">
      <p class="text-sm">
        {unfinished.length}
        {unfinished.length === 1 ? "slice is" : "slices are"} not finished. Marking this
        program done says the whole arc is over — you can set it back to active if
        it isn't.
      </p>
      <ul class="space-y-1 text-sm">
        {#each unfinished as u (u.slice.node_id)}
          <li class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5">
            <span class={ID_CLASS}>#{u.slice.node_id}</span>
            <span class="ml-1">{u.slice.title}</span>
            <span class="ml-1 text-xs text-[var(--color-muted)]">— {u.why}</span>
          </li>
        {/each}
      </ul>
      <div class="flex justify-end gap-2">
        <button
          class="rounded border border-[var(--color-border)] px-3 py-1 text-sm hover:bg-[var(--color-surface-hi)]"
          onclick={() => (confirming = false)}>Cancel</button
        >
        <button
          class="rounded bg-[var(--color-accent-soft)] px-3 py-1 text-sm hover:bg-[var(--color-accent)]"
          data-testid="program-close-anyway"
          disabled={saving}
          onclick={async () => {
            confirming = false;
            await commitStatus("done");
          }}>Mark done anyway</button
        >
      </div>
    </div>
  </Dialog>
{/if}

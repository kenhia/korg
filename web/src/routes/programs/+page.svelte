<script lang="ts">
  // The program list (#968) — the cross-project layer, one level above the
  // Planning queue.
  //
  // Deliberately minimal: kfdc's Operations panel is the real consumer of this
  // data, and the point of shipping a page here is that a program is visible
  // and reachable in korg itself, not that korg renders it beautifully.
  //
  // Note there is no project filter — a program has no project. `span`, derived
  // from the projects of the proposals it includes, is what each row shows
  // instead, and it is the column worth reading: it is the answer to "which
  // repos does this touch" that a proposal could never give honestly.
  //
  // #1168 asked whether the close-out control belongs on these rows too, since
  // the list is where a finished program is most likely to be *noticed*. It
  // does not, and the reason is in the payload: `ProgramRow` carries
  // `slice_count` and `span`, and no per-slice rollup. A close-out button here
  // could only warn blindly or not warn at all — and the warning IS the item.
  // The row already links to the page that has both the counts and the control.
  import { onMount } from "svelte";
  import { api, type ProgramRow } from "$lib/api";
  import { ID_CLASS, chip, partitionParkedByStatus, programStatusStyle } from "$lib/domain";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";

  let programs = $state<ProgramRow[]>([]);
  // korg already returns parked programs last (#1535); this recovers the
  // boundary that order implies, because a divider needs two lists rather than
  // a sequence. Same treatment #810 gave parked work items — visible, below a
  // labelled rule, never collapsed: hiding is what `done` is for.
  const listed = $derived(partitionParkedByStatus(programs));
  let omitted = $state({ done: 0, archived: 0 });
  let showAll = $state(false);
  let loading = $state(true);
  let loadError = $state<unknown>(null);

  async function load() {
    loading = true;
    loadError = null;
    try {
      const r = await api.programs(showAll ? "all" : undefined);
      programs = r.items;
      omitted = r.omitted;
    } catch (e) {
      loadError = e;
    } finally {
      loading = false;
    }
  }


  onMount(load);
</script>

<section class="space-y-4">
  <header class="flex flex-wrap items-end justify-between gap-3">
    <div>
      <p
        class="text-xs font-medium uppercase tracking-[0.18em] text-[var(--color-accent)]"
      >
        Programs
      </p>
      <h1 class="text-2xl font-semibold">Work that spans repos</h1>
    </div>
    <label class="flex items-center gap-2 text-sm text-[var(--color-muted)]">
      <input
        type="checkbox"
        bind:checked={showAll}
        onchange={load}
        class="accent-[var(--color-accent)]"
      />
      Show finished
    </label>
  </header>

  {#if loadError}
    <ErrorNotice error={loadError} what="programs" retry={load} />
  {:else if loading}
    <p class="text-[var(--color-muted)]">Loading programs…</p>
  {:else if programs.length === 0}
    <p class="text-[var(--color-muted)]">
      No programs yet. A program bundles sprint proposals across repos — a
      proposal itself stays single-project.
    </p>
  {:else}
    <ul class="space-y-2">
      {#each listed.active as p (p.node_id)}
        {@render program(p)}
      {/each}
      {#if listed.parked.length > 0}
        <li
          class="border-t border-[var(--color-border)] px-1 pb-1 pt-3 text-xs uppercase tracking-wide text-[var(--color-muted)]"
          data-testid="parked-divider"
        >
          <span class="mr-2">parked</span>
          <span class="text-[0.65rem] normal-case tracking-normal"
            >waiting on a condition, not on you — {listed.parked.length}
            {listed.parked.length === 1 ? "program" : "programs"}</span
          >
        </li>
        {#each listed.parked as p (p.node_id)}
          {@render program(p)}
        {/each}
      {/if}
    </ul>
  {/if}

  {#if !showAll && omitted.done > 0}
    <p class="text-xs text-[var(--color-muted)]">
      {omitted.done} finished {omitted.done === 1 ? "program" : "programs"} hidden.
    </p>
  {/if}
</section>

{#snippet program(p: ProgramRow)}
  <li
    class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
  >
    <div class="flex flex-wrap items-baseline gap-2">
      <span class={ID_CLASS}>#{p.node_id}</span>
      <a
        class="font-semibold hover:underline"
        href={`/programs/${p.node_id}`}>{p.title}</a
      >
      <span
        class={`rounded px-1.5 py-0.5 text-xs ${programStatusStyle(p.status)}`}
        >{p.status}</span
      >
      <span class="text-xs text-[var(--color-muted)]">
        {p.slice_count}
        {p.slice_count === 1 ? "slice" : "slices"}
      </span>
      {#each p.span as project (project)}
        <span class={chip.project}>{project}</span>
      {/each}
    </div>
    <p class="mt-1 text-sm text-[var(--color-muted)]">{p.aim}</p>
  </li>
{/snippet}

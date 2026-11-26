<script lang="ts">
  // One work item, at its own URL (WI #1467).
  //
  // This route is the one that was *already being linked to* and did not exist:
  // `nodeHref` returned `/work-items?wi=<n>` and the Work Items page read no
  // URL parameters at all, so every such link — the proposal detail page's
  // "open in Work Items", the awaiting lane, search's "open in context" —
  // landed on the unfiltered list and did nothing. The audit #1467 asked for
  // found it; this is the fix.
  //
  // It was read-only at first, on the theory that the list page is where work
  // items are edited. WI #5024 retired that: this is the page "find by ID" and
  // every pasted link land on, and editing from here meant "Show in project",
  // reopening the item, then Edit. So it now carries the list page's own
  // `WorkItemForm`, edited in place. The form is self-contained — it needs the
  // item row and its project's areas, nothing from the list's filters or rail —
  // which is why it is reused rather than deep-linked into the list view.
  import { page } from "$app/stores";
  import { api, type WorkItemRow } from "$lib/api";
  import BackTo from "$lib/components/BackTo.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import NodeDetail from "$lib/components/NodeDetail.svelte";
  import WorkItemForm from "$lib/components/WorkItemForm.svelte";

  // A work item's node id IS its wi_number since the 0009 identity migration,
  // so the parameter is spelled `wi_number` (the handle people type) and used
  // as a node id without conversion.
  const nodeId = $derived(Number($page.params.wi_number));

  // The full row, fetched when Edit is pressed rather than on load: the read
  // view is `NodeDetail`'s uniform preview, and a second fetch on every visit
  // would pay for an editor most visits never open.
  let editItem = $state<WorkItemRow | null>(null);
  let editAreas = $state<{ id: number; name: string }[]>([]);
  let opening = $state(false);
  let openError = $state<unknown>(null);
  // Bumped after a save so `NodeDetail` remounts and re-reads the item; the
  // read view must show what was just saved, not what was loaded before it.
  let generation = $state(0);

  // Navigating to another id while the editor is open closes it: the form
  // belongs to the item it was opened on.
  $effect(() => {
    void nodeId;
    editItem = null;
    openError = null;
  });

  async function openEditor() {
    const id = nodeId;
    opening = true;
    openError = null;
    try {
      const item = await api.workItem(id);
      if (id !== nodeId) return;
      if (item === null) throw new Error(`No work item #${id}`);
      editAreas = item.project ? await api.areas(item.project) : [];
      if (id !== nodeId) return;
      editItem = item;
    } catch (e) {
      if (id === nodeId) openError = e;
    } finally {
      opening = false;
    }
  }
</script>

<section class="space-y-4">
  <div class="flex items-center gap-2">
    <BackTo href="/work-items" label="← Work items" />
    <!-- The same destination the control above goes to, and that is the point:
         "back" leaves the scope alone, "Show in project" (WI #2380) sets it to
         this item's project on the way. -->
    {#if editItem === null}
      <button
        class="ml-auto rounded bg-[var(--color-accent-soft)] px-3 py-1 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40"
        data-testid="wi-edit"
        disabled={opening}
        onclick={openEditor}>Edit</button
      >
    {/if}
  </div>

  {#if openError}
    <ErrorNotice error={openError} what="this work item for editing" retry={openEditor} />
  {/if}

  {#if editItem}
    <div class="max-w-5xl">
      <WorkItemForm
        {editItem}
        projectId={undefined}
        areas={editAreas}
        onSaved={() => {
          editItem = null;
          generation += 1;
        }}
        onCancel={() => (editItem = null)}
      />
    </div>
  {:else}
    {#key generation}
      <NodeDetail {nodeId} expect="workitem" projectRail="/work-items" />
    {/key}
  {/if}
</section>

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
  // The list page stays the place work items are *edited* — it holds the
  // filters, the rail and the inline editor. This is the place one is read and
  // linked to, which is a different job.
  import { page } from "$app/stores";
  import BackTo from "$lib/components/BackTo.svelte";
  import NodeDetail from "$lib/components/NodeDetail.svelte";

  // A work item's node id IS its wi_number since the 0009 identity migration,
  // so the parameter is spelled `wi_number` (the handle people type) and used
  // as a node id without conversion.
  const nodeId = $derived(Number($page.params.wi_number));
</script>

<section class="space-y-4">
  <BackTo href="/work-items" label="← Work items" />
  <NodeDetail {nodeId} expect="workitem" />
</section>

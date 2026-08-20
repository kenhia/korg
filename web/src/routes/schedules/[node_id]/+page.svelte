<script lang="ts">
  // One schedule, at its own URL (WI #1467).
  //
  // Due-ness is deliberately absent here, for the reason `get_node_preview`
  // gives: it is a read-time predicate over three clauses and the schedules
  // page renders it with the context that makes it mean something. A second,
  // thinner derivation on a detail page is the drift `SCHEDULE_DUE_SQL` exists
  // to prevent.
  import { page } from "$app/stores";
  import BackTo from "$lib/components/BackTo.svelte";
  import NodeDetail from "$lib/components/NodeDetail.svelte";

  const nodeId = $derived(Number($page.params.node_id));
</script>

<section class="space-y-4">
  <BackTo href="/schedules" label="← Schedules" />
  <NodeDetail {nodeId} expect="schedule" />
</section>

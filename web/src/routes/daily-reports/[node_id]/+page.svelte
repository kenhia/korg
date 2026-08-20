<script lang="ts">
  // One report, at its own URL (WI #1467). kfo writes these and cites them by
  // node id; until now that citation had nowhere to point.
  import { page } from "$app/stores";
  import BackTo from "$lib/components/BackTo.svelte";
  import NodeDetail from "$lib/components/NodeDetail.svelte";

  const nodeId = $derived(Number($page.params.node_id));
</script>

<section class="space-y-4">
  <BackTo href="/daily-reports" label="← Reports" />
  <NodeDetail {nodeId} expect="report" />
</section>

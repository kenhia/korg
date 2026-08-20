<script lang="ts">
  // One reading-list link, at its own URL (WI #1467).
  //
  // Two different URLs live on this page and confusing them is the obvious
  // trap: this route is korg's page *about* the link, and the `URL` field in
  // the body is the thing being read. The field is the one that leaves korg.
  import { page } from "$app/stores";
  import BackTo from "$lib/components/BackTo.svelte";
  import NodeDetail from "$lib/components/NodeDetail.svelte";

  const nodeId = $derived(Number($page.params.node_id));
</script>

<section class="space-y-4">
  <BackTo href="/reading-list" label="← Reading list" />
  <NodeDetail {nodeId} expect="link" />
</section>

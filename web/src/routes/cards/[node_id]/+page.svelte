<script lang="ts">
  // One card, at its own URL (WI #1467). The kanban is where a card is *worked*;
  // this is where it is linked to.
  import { page } from "$app/stores";
  import BackTo from "$lib/components/BackTo.svelte";
  import NodeDetail from "$lib/components/NodeDetail.svelte";

  const nodeId = $derived(Number($page.params.node_id));
</script>

<section class="space-y-4">
  <BackTo href="/cards" label="← Cards" />
  <NodeDetail {nodeId} expect="card" />
</section>

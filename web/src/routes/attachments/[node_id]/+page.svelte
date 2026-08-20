<script lang="ts">
  // One attachment, at its own URL (WI #1467).
  //
  // There is no attachments *list* page — an attachment is reached from the
  // node that owns it — so this route's back link goes to the owner where
  // `NodeDetail`'s related block found one, and the image itself is rendered
  // above the metadata: a screenshot's answer to "what is this" is the picture.
  import { page } from "$app/stores";
  import NodeDetail from "$lib/components/NodeDetail.svelte";

  const nodeId = $derived(Number($page.params.node_id));
</script>

<section class="space-y-4">
  <NodeDetail {nodeId} expect="attachment" />
</section>

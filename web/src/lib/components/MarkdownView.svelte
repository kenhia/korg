<script lang="ts">
  // Markdown with korg's attachments in it (#1120). The one renderer every
  // prose surface goes through, so an image pasted into a work item looks and
  // behaves the same in the detail panel, the review page and the node preview.
  //
  // `renderMarkdown` turns each `![img-<hex>](…)` token into a thumbnail
  // *button* carrying `data-img-id`; this component supplies the behaviour that
  // markup cannot — one lightbox, opened by whichever thumbnail was clicked.
  //
  // The listener is delegated and attached in code rather than written as
  // `onclick` on the wrapper: the interactive elements are the buttons the
  // renderer emitted, and putting a click handler on their container would be
  // claiming the container is interactive when it is not.

  import { renderMarkdown } from "$lib/markdown";
  import Lightbox from "./Lightbox.svelte";

  let {
    src,
    class: cls = "prose prose-invert max-w-none text-sm",
    style = "",
    testid = undefined,
  }: {
    src: string | null | undefined;
    class?: string;
    style?: string;
    testid?: string;
  } = $props();

  let el = $state<HTMLDivElement | null>(null);
  let lightbox = $state<string | null>(null);

  $effect(() => {
    const host = el;
    if (!host) return;
    const onClick = (e: MouseEvent) => {
      const hit = (e.target as HTMLElement | null)?.closest?.("[data-img-id]");
      const id = hit?.getAttribute("data-img-id");
      if (id) lightbox = id;
    };
    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
  });
</script>

<!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized in renderMarkdown -->
<div bind:this={el} class={cls} {style} data-testid={testid}>{@html renderMarkdown(src)}</div>

<Lightbox imgId={lightbox} onClose={() => (lightbox = null)} />

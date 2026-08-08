<script lang="ts">
  // Full-resolution view of one attachment (#1121). Shared by every surface
  // that shows a thumbnail — inline in a body, inline in a comment, or in the
  // attachment list — so "click the picture, see the picture" means the same
  // thing everywhere.
  //
  // The original is fetched only when this opens. That is the whole reason
  // thumbnails exist: a work item with a dozen screenshots renders a dozen
  // 400px images, and the 4K bytes are loaded for the one Ken actually clicks.

  import Dialog from "./Dialog.svelte";
  import { imgUrl } from "$lib/img";

  let {
    imgId,
    onClose,
    caption = null,
  }: {
    /** The attachment to show, or null for closed. */
    imgId: string | null;
    onClose: () => void;
    /** Filename or other context, shown under the image. */
    caption?: string | null;
  } = $props();
</script>

{#if imgId}
  <Dialog
    open={true}
    {onClose}
    placement="lightbox"
    title={caption ?? imgId}
    titleHidden
    class="backdrop:bg-black/80"
  >
    <figure class="flex flex-col items-center gap-2" data-testid="lightbox">
      <!-- `max-h` leaves room for the caption and the close row; the image
           scales to fit rather than forcing the dialog to scroll, because a
           lightbox that needs scrolling to show one picture has failed. -->
      <img
        src={imgUrl(imgId)}
        alt={caption ?? imgId}
        data-testid="lightbox-image"
        class="max-h-[82dvh] max-w-full rounded object-contain"
      />
      <!-- Its own opaque strip: the caption floats over whatever the page was
           showing, and a translucent one reads as page text at a glance. -->
      <figcaption class="flex items-center gap-3 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-xs text-[var(--color-muted)]">
        <span class="font-mono">{imgId}</span>
        {#if caption && caption !== imgId}<span>{caption}</span>{/if}
        <!-- The original, addressable: what a "save this somewhere" or a
             paste-into-a-message reaches for. -->
        <a class="hover:text-[var(--color-accent)] hover:underline" href={imgUrl(imgId)} target="_blank" rel="noreferrer">open original ↗</a>
      </figcaption>
    </figure>
  </Dialog>
{/if}

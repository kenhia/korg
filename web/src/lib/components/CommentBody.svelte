<script lang="ts">
  // A comment, with the images it places (#1120).
  //
  // Comments render as literal text in korg and keep doing so: a `#582` is a
  // work-item reference and a `*` is an asterisk. The only thing lifted out of
  // the text is korg's own `![img-<hex>](…)` token, which becomes the same
  // thumbnail control an inline image gets in a body — the "comments
  // cross-reference attachments, without duplicating the upload chrome" half of
  // #1121.

  import { imgUrl, splitImgTokens, THUMB_CLASS } from "$lib/img";

  let {
    body,
    onOpen,
  }: {
    body: string;
    /** Show this attachment full size — the parent owns the one lightbox. */
    onOpen: (imgId: string) => void;
  } = $props();

  const segments = $derived(splitImgTokens(body));
</script>

<span class="flex-1">
  {#each segments as seg, i (i)}
    {#if seg.kind === "text"}<span class="whitespace-pre-wrap">{seg.text}</span>
    {:else}
      <button
        type="button"
        class={THUMB_CLASS}
        title={`${seg.id} — open full size`}
        data-img-id={seg.id}
        onclick={() => onOpen(seg.id)}
      >
        <img src={imgUrl(seg.id, "thumb")} alt={seg.id} loading="lazy" />
      </button>
    {/if}
  {/each}
</span>

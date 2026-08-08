<script lang="ts">
  // The explicit half of the images UI (#1121): the paperclip, the list, the
  // discard.
  //
  // Inline paste covers the images that belong *in* the prose. This covers
  // everything else — a file attached without a token, an image whose token was
  // deleted, and the only surface from which an attachment can actually be
  // destroyed. That asymmetry is the design (handoff D2): placement lives in
  // markdown and existence lives here, so deleting a token is free and
  // discarding is deliberate.
  //
  // The list is not fetched: `get_work_item` already inlines `attachments`, so
  // this renders what the detail read carried and asks its parent to reload
  // after a change rather than keeping a second copy of the same truth.

  import type { AttachmentRow } from "$lib/api";
  import { api } from "$lib/api";
  import { attempt } from "$lib/toast.svelte";
  import { bodyPlacesImg, formatBytes, imgUrl } from "$lib/img";
  import ConfirmButton from "./ConfirmButton.svelte";
  import Lightbox from "./Lightbox.svelte";

  let {
    attachments,
    ownerNodeId,
    bodies = [],
    onChanged,
  }: {
    attachments: AttachmentRow[];
    /** The node that owns these — what an uploaded file is linked to. */
    ownerNodeId: number;
    /** Every text this node's images could be placed in (content, details,
     *  comment bodies), for the inline-or-not indicator. */
    bodies?: (string | null | undefined)[];
    /** Reload the owner, so the list reflects what just changed. */
    onChanged: () => void;
  } = $props();

  let lightbox = $state<string | null>(null);
  let uploading = $state(false);
  let fileInput = $state<HTMLInputElement | null>(null);

  async function attachFiles(files: FileList | null) {
    if (!files || files.length === 0) return;
    uploading = true;
    // Uploaded with the owner set: a paperclip attach is not paste-before-save,
    // there is a node to belong to already, and it should survive whether or
    // not anything is typed afterwards.
    for (const file of files) {
      const row = await attempt(
        () => api.uploadImage(file, ownerNodeId),
        `Attach ${file.name}`,
      );
      if (!row) break;
    }
    uploading = false;
    if (fileInput) fileInput.value = "";
    onChanged();
  }

  async function discard(row: AttachmentRow) {
    const r = await attempt(() => api.deleteImage(row.img_id), `Discard ${row.img_id}`);
    if (r) onChanged();
  }
</script>

<section data-testid="attachments">
  <div class="mb-1 flex items-center justify-between border-b border-[var(--color-border)] pb-1">
    <h3 class="text-sm font-semibold">
      Attachments
      {#if attachments.length > 0}<span class="ml-1 text-xs font-normal text-[var(--color-muted)]">{attachments.length}</span>{/if}
    </h3>
    <!-- The paperclip is a label driving a hidden file input: the native
         picker, without the button-that-clicks-an-input dance. -->
    <label
      class="cursor-pointer rounded bg-[var(--color-surface-hi)] px-2 py-0.5 text-xs hover:bg-[var(--color-accent-soft)]"
      title="Attach an image"
    >
      {uploading ? "Uploading…" : "📎 Attach"}
      <input
        bind:this={fileInput}
        type="file"
        accept="image/*"
        multiple
        class="sr-only"
        data-testid="attach-input"
        onchange={(e) => attachFiles(e.currentTarget.files)}
      />
    </label>
  </div>

  {#if attachments.length > 0}
    <ul class="space-y-1">
      {#each attachments as a (a.node_id)}
        {@const inline = bodyPlacesImg(a.img_id, ...bodies)}
        <li class="flex items-center gap-2 rounded bg-[var(--color-bg)] px-2 py-1 text-xs">
          <button
            type="button"
            class="shrink-0 rounded border border-[var(--color-border)] hover:border-[var(--color-accent)]"
            title={`${a.img_id} — open full size`}
            onclick={() => (lightbox = a.img_id)}
          >
            <img src={imgUrl(a.img_id, "thumb")} alt={a.filename} class="h-10 w-10 rounded object-cover" loading="lazy" />
          </button>
          <span class="font-mono text-[var(--color-accent)]">{a.img_id}</span>
          <span class="truncate text-[var(--color-muted)]" title={a.filename}>{a.filename}</span>
          <span class="shrink-0 text-[var(--color-muted)]">{a.width}×{a.height} · {formatBytes(a.byte_size)}</span>
          <!-- Inline-or-not, and pending-or-not: the two things about an
               attachment that are not visible from the picture. `pending` is
               the sweeper's shortlist, so saying so is a warning, not trivia. -->
          {#if a.state === "pending"}
            <span class="shrink-0 rounded bg-amber-950 px-1 text-amber-300" title="Not owned by anything yet — swept 24h after upload">pending</span>
          {:else if inline}
            <span class="shrink-0 rounded bg-[var(--color-surface-hi)] px-1 text-[var(--color-muted)]" title="Placed in the text">inline</span>
          {:else}
            <span class="shrink-0 rounded bg-[var(--color-surface-hi)] px-1 text-[var(--color-muted)]" title="Attached, but not placed in any text">not inline</span>
          {/if}
          <!-- The only destructive path for an image, and it takes the bytes
               with it — so it confirms, like every other undoable-by-nothing
               action in korg (WI #549). -->
          <ConfirmButton
            label={`Discard ${a.img_id}`}
            class="ml-auto shrink-0 rounded px-1 text-[var(--color-muted)] hover:text-red-400"
            armedClass="ml-auto shrink-0 rounded bg-red-900 px-1 text-red-100"
            onconfirm={() => discard(a)}
          >
            ✕
          </ConfirmButton>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-xs text-[var(--color-muted)]">
      No attachments. Paste a screenshot into the editor, or use 📎.
    </p>
  {/if}
</section>

<Lightbox imgId={lightbox} onClose={() => (lightbox = null)} />

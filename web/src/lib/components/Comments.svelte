<script lang="ts">
  import { api, type Comment } from "$lib/api";
  import { attempt } from "$lib/toast.svelte";
  import { PasteUploads, pasteImages } from "$lib/imagePaste.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import ConfirmButton from "./ConfirmButton.svelte";
  import CommentBody from "./CommentBody.svelte";
  import Lightbox from "./Lightbox.svelte";

  // `comments` is bindable so a parent can read the loaded bodies (e.g. to
  // extract launch URLs) without owning the fetch/add/delete logic.
  let {
    node_id,
    comments = $bindable([]),
    onAttachmentsChanged,
  }: {
    node_id: number;
    comments?: Comment[];
    /** An image was pasted into a comment and now belongs to this node — the
     *  parent's attachment list is stale. Optional: only surfaces that show
     *  one care. */
    onAttachmentsChanged?: () => void;
  } = $props();

  // A comment is not a node (0001/0007), so an image pasted into one is owned
  // by the node the comment hangs off — handoff I3. That is also why one
  // upload ledger serves the whole thread: every image in it lands on the same
  // owner, and the attachment list is per node.
  const uploads = new PasteUploads();
  let lightbox = $state<string | null>(null);

  // Posting or editing a comment is the save step for anything pasted into it.
  async function claimUploads() {
    if (uploads.pending.length === 0) return;
    await uploads.linkAll(node_id);
    onAttachmentsChanged?.();
  }

  let newComment = $state("");
  let loadError = $state<unknown>(null);
  let loading = $state(true);

  // The comment the URL fragment points at (#1467). Held as state rather than
  // read off `location.hash` at render time so the highlight survives the list
  // re-rendering, and cleared after a few seconds: it is a "here it is" flash,
  // not a selection the reader then has to dismiss.
  let anchored = $state<number | null>(null);

  // Scrolling has to wait for the fetch. The browser applies a fragment when
  // the document loads, and at that moment this list is the single "Loading…"
  // row — so the element the URL names does not exist yet and the native jump
  // silently does nothing. This runs after the comments land instead.
  function focusAnchor() {
    const match = /^#comment-(\d+)$/.exec(window.location.hash);
    if (!match) return;
    const id = Number(match[1]);
    if (!comments.some((c) => c.id === id)) return;
    anchored = id;
    requestAnimationFrame(() => {
      document
        .getElementById(`comment-${id}`)
        ?.scrollIntoView({ block: "center" });
    });
    setTimeout(() => {
      if (anchored === id) anchored = null;
    }, 4000);
  }

  // Reload whenever the target node changes (component is reused across items).
  // This used to end in `.catch(() => {})`, so a failed fetch rendered the same
  // "No comments." as a node that genuinely has none (WI #547).
  function load(id: number) {
    loading = true;
    loadError = null;
    api
      .nodeComments(id)
      .then((c) => {
        if (id !== node_id) return;
        comments = c;
        focusAnchor();
      })
      .catch((e) => {
        if (id === node_id) loadError = e;
      })
      .finally(() => {
        if (id === node_id) loading = false;
      });
  }

  $effect(() => {
    const id = node_id;
    comments = [];
    load(id);
  });

  async function addComment() {
    if (newComment.trim() === "") return;
    const body = newComment.trim();
    const c = await attempt(() => api.addComment(node_id, body), "Add comment");
    // Only touch local state on success — optimistically appending and leaving
    // it there after a failure is its own kind of lie.
    if (c) {
      comments = [...comments, c];
      newComment = "";
      await claimUploads();
    }
  }

  async function removeComment(id: number) {
    const r = await attempt(() => api.deleteComment(id), "Delete comment");
    if (r) comments = comments.filter((c) => c.id !== id);
  }

  // WI #232 — edit in place ("saved, then realized I should include a WI #").
  let editingId = $state<number | null>(null);
  let editBuf = $state("");

  function startEdit(c: Comment) {
    editingId = c.id;
    editBuf = c.body;
  }

  async function saveEdit() {
    if (editingId == null || editBuf.trim() === "") return;
    const id = editingId;
    const body = editBuf.trim();
    const updated = await attempt(
      () => api.updateComment(id, body),
      "Save comment",
    );
    // Stay in the editor on failure so the typing is not lost.
    if (updated) {
      comments = comments.map((c) => (c.id === updated.id ? updated : c));
      editingId = null;
      await claimUploads();
    }
  }
</script>

<div class="border-t border-[var(--color-border)] pt-2">
  <p class="mb-1 text-xs font-semibold text-[var(--color-muted)]">Comments</p>
  {#if loadError}
    <ErrorNotice
      error={loadError}
      what="comments"
      retry={() => load(node_id)}
    />
  {:else}
    <ul class="space-y-1" data-testid="comment-list">
      {#each comments as c (c.id)}
        <!-- `korg:1469#comment-777` is a locator korg already prints — from
             `search`, from the copy affordances, in agent prose. #1467 made it
             a URL, and this is the anchor it lands on. One convention for every
             kind, because every node page mounts this component. -->
        <li
          id={`comment-${c.id}`}
          class="flex scroll-mt-24 items-start gap-2 rounded px-2 py-1 text-sm {anchored ===
          c.id
            ? 'bg-[var(--color-accent-soft)]'
            : 'bg-[var(--color-bg)]'}"
        >
          {#if editingId === c.id}
            <label class="sr-only" for={`comment-edit-${c.id}`}>Edit comment</label>
            <textarea id={`comment-edit-${c.id}`} class="min-h-[3rem] flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" bind:value={editBuf} use:pasteImages={uploads} onkeydown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) saveEdit(); if (e.key === "Escape") editingId = null; }}></textarea>
            <button class="text-xs text-[var(--color-accent)] hover:underline" onclick={saveEdit}>Save</button>
            <button class="text-xs text-[var(--color-muted)] hover:underline" onclick={() => (editingId = null)}>Cancel</button>
          {:else}
            <CommentBody body={c.body} onOpen={(id) => (lightbox = id)} />
            <button class="text-xs text-[var(--color-muted)] hover:text-[var(--color-accent)]" aria-label="Edit comment" title="Edit" onclick={() => startEdit(c)}>✎</button>
            <!-- Deleting a comment cannot be undone from the UI, so it confirms
                 rather than offering an undo (WI #549). -->
            <ConfirmButton
              label="Delete comment"
              class="text-xs text-[var(--color-muted)] hover:text-red-400"
              armedClass="rounded bg-red-900 px-1 text-xs text-red-100"
              onconfirm={() => removeComment(c.id)}
            >
              ✕
            </ConfirmButton>
          {/if}
        </li>
      {:else}
        <li class="text-xs text-[var(--color-muted)]">
          {loading ? "Loading…" : "No comments."}
        </li>
      {/each}
    </ul>
  {/if}
  <div class="mt-2 flex gap-2">
    <label class="sr-only" for={`comment-new-${node_id}`}>Add a comment</label>
    <textarea id={`comment-new-${node_id}`} class="min-h-[3rem] flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="Add a comment… (Ctrl/⌘-Enter to post, Ctrl-V an image)" data-testid="comment-input" bind:value={newComment} use:pasteImages={uploads} onkeydown={(e) => (e.key === "Enter" && (e.ctrlKey || e.metaKey)) && addComment()}></textarea>
    <button class="self-start rounded bg-[var(--color-surface-hi)] px-3 py-1 text-sm hover:bg-[var(--color-accent-soft)]" onclick={addComment}>Add</button>
  </div>
</div>

<Lightbox imgId={lightbox} onClose={() => (lightbox = null)} />

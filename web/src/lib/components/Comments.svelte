<script lang="ts">
  import { api, type Comment } from "$lib/api";
  import { attempt } from "$lib/toast.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import ConfirmButton from "./ConfirmButton.svelte";

  // `comments` is bindable so a parent can read the loaded bodies (e.g. to
  // extract launch URLs) without owning the fetch/add/delete logic.
  let { node_id, comments = $bindable([]) }: { node_id: number; comments?: Comment[] } =
    $props();

  let newComment = $state("");
  let loadError = $state<unknown>(null);
  let loading = $state(true);

  // Reload whenever the target node changes (component is reused across items).
  // This used to end in `.catch(() => {})`, so a failed fetch rendered the same
  // "No comments." as a node that genuinely has none (WI #547).
  function load(id: number) {
    loading = true;
    loadError = null;
    api
      .nodeComments(id)
      .then((c) => {
        if (id === node_id) comments = c;
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
        <li class="flex items-start gap-2 rounded bg-[var(--color-bg)] px-2 py-1 text-sm">
          {#if editingId === c.id}
            <label class="sr-only" for={`comment-edit-${c.id}`}>Edit comment</label>
            <textarea id={`comment-edit-${c.id}`} class="min-h-[3rem] flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" bind:value={editBuf} onkeydown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) saveEdit(); if (e.key === "Escape") editingId = null; }}></textarea>
            <button class="text-xs text-[var(--color-accent)] hover:underline" onclick={saveEdit}>Save</button>
            <button class="text-xs text-[var(--color-muted)] hover:underline" onclick={() => (editingId = null)}>Cancel</button>
          {:else}
            <span class="flex-1 whitespace-pre-wrap">{c.body}</span>
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
    <textarea id={`comment-new-${node_id}`} class="min-h-[3rem] flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="Add a comment… (Ctrl/⌘-Enter to post)" data-testid="comment-input" bind:value={newComment} onkeydown={(e) => (e.key === "Enter" && (e.ctrlKey || e.metaKey)) && addComment()}></textarea>
    <button class="self-start rounded bg-[var(--color-surface-hi)] px-3 py-1 text-sm hover:bg-[var(--color-accent-soft)]" onclick={addComment}>Add</button>
  </div>
</div>

<script lang="ts">
  import { api, type Comment } from "$lib/api";

  // `comments` is bindable so a parent can read the loaded bodies (e.g. to
  // extract launch URLs) without owning the fetch/add/delete logic.
  let { node_id, comments = $bindable([]) }: { node_id: number; comments?: Comment[] } =
    $props();

  let newComment = $state("");

  // Reload whenever the target node changes (component is reused across items).
  $effect(() => {
    const id = node_id;
    comments = [];
    api
      .nodeComments(id)
      .then((c) => {
        if (id === node_id) comments = c;
      })
      .catch(() => {});
  });

  async function addComment() {
    if (newComment.trim() === "") return;
    const c = await api.addComment(node_id, newComment.trim());
    comments = [...comments, c];
    newComment = "";
  }

  async function removeComment(id: number) {
    await api.deleteComment(id);
    comments = comments.filter((c) => c.id !== id);
  }
</script>

<div class="border-t border-[var(--color-border)] pt-2">
  <p class="mb-1 text-xs font-semibold text-[var(--color-muted)]">Comments</p>
  <ul class="space-y-1" data-testid="comment-list">
    {#each comments as c (c.id)}
      <li class="flex items-start gap-2 rounded bg-[var(--color-bg)] px-2 py-1 text-sm">
        <span class="flex-1 whitespace-pre-wrap">{c.body}</span>
        <button class="text-xs text-[var(--color-muted)] hover:text-red-400" aria-label="Delete comment" onclick={() => removeComment(c.id)}>✕</button>
      </li>
    {:else}<li class="text-xs text-[var(--color-muted)]">No comments.</li>{/each}
  </ul>
  <div class="mt-2 flex gap-2">
    <textarea class="min-h-[3rem] flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-sm outline-none" placeholder="Add a comment… (Ctrl/⌘-Enter to post)" data-testid="comment-input" bind:value={newComment} onkeydown={(e) => (e.key === "Enter" && (e.ctrlKey || e.metaKey)) && addComment()}></textarea>
    <button class="self-start rounded bg-[var(--color-surface-hi)] px-3 py-1 text-sm hover:bg-[var(--color-accent-soft)]" onclick={addComment}>Add</button>
  </div>
</div>

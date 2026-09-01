<script lang="ts">
  import { api, type Comment } from "$lib/api";
  import { attempt } from "$lib/toast.svelte";
  import { PasteUploads, pasteImages } from "$lib/imagePaste.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import ConfirmButton from "./ConfirmButton.svelte";
  import MarkdownView from "./MarkdownView.svelte";

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
    <!-- #1651 — a thread used to read as one wall of prose. Two things caused
         it and both are fixed here. `space-y-1` was a 4px gutter, smaller than
         the line-height inside a comment, so the gap between two comments read
         as a paragraph break within one. And the row's own `bg-[var(--color-bg)]`
         was the *page* background: on the surfaces that matter most for reading
         a thread — the proposal detail page and every `NodeDetail` route, whose
         containers set no background — a comment was tinted exactly like the
         page behind it, so the block had no edge at all. It only ever showed up
         inside the two `bg-[var(--color-surface)]` hosts, and even there at a
         0.04 lightness delta.

         `--color-surface-hi` is Ken's own reference (the "tags" box on the
         create form, which is where he pointed): one step *lighter* than
         either host, so a comment is a card on every surface rather than a
         card on one of them. The border is the separator half of "I'm leaning
         toward both" — with multi-line markdown inside, the gutter alone
         cannot say where one comment stops. -->
    <ul class="space-y-2" data-testid="comment-list">
      {#each comments as c (c.id)}
        <!-- `korg:1469#comment-777` is a locator korg already prints — from
             `search`, from the copy affordances, in agent prose. #1467 made it
             a URL, and this is the anchor it lands on. One convention for every
             kind, because every node page mounts this component. -->
        <li
          id={`comment-${c.id}`}
          class="flex scroll-mt-24 items-start gap-2 rounded border border-[var(--color-border)] px-2 py-1 text-sm {anchored ===
          c.id
            ? 'bg-[var(--color-accent-soft)]'
            : 'bg-[var(--color-surface-hi)]'}"
        >
          {#if editingId === c.id}
            <label class="sr-only" for={`comment-edit-${c.id}`}>Edit comment</label>
            <!-- Inset, not raised: the row itself is now `--color-surface-hi`, so
                 the editor takes the darker tone to keep an edge against the
                 card it sits in. -->
            <textarea id={`comment-edit-${c.id}`} class="min-h-[3rem] flex-1 rounded bg-[var(--color-bg)] px-2 py-1 text-sm outline-none" bind:value={editBuf} use:pasteImages={uploads} onkeydown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) saveEdit(); if (e.key === "Escape") editingId = null; }}></textarea>
            <button class="text-xs text-[var(--color-accent)] hover:underline" onclick={saveEdit}>Save</button>
            <button class="text-xs text-[var(--color-muted)] hover:underline" onclick={() => (editingId = null)}>Cancel</button>
          {:else}
            <!-- #1625 reverses #1120/#1121's "comments render as literal text
                 and keep doing so". That rule was written to protect two things
                 and korg's own renderer config already protects both: `#582`
                 stays literal because an ATX heading needs whitespace after the
                 hashes (CommonMark, and marked's `(?=\s|$)` lookahead), and a
                 single newline stays a line break because `markdown.ts` sets
                 `breaks: true` — the same shape `whitespace-pre-wrap` gave.
                 What genuinely changes is small and wanted: `*` italicises,
                 `-`/`1.` start lists. `_` does not, so `snake_case` is safe.

                 `MarkdownView` already renders korg's `![img-<hex>]` tokens as
                 the same thumbnail buttons the old component hand-rolled, and
                 owns its own lightbox — which is why this component no longer
                 has one.

                 `min-w-0` is load-bearing: this is a flex row, and a prose
                 block without it refuses to shrink below its longest
                 unbreakable token, so one pasted URL would push the edit and
                 delete controls off the row. -->
            <MarkdownView
              src={c.body}
              class="prose prose-invert min-w-0 max-w-none flex-1 text-sm"
            />
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

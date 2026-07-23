<script lang="ts">
  // The single live region for transient messages (WI #547). Mounted once in
  // +layout.svelte.
  //
  // Two regions rather than one, because assertiveness is not a style choice:
  // an error interrupts what a screen reader is saying, a success waits its
  // turn. The review found role="alert" on 4 of 10 pages; having exactly one
  // element that needs to get this right is how it stays right.
  import { toasts, dismiss } from "$lib/toast.svelte";

  const errors = $derived(toasts.filter((t) => t.kind === "error"));
  const successes = $derived(toasts.filter((t) => t.kind === "success"));
</script>

<div
  class="pointer-events-none fixed inset-x-0 bottom-0 z-[60] flex flex-col items-center gap-2 p-4"
  data-testid="toaster"
>
  <!-- Errors: assertive. role="alert" implies aria-live="assertive". -->
  <div role="alert" class="contents">
    {#each errors as t (t.id)}
      <div
        class="pointer-events-auto flex w-full max-w-lg items-start gap-3 rounded border border-red-800 bg-red-950 px-3 py-2 text-sm text-red-200 shadow-lg"
        data-testid="toast-error"
      >
        <span class="flex-1">{t.text}</span>
        <button
          class="shrink-0 rounded px-1 text-red-300 hover:bg-red-900"
          onclick={() => dismiss(t.id)}>Dismiss</button
        >
      </div>
    {/each}
  </div>

  <!-- Successes: polite, and auto-dismissed by the store. `role="status"`
       implies aria-live="polite" and gives the region a role tests and screen
       readers can both address. -->
  <div role="status" class="contents">
    {#each successes as t (t.id)}
      <div
        class="pointer-events-auto flex w-full max-w-lg items-start gap-3 rounded border border-[var(--color-border)] bg-[var(--color-surface-hi)] px-3 py-2 text-sm shadow-lg"
        data-testid="toast-success"
      >
        <span class="flex-1">{t.text}</span>
        {#if t.undo}
          <button
            class="shrink-0 rounded px-2 font-medium text-[var(--color-accent)] hover:underline"
            onclick={() => {
              t.undo?.();
              dismiss(t.id);
            }}>Undo</button
          >
        {/if}
        <button
          class="shrink-0 rounded px-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
          aria-label="Dismiss"
          onclick={() => dismiss(t.id)}>✕</button
        >
      </div>
    {/each}
  </div>
</div>

<script lang="ts">
  // The one modal primitive (WI #548), built on the native `<dialog>` element.
  //
  // Why the platform rather than a hand-rolled trap: `showModal()` gives us,
  // from the browser and correctly,
  //
  //   * a focus trap — the dialog goes into the top layer and everything else
  //     becomes inert, so Tab cannot escape without us intercepting a single
  //     keystroke;
  //   * focus restore to the opener when it closes;
  //   * Escape, via the `cancel` event;
  //   * `aria-modal` semantics and removal of the rest of the page from the
  //     accessibility tree;
  //   * `::backdrop` for the scrim.
  //
  // The alternative is ~60 lines that enumerate focusable selectors and wrap
  // Tab at the ends, and every hand-written version gets something subtly
  // wrong — disabled elements, `<summary>`, shadow roots, elements made
  // inert by an ancestor. Before this, NodePreview had none of trap, restore
  // or Escape, and the cards modal had Escape only.
  //
  // `placement` reproduces the two looks korg already had: a right-hand
  // slide-over (NodePreview) and a centred panel (the cards editor).

  interface Props {
    open: boolean;
    onClose: () => void;
    /** Accessible name for the dialog. */
    title: string;
    /** Show the title visually, or keep it for assistive tech only. */
    titleHidden?: boolean;
    placement?: "side" | "center";
    children: import("svelte").Snippet;
    /** Optional extra controls rendered next to the close button. */
    actions?: import("svelte").Snippet;
    class?: string;
  }

  let {
    open,
    onClose,
    title,
    titleHidden = false,
    placement = "center",
    children,
    actions,
    class: cls = "",
  }: Props = $props();

  let el = $state<HTMLDialogElement | null>(null);
  const titleId = `dlg-${Math.random().toString(36).slice(2, 9)}`;

  // Who to give focus back to. `<dialog>` restores focus itself on `close()`,
  // but only if the element is still around to do it — and korg's callers wrap
  // this in `{#if open}`, so closing usually destroys the component in the same
  // tick. Remembering the opener here makes restore independent of how the
  // parent chooses to unmount.
  let opener: HTMLElement | null = null;

  function restoreFocus() {
    const target = opener;
    opener = null;
    // Only if focus is still somewhere inside (or nowhere) — if something else
    // has deliberately taken it since, stealing it back would be worse.
    const active = document.activeElement;
    const stranded =
      !active || active === document.body || el?.contains(active) === true;
    if (target?.isConnected && stranded) target.focus();
  }

  // Drive the element from the `open` prop. `showModal()` is what puts the
  // dialog in the top layer — the plain `open` attribute does NOT, and gives
  // none of the behaviour above, so it is deliberately never used here.
  $effect(() => {
    const d = el;
    if (!d) return;
    if (open && !d.open) {
      opener = document.activeElement as HTMLElement | null;
      d.showModal();
    } else if (!open && d.open) {
      d.close();
      restoreFocus();
    }
  });

  // Covers the common case: the parent drops the component rather than setting
  // `open` to false, so the effect above never runs its close branch.
  $effect(() => () => restoreFocus());
</script>

<dialog
  bind:this={el}
  aria-labelledby={titleId}
  data-testid="dialog"
  data-placement={placement}
  class={[
    "backdrop:bg-black/60 text-[var(--color-text)] outline-none",
    placement === "side"
      ? "ml-auto mr-0 h-full max-h-full w-full max-w-md border-l border-[var(--color-border)] bg-[var(--color-surface)] p-4"
      : "m-auto w-full max-w-lg rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl",
    cls,
  ].join(" ")}
  oncancel={(e) => {
    // Escape fires `cancel`; let the parent own `open` rather than letting the
    // element close itself behind the state's back.
    e.preventDefault();
    onClose();
  }}
  onclick={(e) => {
    // A click on the dialog's own box is the backdrop: the element fills the
    // viewport region, and its children stop propagation by being children.
    // Comparing against currentTarget is what distinguishes the two.
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div class="mb-3 flex items-center justify-between gap-2">
    <h2
      id={titleId}
      class={titleHidden ? "sr-only" : "text-lg font-semibold"}
    >
      {title}
    </h2>
    <div class="flex items-center gap-1">
      {#if actions}{@render actions()}{/if}
      <button
        class="rounded px-2 py-1 text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)]"
        aria-label="Close"
        data-testid="dialog-close"
        onclick={onClose}>✕</button
      >
    </div>
  </div>

  {@render children()}
</dialog>

<style>
  /* The side variant is a full-height sheet; `<dialog>`'s default centring
     margins would otherwise float it. */
  dialog[data-placement="side"] {
    max-height: 100dvh;
    height: 100dvh;
  }
  dialog {
    overflow: auto;
  }
</style>

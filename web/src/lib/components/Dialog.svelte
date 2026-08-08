<script module lang="ts">
  // How many modals are open, and the question pages actually ask: "is
  // something modal on top of me?".
  //
  // `showModal()` makes the rest of the page inert for *pointer and focus*, but
  // a `keydown` still bubbles to `window` — so a page-level Escape handler
  // fires alongside the dialog's own, and one keystroke closes two things. That
  // is how Escape out of the lightbox also closed the work item behind it
  // (#1121). Counting here rather than querying `dialog[open]` keeps the answer
  // to korg's own modals: a `<dialog>` opened non-modally by something else is
  // not on top of anything.
  let openModals = 0;

  /** True while any korg modal is open. Page-level key handlers return early on
   *  this so the topmost thing owns the key. */
  export function modalOpen(): boolean {
    return openModals > 0;
  }
</script>

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
  // slide-over (NodePreview) and a centred panel (the cards editor) — plus
  // `lightbox` (#1121), a transparent full-viewport box whose only content is
  // the image. A third placement rather than a second modal primitive: the
  // lightbox needs exactly the trap/restore/Escape/backdrop this element
  // already gives, and differs only in how big the box is.

  interface Props {
    open: boolean;
    onClose: () => void;
    /** Accessible name for the dialog. */
    title: string;
    /** Show the title visually, or keep it for assistive tech only. */
    titleHidden?: boolean;
    placement?: "side" | "center" | "lightbox";
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

  // Whether *this* dialog is currently counted in `openModals`, so the count
  // survives both close paths (the `open` prop going false, and the parent
  // simply unmounting the component) without ever double-counting.
  let counted = false;

  function markOpen() {
    if (!counted) {
      counted = true;
      openModals += 1;
    }
  }

  function markClosed() {
    if (counted) {
      counted = false;
      openModals -= 1;
    }
  }

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
      markOpen();
    } else if (!open && d.open) {
      d.close();
      markClosed();
      restoreFocus();
    }
  });

  // Covers the common case: the parent drops the component rather than setting
  // `open` to false, so the effect above never runs its close branch.
  $effect(() => () => {
    markClosed();
    restoreFocus();
  });
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
      : placement === "lightbox"
        ? "m-auto max-h-[96dvh] max-w-[96vw] border-0 bg-transparent p-0 shadow-none"
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
  <div
    class={[
      "flex items-center gap-2",
      placement === "lightbox" ? "mb-1 justify-end" : "mb-3 justify-between",
    ].join(" ")}
  >
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

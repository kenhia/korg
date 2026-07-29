<script lang="ts">
  // A "back out of here" control for pages you arrive at from somewhere
  // specific, rather than navigate to (sprint 029).
  //
  // History and Topics left the top nav — they are reached from Today — so they
  // needed a way back that is not the browser's back button. Escape does the
  // same thing, matching the dialogs and the handoff page: on this UI, Escape
  // consistently means "leave what I just opened".
  //
  // Navigates to a fixed destination rather than `history.back()` on purpose.
  // These pages have one conceptual parent, and going *back* would return you
  // to whatever you were doing before, which is a different promise than the
  // label makes.
  import { goto } from "$app/navigation";

  let {
    href,
    label = "← Back",
  }: { href: string; label?: string } = $props();

  function leave() {
    void goto(href);
  }

  function onKeyDown(e: KeyboardEvent) {
    // Not while typing — Escape belongs to the field then (clearing a search,
    // closing its own dropdown).
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (e.key === "Escape") leave();
  }
</script>

<svelte:window onkeydown={onKeyDown} />

<button
  class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface-hi)]"
  data-testid="back-to"
  title={`${label} (Esc)`}
  onclick={leave}>{label}</button
>

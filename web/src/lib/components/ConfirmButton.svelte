<script lang="ts">
  // A two-step button for irreversible actions (WI #549).
  //
  // Scope note, because it is a design decision and not an oversight: this is
  // for actions that CANNOT be undone through the UI — comment delete,
  // relationship remove. Reversible actions (archive) get an undo toast
  // instead. Putting a confirm on a reversible action trains people to click
  // through confirms, which is exactly how confirms stop working on the one
  // that mattered.
  //
  // Two-step rather than a modal dialog: these are inline row controls, and a
  // modal for "delete this one-line comment" is heavier than the decision. The
  // button arms on first press and acts on second; it disarms on blur, on
  // Escape, and after a timeout, so an armed button is never left lying around
  // for a later stray click to trigger.

  interface Props {
    /** Imperative description — "Delete comment". Becomes the accessible name. */
    label: string;
    onconfirm: () => void;
    /** Glyph or text shown when idle. */
    children?: import("svelte").Snippet;
    class?: string;
    armedClass?: string;
    disarmMs?: number;
  }

  let {
    label,
    onconfirm,
    children,
    class: cls = "",
    armedClass = "",
    disarmMs = 4000,
  }: Props = $props();

  let armed = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function disarm() {
    armed = false;
    clearTimeout(timer);
  }

  function click() {
    if (armed) {
      disarm();
      onconfirm();
      return;
    }
    armed = true;
    clearTimeout(timer);
    timer = setTimeout(disarm, disarmMs);
  }

  $effect(() => () => clearTimeout(timer));
</script>

<button
  class={armed ? armedClass || cls : cls}
  aria-label={armed ? `Confirm: ${label}` : label}
  title={armed ? `Confirm: ${label}` : label}
  data-armed={armed ? "true" : undefined}
  data-testid="confirm-button"
  onclick={click}
  onblur={disarm}
  onkeydown={(e) => {
    if (e.key === "Escape") disarm();
  }}
>
  {#if armed}
    Sure?
  {:else if children}
    {@render children()}
  {:else}
    {label}
  {/if}
</button>
{#if armed}
  <!-- The button's accessible name changed while it kept focus, which screen
       readers do not reliably re-announce. This says it once, politely. -->
  <span role="status" class="sr-only">Press again to confirm: {label}</span>
{/if}

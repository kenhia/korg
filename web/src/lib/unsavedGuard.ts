// The guard between typed-but-unsaved text and the click that throws it away
// (WI #2845).
//
// The report was a work item half-written in the create form, a moment of "let
// me just check something", and the text gone. korg had no guard of any kind:
// no router hook, no unload handler, and — the cheapest loss of the three —
// nothing on the Escape key that closes the form outright.
//
// Three exits, and no two of them share a mechanism:
//
//   * **Leaving the page.** SvelteKit client-side navigation — a nav link, a
//     `goto`, Back. `beforeNavigate` sees all of it.
//   * **Leaving the browser.** Tab close, reload, an external link. SvelteKit
//     surfaces this through the *same* hook as `type: "leave"`, so there is no
//     second listener here; `cancel()` on a leave is what asks the browser for
//     its native "Leave site?" prompt.
//   * **Neither.** korg's forms are destroyed by *state*: Escape, Cancel, ←
//     Back, picking another project, clicking another row. The router never
//     hears about any of them, so those call `okToDiscard()` themselves, one
//     line at the top of the handler that does the destroying.
//
// Why `window.confirm` and not korg's own `Dialog` primitive: `beforeNavigate`
// must decide synchronously whether to cancel, and a modal answers later. The
// alternative is to cancel every navigation, ask, and re-issue it on discard —
// which turns Back into a forward push and wraps a state machine around a
// yes/no question. The unload prompt is the browser's own whatever we do, so
// the native one is also the consistent one. If this ever becomes a styled
// dialog, `okToDiscard()` is the seam: the state-destroying callers can await
// it, and only the two router cases need the synchronous answer.
//
// What this is NOT: draft autosave. Nothing here stores korg's data anywhere —
// the registry holds predicates, not content — so the korg+ GP-1 side-store
// line is untouched.

import { beforeNavigate } from "$app/navigation";

/** Answers "is there unsaved work in me right now?" for one editing surface. */
type DirtyCheck = () => boolean;

// A set of predicates rather than a dirty *flag*: several surfaces can be open
// at once (a work item being edited with a comment half-typed beneath it), and
// each has to be able to answer for itself and then stop answering when it is
// unmounted.
const sources = new Set<DirtyCheck>();

/**
 * Declare that this component holds unsaved work when `isDirty()` says so.
 *
 * Returns the unregister, so a component registers for exactly as long as it
 * exists:
 *
 * ```ts
 * $effect(() => registerUnsaved(isDirty));
 * ```
 *
 * The effect body must not *read* reactive state — pass a closure that reads
 * it when called, so the registration happens once rather than churning on
 * every keystroke.
 */
export function registerUnsaved(isDirty: DirtyCheck): () => void {
  sources.add(isDirty);
  return () => {
    sources.delete(isDirty);
  };
}

/** True if any open editing surface holds work korg has not got yet. */
export function hasUnsaved(): boolean {
  for (const isDirty of sources) if (isDirty()) return true;
  return false;
}

// Phrased as a question about the work, not about the mechanism: the answer is
// being given by someone who has just clicked something and may not yet know
// they were about to lose anything.
const MESSAGE = "You have unsaved changes. Leave and discard them?";

/**
 * Ask before destroying an open editing surface — the state-driven exits
 * (Escape, Cancel, ← Back, changing selection) that the router never sees.
 *
 * Guards the *whole screen* by default, because that is what most of these
 * callers destroy: Escape on the work-items page takes the create form with
 * it, and ← Back takes the detail panel and the comment draft inside it. One
 * import, one line:
 *
 * ```ts
 * if (!okToDiscard()) return;
 * ```
 *
 * Pass `isDirty` where the caller destroys exactly one surface and knows it —
 * a form's own Cancel button. Without that, cancelling an untouched form while
 * a comment sits half-written below it would ask about the comment, which the
 * click was never going to discard. An alarm that fires for something the
 * action would not have harmed is the kind that gets clicked through.
 */
export function okToDiscard(isDirty: DirtyCheck = hasUnsaved): boolean {
  return !isDirty() || confirm(MESSAGE);
}

/**
 * Wire the router half of the guard. Called once, from the root layout's
 * component initialisation — `beforeNavigate` is a lifecycle function and must
 * run there, not inside an effect.
 */
export function installUnsavedGuard(): void {
  beforeNavigate((nav) => {
    if (!hasUnsaved()) return;
    if (nav.type === "leave") {
      // A full-page unload. We are inside `beforeunload`, where a `confirm()`
      // of our own is ignored by every browser; cancelling is how we ask for
      // theirs, and its wording is not ours to choose.
      nav.cancel();
      return;
    }
    if (!confirm(MESSAGE)) nav.cancel();
  });
}

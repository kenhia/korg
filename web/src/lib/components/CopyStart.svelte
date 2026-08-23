<script lang="ts">
  // "Start this sprint" — the `/start-sprint korg:<node_id>` prompt, on every
  // surface a proposal can be read from (WI #1550).
  //
  // It used to live inline on the Planning card and nowhere else, which is fine
  // until you arrive at a proposal from somewhere other than the queue: kfdc's
  // pane opens `/planning/<id>` directly, so deciding to start the thing you
  // were just reading meant backing out to Planning (`← Planning`, or Esc)
  // purely to reach a button. Two call sites is the point at which the button
  // stops being markup and becomes a component — and #1498's failure handling
  // below is the other half of the argument: it is worth writing once.
  import { nodePage } from "$lib/domain";
  import { reportError } from "$lib/toast.svelte";

  interface Props {
    nodeId: number;
    /** Show the wording beside the glyph — the detail page has the room. */
    label?: boolean;
    class?: string;
  }

  let {
    nodeId,
    label = false,
    class: klass = "rounded px-1.5 py-0.5 text-xs hover:bg-[var(--color-surface)]",
  }: Props = $props();

  let copied = $state(false);
  let clearing: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    const text = `/start-sprint korg:${nodeId}`;
    try {
      // navigator.clipboard requires a secure context (HTTPS or localhost);
      // korg is served over plain HTTP on the LAN, so it's often undefined.
      // That ABSENCE is what `legacyCopy` is for and it works — #1498 is about
      // the other failure, where the object is present and `writeText()`
      // rejects. The two branches are left as they were on purpose: the fix for
      // a rejection is a message, not a fallback (see `blocked`).
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(text);
      } else {
        legacyCopy(text);
      }
      copied = true;
      clearTimeout(clearing);
      clearing = setTimeout(() => (copied = false), 1500);
    } catch (err) {
      reportError(err, "Copy to clipboard", blocked());
    }
  }

  /**
   * What to say about a rejected `writeText()` — WI #1498.
   *
   * The message names the embedder's `allow` list, unconditionally, and korg+
   * GP-17 is why: a cross-origin frame holds `clipboard-write` only if its
   * embedder delegates it, so "copy does not work in the pane" is the `allow`
   * attribute every time. kfdc paid for that discovery once (kfdc #1497) and
   * korg-dash, kdeskdash and korg-vs are each queued up to pay for it again —
   * naming the attribute sends the next report where GP-17 says it belongs,
   * rather than back to first principles about whether embedding was the right
   * shape.
   *
   * **Unconditional on purpose.** Deciding whether korg is *actually* framed
   * means reading `window.top`, which no test can fake (the property is
   * unforgeable) and no spec can produce honestly without constructing a real
   * cross-origin embed that withheld the delegation — the same
   * verification-costs-more-than-the-fix trap #1498's comment 1061 walked
   * through for the rejected `execCommand` fallback. So the sentence is worded
   * to be true either way: it says what embedded korg needs, and asserts
   * nothing about where you are reading it.
   *
   * The escape hatch is worth having in both places anyway. Framed, it is the
   * way out; top-level, "Document is not focused" is the everyday rejection and
   * a fresh tab is a real answer to it.
   */
  function blocked(): { hint: string; action: { label: string; href: string } } {
    return {
      hint: 'Embedded korg holds clipboard-write only if the embedder delegates it — allow="clipboard-write" on the iframe.',
      // The proposal's own page, from the generated route table rather than a
      // hand-built path (GP-16), opened top-level so the copy works there.
      action: { label: "Open in korg ↗", href: nodePage("sprint_proposal", nodeId) ?? "/planning" },
    };
  }

  function legacyCopy(text: string) {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    if (!ok) throw new Error("Copy failed — clipboard access is unavailable in this context.");
  }
</script>

<button
  class={klass}
  title="Copy /start-sprint prompt"
  data-testid="proposal-copy-start"
  onclick={copy}
  >{copied ? "✓" : "⧉"}{#if label}<span class="ml-1">{copied ? "Copied" : "Copy /start-sprint"}</span
    >{/if}</button
>

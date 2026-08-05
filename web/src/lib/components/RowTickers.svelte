<script lang="ts">
  // Row tickers — "there is more on this item than its title" (Ken,
  // 2026-07-31), after living with the comment ticker on the Review page.
  //
  // One component rather than the same three spans inlined per list, because
  // that is exactly how two pages end up disagreeing about what a marker means.
  // It is also where the handoff ticker lands when the row shape can carry it:
  // `has_handoff` is not on WorkItemRow or WorkItemSummary today, so no list
  // read can answer it without a server change (see the WI).
  //
  // Every ticker is presence-only except comments, which carry a count — a
  // thread of one and a thread of twelve are different amounts of "go read
  // this", and the count is already on the row.
  //
  // Glyphs are chosen for font coverage, not just meaning: 🗒️ (U+1F5D2) was the
  // first pick for details and rendered as a missing-glyph box in Chromium here,
  // so it is 📝 (U+1F4DD), which is old enough to be everywhere.
  //
  // Sprint 042 landed the handoff ticker, once `has_handoff` was on both row
  // contracts. The glyph is 📄 (U+1F4C4), NOT the 🫱 the WI proposed: 🫱 is
  // Unicode 14 and was flagged as the least portable of the three, and korg
  // already renders a handoff as 📄 on the Planning card's related-edge chips.
  // Matching the page that already exists beats introducing a second symbol
  // for the same thing — and it sidesteps the font question entirely.
  //
  // `details` on the Review page is honest now too: `WorkItemSummary` carries
  // `has_details` rather than the page inferring it from a body it never
  // fetched, so a missing 📝 there means "no details", not "cannot tell".

  let {
    comments = 0,
    details = false,
    handoff = false,
  }: { comments?: number; details?: boolean; handoff?: boolean } = $props();
</script>

{#if comments > 0}<span
    class="ml-1.5 whitespace-nowrap text-xs text-amber-300"
    title={`${comments} comment${comments === 1 ? "" : "s"} — read before closing`}
    >💬{comments}</span
  >{/if}{#if details}<span
    class="ml-1.5 whitespace-nowrap text-xs"
    title="Has a details section"
    aria-label="Has a details section">📝</span
  >{/if}{#if handoff}<span
    class="ml-1.5 whitespace-nowrap text-xs"
    title="Has a handoff — durable context waiting; read it before acting"
    aria-label="Has a handoff">📄</span
  >{/if}

<script lang="ts">
  // The Commander's Call lane (#969): everything that moves only when Ken acts.
  //
  // One `GET /api/awaiting` renders it — the rows already carry a resolved
  // title, the node's own status and its project, so there is no follow-up read
  // per row. The clear button is `PUT /api/nodes/:id/awaiting {awaiting:false}`,
  // the same core call an agent makes when it retracts its own marker; the UI
  // does not get a second path that could drift from the one agents use.
  //
  // Deliberately silent when empty: an "awaiting you" panel that renders an
  // empty state every day is a panel Ken learns to skip, and this one needs to
  // be read on the days it *does* have rows.
  import { api, type AwaitingRow } from "$lib/api";
  import { attempt } from "$lib/toast.svelte";
  import { ID_CLASS, chip, nodeHref } from "$lib/domain";

  let rows = $state<AwaitingRow[]>([]);
  let clearing = $state<number | null>(null);
  // Which row's reply box is open, and what is in it. One at a time: the lane
  // is a queue you work down, not a form you fill in.
  let replyingTo = $state<number | null>(null);
  let draft = $state("");

  export async function load(): Promise<void> {
    const r = await attempt(() => api.awaiting(), "Loading what's awaiting you");
    if (r) rows = r;
  }

  function openReply(row: AwaitingRow) {
    replyingTo = replyingTo === row.node_id ? null : row.node_id;
    draft = "";
  }

  async function clear(row: AwaitingRow) {
    clearing = row.node_id;
    const done = await attempt(
      () => api.setAwaiting(row.node_id, false),
      `Clearing "${row.title}"`,
    );
    if (done) rows = rows.filter((r) => r.node_id !== row.node_id);
    clearing = null;
  }

  /**
   * Register a decision in one gesture (#981): comment on the node, then clear
   * the marker.
   *
   * The comment lands on the **node**, via the same `POST
   * /api/nodes/:id/comments` an agent reads back — the lane stays a flag, per
   * D-3/D-7, and never grows a store of its own. That ordering is deliberate:
   * if the comment fails the marker stays up, so the ask survives; if the
   * clear fails the decision is still recorded where agents look. The failure
   * mode to avoid is a cleared marker with the answer nowhere.
   */
  async function decide(row: AwaitingRow) {
    const body = draft.trim();
    if (body === "") return;
    clearing = row.node_id;
    const posted = await attempt(
      () => api.addComment(row.node_id, body),
      `Answering "${row.title}"`,
    );
    if (!posted) {
      clearing = null;
      return;
    }
    const cleared = await attempt(
      () => api.setAwaiting(row.node_id, false),
      `Clearing "${row.title}"`,
    );
    if (cleared) {
      rows = rows.filter((r) => r.node_id !== row.node_id);
      replyingTo = null;
      draft = "";
    }
    clearing = null;
  }

  // "waiting 9 days" is the whole reason the marker is a timestamp rather than
  // a flag, so it is the most prominent thing on the row after the title.
  function waitedFor(since: string | null): string {
    if (!since) return "";
    const days = Math.floor(
      (Date.now() - new Date(since).getTime()) / 86_400_000,
    );
    if (days < 1) return "today";
    if (days === 1) return "1 day";
    return `${days} days`;
  }

  // Every kind the lane renders is clickable (#981). This used to return null
  // for anything that was not a work item, a program or a proposal — leaving
  // Ken with an ask he could not follow, on the surface whose whole job is
  // routing him to the thing that needs him. `nodeHref` knows the two kinds
  // with a page of their own and the list page for the rest; a list page is a
  // real destination, which is the argument the `sprint_proposal` → /planning
  // case already made.
  function href(row: AwaitingRow): string | null {
    return nodeHref(row.kind, row.node_id, row.wi_number);
  }

  $effect(() => {
    load();
  });
</script>

{#if rows.length > 0}
  <section
    class="rounded border border-amber-700/60 bg-amber-950/20 p-3"
    data-testid="awaiting-lane"
  >
    <h2 class="mb-2 text-xs font-medium uppercase tracking-[0.18em] text-amber-400">
      Awaiting you · {rows.length}
    </h2>
    <ul class="space-y-1.5">
      {#each rows as row (row.node_id)}
        <li class="text-sm" data-testid={`awaiting-${row.node_id}`}>
          <div class="flex flex-wrap items-baseline gap-2">
            <span class="text-xs tabular-nums text-amber-500/90"
              >{waitedFor(row.awaiting_since)}</span
            >
            <!-- #980 — the ask names a node, and the node is what Ken will
                 quote back at an agent. -->
            <span class={ID_CLASS}>#{row.wi_number ?? row.node_id}</span>
            {#if href(row)}
              <a class="font-medium hover:underline" href={href(row)}
                >{row.title}</a
              >
            {:else}
              <span class="font-medium">{row.title}</span>
            {/if}
            {#if row.awaiting_note}
              <span class="text-[var(--color-muted)]">— {row.awaiting_note}</span
              >
            {/if}
            {#if row.project}<span class={chip.project}>{row.project}</span>{/if}
            {#if row.status}<span class="text-xs text-[var(--color-muted)]"
                >{row.status}</span
              >{/if}
            <!-- Reply is the primary gesture now (#981): a judgment-shaped ask
                 used to cost three surfaces — follow the link, comment on the
                 node's page, come back, clear. `clear` stays for asks that need
                 no answer. -->
            <button
              class="ml-auto rounded px-2 py-0.5 text-xs text-amber-300 hover:bg-[var(--color-surface-hi)] disabled:opacity-50"
              aria-expanded={replyingTo === row.node_id}
              disabled={clearing === row.node_id}
              onclick={() => openReply(row)}
              title="Answer this — comments on the item, then clears the ask"
              >reply</button
            >
            <button
              class="rounded px-2 py-0.5 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)] disabled:opacity-50"
              disabled={clearing === row.node_id}
              onclick={() => clear(row)}
              title="Clear — you have dealt with this">clear</button
            >
          </div>
          {#if replyingTo === row.node_id}
            <div class="mt-1.5 flex flex-col gap-1.5">
              <label class="sr-only" for={`awaiting-reply-${row.node_id}`}
                >Your decision on {row.title}</label
              >
              <!-- The button that opened this box exists only to reach it, so
                   focus follows the click: landing anywhere else would put a
                   second tab stop inside a flow whose whole point is being one
                   gesture. It only ever appears in response to that click, not
                   on page load, which is the case the rule is guarding. -->
              <!-- svelte-ignore a11y_autofocus -->
              <textarea
                id={`awaiting-reply-${row.node_id}`}
                data-testid="awaiting-reply"
                class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none"
                rows="2"
                autofocus
                placeholder="Your decision — lands as a comment on the item, then clears the ask"
                bind:value={draft}
                onkeydown={(e) => {
                  // ⌘/Ctrl+Enter submits; plain Enter keeps making paragraphs,
                  // because a decision is often more than one line.
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault();
                    void decide(row);
                  }
                  if (e.key === "Escape") openReply(row);
                }}
              ></textarea>
              <div class="flex items-center gap-2">
                <button
                  class="rounded bg-[var(--color-accent-soft)] px-2 py-0.5 text-xs hover:bg-[var(--color-accent)] disabled:opacity-50"
                  data-testid="awaiting-reply-submit"
                  disabled={clearing === row.node_id || draft.trim() === ""}
                  onclick={() => decide(row)}>Answer &amp; clear</button
                >
                <button
                  class="rounded px-2 py-0.5 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface-hi)]"
                  onclick={() => openReply(row)}>Cancel</button
                >
                <!-- D-7 already clears the marker on a Ken-only transition, so
                     a status-shaped decision needs nothing from this box. Say
                     so rather than letting Ken type "closing this" into it. -->
                <span class="text-[10px] text-[var(--color-muted)]"
                  >⌘↵ to send · closing or finishing the item clears this ask on
                  its own</span
                >
              </div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

<script lang="ts">
  // WI #570 — the close-out page.
  //
  // The ritual it replaces: set the status filter to done+resolved, turn on
  // Quick Edit, scan titles, set each row's status dropdown to `closed`. Three
  // holes, and two of them are correctness rather than ergonomics:
  //
  //   1. A mis-set filter lets you close an item that was still open.
  //   2. A dropdown mis-click lands an item in a state nobody chose.
  //   3. A title-only scan misses what lives in `details` or in a comment.
  //
  // So this page cannot show an open item (its rows come from a status-filtered
  // server read), its only mutation is `-> closed` (a button, not a dropdown),
  // and selecting a row shows content, details AND comments together.
  //
  // Deliberately off the top nav: it is somewhere you go from Work Items when
  // the work is finished, not a place you start.
  import { tick } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import {
    api,
    type ProjectRow,
    type WorkItemDetail,
    type WorkItemSummary,
  } from "$lib/api";
  import { IN_PROPOSAL_COLOR, chip, projectRailColor } from "$lib/domain";
  import { renderMarkdown } from "$lib/markdown";
  import BackTo from "$lib/components/BackTo.svelte";
  import Comments from "$lib/components/Comments.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import RowTickers from "$lib/components/RowTickers.svelte";
  import { attempt, notify } from "$lib/toast.svelte";

  // The statuses this page reviews. `closed` is the destination, not a source;
  // `open` is the thing the old filter-based ritual could show by accident.
  const REVIEWABLE = ["done", "resolved"] as const;

  // The project rides in the URL rather than in localStorage, so the address
  // bar says what is on screen and a stale sticky value cannot mislead. Absent
  // = every project.
  const project = $derived($page.url.searchParams.get("project"));

  let projects = $state<ProjectRow[]>([]);
  let rows = $state<WorkItemSummary[]>([]);
  let loading = $state(true);
  let loadError = $state<unknown>(null);

  // What the server says matches, per status. Only interesting when it exceeds
  // what we hold — see `truncated`.
  let totals = $state<Record<string, number>>({});

  let selected = $state<number | null>(null);
  let detail = $state<WorkItemDetail | null>(null);
  let detailLoading = $state(false);
  let detailError = $state<unknown>(null);
  // Closing is one request; the button disables itself so a double-click cannot
  // send two.
  let closing = $state<Set<number>>(new Set());

  const railColor = $derived.by(() => {
    const byName = new Map(projects.map((p) => [p.name, projectRailColor(p)]));
    return (name: string | null) => (name ? byName.get(name) : undefined);
  });

  // A survey page is capped at LIST_LIMIT_MAX (500). Per status, that is a
  // ceiling this page will realistically never touch — but WI #762 is exactly
  // the bug of a list that truncates and says nothing, so it says something.
  const truncated = $derived(
    REVIEWABLE.some(
      (s) => (totals[s] ?? 0) > rows.filter((r) => r.wi_status === s).length,
    ),
  );

  async function load() {
    loading = true;
    loadError = null;
    try {
      const [ps, ...surveys] = await Promise.all([
        api.projects(),
        // One request per status: the server filter takes a single value, and
        // two round trips is the price of the list being *complete* for the
        // statuses it claims to show.
        ...REVIEWABLE.map((wi_status) =>
          api.surveyWorkItems({
            project: project ?? undefined,
            wi_status,
            archived: false,
          }),
        ),
      ]);
      projects = ps;
      totals = Object.fromEntries(
        REVIEWABLE.map((s, i) => [s, surveys[i].total]),
      );
      // Newest first: the sprint that just finished is the one being closed
      // out, and it sits at the top rather than at the bottom of five years of
      // history.
      rows = surveys.flatMap((s) => s.items).sort((a, b) => b.wi_number - a.wi_number);
      selected = null;
      detail = null;
    } catch (e) {
      loadError = e;
    } finally {
      loading = false;
    }
  }

  async function select(wi: number) {
    selected = wi;
    detail = null;
    detailError = null;
    detailLoading = true;
    try {
      // The authoritative read — content, details, comments and edges in one
      // request. The list rows are the slim survey projection on purpose: they
      // carry no bodies, so nothing here can render a stale one.
      const full = await api.workItem(wi);
      if (selected !== wi) return; // a faster click won
      if (full) detail = full;
      else detailError = new Error(`Work item #${wi} is no longer there.`);
    } catch (e) {
      if (selected === wi) detailError = e;
    } finally {
      if (selected === wi) detailLoading = false;
    }
  }

  /** The only mutation on this page. Reversible, so it undoes rather than
   *  confirms (WI #549) — a confirm on every row would rebuild the friction
   *  the one-click button exists to remove. */
  async function close(row: WorkItemSummary) {
    const wi = row.wi_number;
    const was = row.wi_status;
    closing = new Set(closing).add(wi);
    const r = await attempt(
      () => api.updateWorkItem(wi, { wi_status: "closed" }),
      "Close work item",
    );
    closing = new Set([...closing].filter((n) => n !== wi));
    if (!r) return;

    // Advance to the next row before dropping this one, so closing a run of
    // items keeps the details pane moving down the list instead of emptying.
    const idx = rows.findIndex((x) => x.wi_number === wi);
    const next = rows[idx + 1] ?? rows[idx - 1] ?? null;
    rows = rows.filter((x) => x.wi_number !== wi);
    if (totals[was] != null) totals[was] -= 1;
    if (selected === wi) {
      if (next) await select(next.wi_number);
      else {
        selected = null;
        detail = null;
      }
    }

    notify(`#${wi} closed.`, async () => {
      const undone = await attempt(
        () => api.updateWorkItem(wi, { wi_status: was }),
        "Undo",
      );
      if (!undone) return;
      await load();
      await select(wi);
      await tick();
      document.getElementById(`review-row-${wi}`)?.scrollIntoView({ block: "center" });
    });
  }

  function move(delta: number) {
    if (rows.length === 0) return;
    const idx = rows.findIndex((r) => r.wi_number === selected);
    const nextIdx = Math.min(
      rows.length - 1,
      Math.max(0, (idx < 0 ? 0 : idx) + delta),
    );
    void select(rows[nextIdx].wi_number);
  }

  // Arrows only. There is deliberately no keyboard shortcut for Close: this
  // page exists because a stray input put items in the wrong state, and a
  // hotkey next to the navigation keys would be a new way to do exactly that.
  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    }
  }

  function switchProject(name: string) {
    void goto(
      name === "" ? "/work-items/review" : `/work-items/review?project=${encodeURIComponent(name)}`,
    );
  }

  // Re-runs on every project change, not just on mount: switching projects is a
  // client-side navigation, which does not remount the page component.
  $effect(() => {
    void project;
    void load();
  });
</script>

<svelte:window onkeydown={onKey} />

<section class="space-y-4">
  <div class="flex flex-wrap items-center gap-2">
    <BackTo href="/work-items" label="← Work Items" />
    <h1 class="text-xl font-semibold">Review completed</h1>
    <label class="sr-only" for="review-project">Project</label>
    <select
      id="review-project"
      class="ml-auto max-w-56 truncate rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-xs"
      data-testid="review-project"
      style={railColor(project) ? `color: ${railColor(project)}` : ""}
      value={project ?? ""}
      onchange={(e) => switchProject(e.currentTarget.value)}
    >
      <option value="">All projects</option>
      {#each projects as p (p.id)}
        <option value={p.name} style={projectRailColor(p) ? `color: ${projectRailColor(p)}` : ""}
          >{p.name}</option
        >
      {/each}
    </select>
  </div>

  <p class="text-xs text-[var(--color-muted)]">
    Only <span class="text-[var(--color-text)]">done</span> and
    <span class="text-[var(--color-text)]">resolved</span> items — an open item cannot
    appear here. Closing is one click and undoable. ↑/↓ to move.
  </p>

  {#if loadError}
    <ErrorNotice error={loadError} what="completed work items" retry={load} />
  {:else if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else if rows.length === 0}
    <p
      class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-6 text-center text-sm text-[var(--color-muted)]"
      data-testid="review-empty"
    >
      Nothing to review{project ? ` in ${project}` : ""} — no done or resolved items.
    </p>
  {:else}
    <!-- The one interactive-tuning decision in this sprint (WI #570): the
         details pane sits BELOW the list until there is room for it BESIDE the
         list, at which point reviewing stops being a scroll.
         A container query, not a media query — this route renders inside the
         layout's `roomy` 80%-of-viewport main, so the viewport width is not the
         width that matters. The two knobs are both in the class list below:
         `@[86rem]` is where it flips, and `minmax(26rem,38rem)` is how much the
         details column may take. Tuned on Ken's wide monitor, 2026-07-31. -->
    <div class="@container">
      <div
        class="grid grid-cols-1 gap-4 @[86rem]:grid-cols-[minmax(0,1fr)_minmax(26rem,38rem)] @[86rem]:items-start"
        data-testid="review-layout"
      >
        <div class="min-w-0 space-y-2">
          <p class="text-xs text-[var(--color-muted)]">
            {rows.length} to review{#if truncated}<span class="ml-1 text-amber-300"
                >· showing {rows.length} of {(totals.done ?? 0) + (totals.resolved ?? 0)}</span
              >{/if}
          </p>
          <!-- Beside the details pane, the list scrolls in its own box: that is
               what keeps the sticky header row on screen (it sticks to its
               scroll container, and while the page is the scroller the header
               leaves with everything else). Stacked, the page scrolls as one. -->
          <div
            class="overflow-auto rounded border border-[var(--color-border)] @[86rem]:max-h-[calc(100vh-9rem)]"
          >
            <table class="w-full text-sm">
              <thead
                class="sticky top-0 bg-[var(--color-surface)] text-left text-xs text-[var(--color-muted)]"
              >
                <tr>
                  <th class="px-3 py-2">ID</th>
                  {#if !project}<th class="px-3 py-2">Project</th>{/if}
                  <th class="px-3 py-2">Status</th>
                  <th class="px-3 py-2">Title</th>
                  <th class="px-3 py-2 text-right">Close</th>
                </tr>
              </thead>
              <tbody>
                {#each rows as row (row.wi_number)}
                  <tr
                    id={`review-row-${row.wi_number}`}
                    class="cursor-pointer border-t border-[var(--color-border)] hover:bg-[var(--color-surface-hi)]"
                    class:bg-[var(--color-surface-hi)]={row.wi_number === selected}
                    onclick={() => select(row.wi_number)}
                  >
                    <!-- The three scanning columns never wrap: they are what
                         the eye runs down, and a two-line project name puts the
                         next row's id somewhere new. Only the title wraps. -->
                    <td class="whitespace-nowrap px-3 py-1.5 font-mono text-xs text-[var(--color-muted)]"
                      >{row.wi_number}</td
                    >
                    {#if !project}
                      <td
                        class="whitespace-nowrap px-3 py-1.5 text-xs"
                        style={railColor(row.project) ? `color: ${railColor(row.project)}` : ""}
                        >{row.project ?? "—"}</td
                      >
                    {/if}
                    <td class="whitespace-nowrap px-3 py-1.5">
                      <span class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs"
                        >{row.wi_status}</span
                      >
                    </td>
                    <td class="px-3 py-1.5">
                      <button
                        class="w-full cursor-pointer text-left"
                        style={row.proposal_node_id != null
                          ? `color: ${IN_PROPOSAL_COLOR}`
                          : ""}
                        onclick={(e) => {
                          e.stopPropagation();
                          select(row.wi_number);
                        }}
                        ><!-- The details ticker is honest here as of sprint 042:
                             `WorkItemSummary` carries `has_details`, so a
                             missing 📝 means "no details" rather than "this
                             projection cannot tell you". Same for the handoff
                             marker — which matters most on *this* page, since
                             closing an item whose handoff nobody read is
                             exactly the mistake it prevents. -->
                        {row.title}<RowTickers
                          comments={row.comment_count}
                          details={row.has_details}
                          handoff={row.has_handoff}
                        /></button
                      >
                    </td>
                    <!-- The button this page is for. One click, one destination,
                         no dropdown to mis-hit. -->
                    <td class="px-3 py-1.5 text-right">
                      <button
                        class="rounded bg-[var(--color-accent-soft)] px-2 py-0.5 text-xs hover:bg-[var(--color-accent)] disabled:opacity-40"
                        data-testid={`review-close-${row.wi_number}`}
                        disabled={closing.has(row.wi_number)}
                        title={`Close #${row.wi_number}`}
                        onclick={(e) => {
                          e.stopPropagation();
                          close(row);
                        }}>Close</button
                      >
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>

        <!-- Sticky only once it is beside the list; stacked below, it is just
             the next thing on the page. -->
        <div
          class="min-w-0 @[86rem]:sticky @[86rem]:top-16 @[86rem]:max-h-[calc(100vh-6rem)] @[86rem]:overflow-auto"
          data-testid="review-detail"
        >
          {#if detailError}
            <ErrorNotice
              error={detailError}
              what="the work item"
              retry={() => selected != null && select(selected)}
            />
          {:else if detail}
            {@render detailView(detail)}
          {:else}
            <p
              class="rounded border border-dashed border-[var(--color-border)] px-3 py-6 text-center text-sm text-[var(--color-muted)]"
            >
              {detailLoading ? "Loading…" : "Select a row to see everything on it."}
            </p>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</section>

{#snippet detailView(item: WorkItemDetail)}
  <article
    class="space-y-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4"
  >
    <div class="flex items-start justify-between gap-3">
      <h2 class="min-w-0 flex-1 text-lg font-semibold">
        <span class="font-mono text-sm text-[var(--color-muted)]">#{item.wi_number}</span>
        {item.title}
      </h2>
      <button
        class="shrink-0 rounded bg-[var(--color-accent-soft)] px-3 py-1 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40"
        data-testid="review-detail-close"
        disabled={closing.has(item.wi_number)}
        onclick={() => {
          const row = rows.find((r) => r.wi_number === item.wi_number);
          if (row) close(row);
        }}>Close</button
      >
    </div>

    <dl class="flex flex-wrap gap-x-5 gap-y-1 text-xs">
      <div class="flex gap-1">
        <dt class="text-[var(--color-muted)]">Project</dt>
        <dd style={railColor(item.project) ? `color: ${railColor(item.project)}` : ""}
          >{item.project ?? "—"}</dd
        >
      </div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Area</dt><dd>{item.area ?? "—"}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Type</dt><dd>{item.wi_type}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Status</dt><dd>{item.wi_status}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Size</dt><dd>{item.wi_tshirt}</dd></div>
      <div class="flex gap-1"><dt class="text-[var(--color-muted)]">Sprint</dt><dd>{item.sprint ?? "—"}</dd></div>
      <div class="flex gap-1">
        <dt class="text-[var(--color-muted)]">Updated</dt>
        <dd>{new Date(item.updated).toLocaleDateString()}</dd>
      </div>
    </dl>

    {#if item.tags.length > 0}
      <div class="flex flex-wrap gap-1">
        {#each item.tags as tag (tag)}<span class={chip.tag}>#{tag}</span>{/each}
      </div>
    {/if}

    <section>
      <h3 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">Content</h3>
      <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
      <div class="prose prose-invert max-w-none text-sm">{@html renderMarkdown(item.content)}</div>
    </section>

    <!-- Hole 3: `details` and the comments are the two places a reason to NOT
         close lives, and the old title-scan showed neither. Both render here,
         unprompted, for every selected row. -->
    {#if item.details}
      <section>
        <h3 class="mb-1 border-b border-[var(--color-border)] pb-1 text-sm font-semibold">Details</h3>
        <div
          class="prose prose-invert max-w-none rounded p-2 text-sm"
          style="background: color-mix(in oklch, var(--color-surface) 75%, var(--color-accent) 25%)"
        >
          <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitized markdown -->
          {@html renderMarkdown(item.details)}
        </div>
      </section>
    {/if}

    <Comments node_id={item.node_id} />
  </article>
{/snippet}

<script lang="ts">
  import { onMount } from "svelte";
  import { dndzone, type DndEvent } from "svelte-dnd-action";
  import {
    api,
    type PlanningRollupRow,
    type ProjectRow,
    type ProposalRow,
    type RelatedRef,
  } from "$lib/api";
  import type { ProposalStatus } from "$lib/generated/vocab";
  import {
    CATEGORY_ORDER,
    IN_PROPOSAL_COLOR,
    PARKED,
    chip,
    isParked,
    midRank,
    projectRailColor,
    stampDay,
  } from "$lib/domain";
  import NodePreview from "$lib/components/NodePreview.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import { attempt, reportError } from "$lib/toast.svelte";
  import { ALL_PROJECTS, readProjectScope, writeProjectScope } from "$lib/projectScope";

  type DndItem = { id: number; proposal: ProposalRow };
  type Covered = { wi_number: number; title: string };

  // WI #565 — the queue spans repos, so scope it to the project you're in.
  // Sticky like the work-items rail — and since WI #1310 it is not merely the
  // same convention but the same selection: `$lib/projectScope` owns the key
  // and the sentinel, so picking a project here scopes Work Items too.
  // WI #823 — grouping mode is a view preference and sticks for the same
  // reason the project does. Separate key from the Work Items rail: the two
  // rails answer different questions and Ken may well want them set
  // differently — which is exactly why #1310 moved the project and left this.
  const GROUP_KEY = "korg.planning.groupByCategory";
  function readSticky(key: string): string | null {
    try {
      return typeof localStorage !== "undefined" ? localStorage.getItem(key) : null;
    } catch {
      return null;
    }
  }
  function writeSticky(key: string, value: string): void {
    try {
      if (typeof localStorage !== "undefined") localStorage.setItem(key, value);
    } catch {
      /* storage unavailable — non-fatal */
    }
  }
  let projectFilter = $state(ALL_PROJECTS);
  let proposalsRaw = $state<ProposalRow[]>([]);
  let covers = $state<Record<number, Covered[]>>({});
  // Non-`covers` edges from the LB-3 block (WI #611) — `has_handoff` especially,
  // so a proposal's handoffs are reachable from its Planning card.
  let relatedByProposal = $state<Record<number, RelatedRef[]>>({});
  let loading = $state(true);
  let loadError = $state<unknown>(null);
  let copiedId = $state<number | null>(null);
  // Covered items are work items, whose node id equals their wi_number — pass
  // it straight to the shared preview panel.
  let previewNode = $state<number | null>(null);
  const flip = 150;

  let pinnedBuf = $state<DndItem[]>([]);
  let queueBuf = $state<DndItem[]>([]);

  // `proposalsRaw` holds every proposal; `scoped` is what the current project
  // filter shows. They used to be the same thing, fetched pre-filtered — which
  // is what broke the dropdown (see `load`).
  const scoped = $derived(
    projectFilter === ALL_PROJECTS
      ? proposalsRaw
      : proposalsRaw.filter((p) => p.project === projectFilter),
  );

  const active = $derived(
    scoped.filter((p) => p.status === "active").sort((a, b) => Number(a.rank) - Number(b.rank)),
  );
  const done = $derived(scoped.filter((p) => p.status === "done"));
  const declined = $derived(scoped.filter((p) => p.status === "declined"));
  let showDone = $state(false);
  let showDeclined = $state(false);

  // Parked (#1534) is NOT a third collapsible beside these two, and that is the
  // whole point of the status. `done` and `declined` are hidden because they are
  // over; a parked proposal is live work you asked to keep in view, so it renders
  // below a labelled rule the way #810's parked work items do — de-prioritised,
  // never folded away. Rank-ordered within the section, mirroring the queue.
  const parked = $derived(
    scoped.filter((p) => isParked(p.status)).sort((a, b) => Number(a.rank) - Number(b.rank)),
  );

  // WI #823 — the rail replaced the dropdown, so its source changed with it.
  //
  // The dropdown's options were "every project the queue mentions", which is
  // the wrong set for a rail: a project with no proposals is exactly the one
  // you want to click to see it has none. So the rail lists *projects*, from
  // the project roster, and the rollup supplies each entry's counts.
  let projects = $state<ProjectRow[]>([]);
  let rollup = $state<Record<string, PlanningRollupRow>>({});
  let showAllProjects = $state(false);
  // Unset means grouped, matching the Work Items rail — only an explicit
  // opt-out is remembered.
  let groupByCategory = $state(readSticky(GROUP_KEY) !== "0");

  const visibleProjects = $derived(
    projects.filter((p) => showAllProjects || p.status === "active"),
  );

  // Same construction as the Work Items rail (WI #678): CATEGORY_ORDER for a
  // predictable position, and an "Uncategorised" bucket so nothing the
  // vocabulary didn't claim falls out of the rail.
  const projectGroups = $derived.by(() => {
    const groups: { key: string; label: string; projects: ProjectRow[] }[] = [];
    const placed = new Set<number>();
    for (const c of CATEGORY_ORDER) {
      const ps = visibleProjects.filter((p) => p.category === c);
      if (ps.length === 0) continue;
      ps.forEach((p) => placed.add(p.id));
      groups.push({ key: c, label: c, projects: ps });
    }
    const rest = visibleProjects.filter((p) => !placed.has(p.id));
    if (rest.length > 0) {
      // The key is the ESCAPE `\u0000none`, not a raw NUL byte, which is what
      // this line held until #1310 went looking through it: one NUL makes the
      // whole file binary to git (`Bin 26176 bytes` instead of a reviewable
      // diff) and invisible to `grep -rn` across the repo. Same value at
      // runtime, same spelling as the Work Items rail.
      groups.push({ key: "\u0000none", label: "Uncategorised", projects: rest });
    }
    return groups;
  });

  // The "All Projects" row's own counts: the totals across everything the rail
  // is showing, so the top entry is a summary rather than a blank.
  const allCounts = $derived.by(() => {
    const rows = visibleProjects
      .map((p) => rollup[p.name])
      .filter((r): r is PlanningRollupRow => !!r);
    return {
      proposals: rows.reduce((n, r) => n + r.proposals, 0),
      wi_in_proposal: rows.reduce((n, r) => n + r.wi_in_proposal, 0),
      wi_total: rows.reduce((n, r) => n + r.wi_total, 0),
    };
  });

  // Picking a project no longer refetches. `load` was already fetching the
  // whole queue unfiltered (WI #622) and scoping in the client — `scoped` does
  // the work, so a re-read here only bought a spinner. Covers for the newly
  // visible cards are the one thing still missing, so fetch just those.
  //
  // `rebuild()` is NOT optional here, and dropping it is how this first
  // shipped broken: `active`/`done`/`declined` are `$derived` off `scoped` and
  // re-filter themselves, but the two drag zones are plain state that
  // `rebuild` populates. Without it the pinned and queue columns keep showing
  // the previous project's proposals while the rest of the page has moved on.
  async function pickProject(name: string) {
    projectFilter = name;
    writeProjectScope(name);
    rebuild();
    await loadMissingCovers();
  }

  function rebuild() {
    const proposed = scoped.filter((p) => p.status === "proposed");
    pinnedBuf = proposed
      .filter((p) => p.pinned)
      .sort((a, b) => Number(a.rank) - Number(b.rank))
      .map((p) => ({ id: p.node_id, proposal: p }));
    queueBuf = proposed
      .filter((p) => !p.pinned)
      .sort((a, b) => Number(a.rank) - Number(b.rank))
      .map((p) => ({ id: p.node_id, proposal: p }));
  }

  // One authoritative read per proposal (WI #536) — no neighbors call, no
  // client-side join against every work item in the instance.
  async function loadCovers(proposalNodeId: number) {
    // Best-effort: a proposal whose covers fail to load still renders, and the
    // page-level load already reports a broader failure.
    const detail = await attempt(
      () => api.proposal(proposalNodeId),
      "Load covered work items",
    );
    covers[proposalNodeId] = (detail?.covered ?? []).map((c) => ({
      wi_number: c.wi_number,
      title: c.title,
    }));
    // get_proposal.related already excludes `covers` (those are `covered`), so
    // this is has_handoff / depends_on / related-to.
    relatedByProposal[proposalNodeId] = detail?.related ?? [];
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      // Fetch UNFILTERED and scope in the client (WI #622).
      //
      // This used to ask the server for just the current project's proposals
      // and rebuild the dropdown's options from the result — but a filtered
      // fetch cannot contain the projects the filter excluded, so the option
      // list was only ever correct on an unfiltered load. Return to Planning
      // with a sticky project set and `onMount` restores it *before* the first
      // load, so the rebuild was skipped entirely and the dropdown offered
      // nothing but "All projects": no way back out without clearing it.
      //
      // The whole queue is 87 rows, so one unfiltered read is cheaper than the
      // bug. Covers are still fetched only for what is on screen.
      //
      // WI #823 — the rail and its counts join the same parallel fetch. Three
      // requests, not one-per-project: `planningRollup` is a single statement
      // server-side for exactly that reason.
      const [queue, roster, counts] = await Promise.all([
        api.proposals(),
        api.projects(),
        api.planningRollup(),
      ]);
      proposalsRaw = queue;
      projects = roster;
      rollup = Object.fromEntries(counts.map((r) => [r.project, r]));
      rebuild();
      await loadMissingCovers();
    } catch (e) {
      loadError = e;
    } finally {
      loading = false;
    }
  }

  // Covers for whatever is on screen and hasn't been fetched yet. Only
  // proposals that actually cover something need a detail read; `covered_count`
  // on the row answers the rest.
  async function loadMissingCovers() {
    await Promise.all(
      scoped
        .filter((p) => p.covered_count > 0 && covers[p.node_id] === undefined)
        .map((p) => loadCovers(p.node_id)),
    );
  }

  function considerPinned(e: CustomEvent<DndEvent<DndItem>>) {
    pinnedBuf = e.detail.items;
  }
  function considerQueue(e: CustomEvent<DndEvent<DndItem>>) {
    queueBuf = e.detail.items;
  }
  async function finalizePinned(e: CustomEvent<DndEvent<DndItem>>) {
    pinnedBuf = e.detail.items;
    await settle(pinnedBuf, e.detail.info.id, true);
  }
  async function finalizeQueue(e: CustomEvent<DndEvent<DndItem>>) {
    queueBuf = e.detail.items;
    await settle(queueBuf, e.detail.info.id, false);
  }
  async function settle(items: DndItem[], movedId: string | number, pinned: boolean) {
    const idx = items.findIndex((it) => String(it.id) === String(movedId));
    if (idx < 0) return;
    const rank = midRank(items[idx - 1]?.proposal.rank, items[idx + 1]?.proposal.rank);
    const moved = items[idx].proposal;
    try {
      await api.updateProposal(moved.node_id, { rank, pinned });
      moved.rank = String(rank);
      moved.pinned = pinned;
    } catch (err) {
      // Resynchronise: the list is showing an order the server rejected.
      reportError(err, "Reorder proposal");
      await load();
    }
  }

  async function togglePin(p: ProposalRow) {
    const r = await attempt(
      () => api.updateProposal(p.node_id, { pinned: !p.pinned }),
      p.pinned ? "Unpin proposal" : "Pin proposal",
    );
    if (!r) return;
    p.pinned = !p.pinned;
    rebuild();
  }

  async function setStatus(p: ProposalRow, status: ProposalStatus) {
    const r = await attempt(
      () => api.updateProposal(p.node_id, { status }),
      "Set proposal status",
    );
    if (!r) return;
    p.status = status;
    proposalsRaw = [...proposalsRaw];
    rebuild();
  }

  async function copyStart(p: ProposalRow) {
    const text = `/start-sprint korg:${p.node_id}`;
    try {
      // navigator.clipboard requires a secure context (HTTPS or localhost);
      // korg is served over plain HTTP on the LAN, so it's often undefined.
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(text);
      } else {
        legacyCopy(text);
      }
      copiedId = p.node_id;
      setTimeout(() => (copiedId = null), 1500);
    } catch (err) {
      reportError(err, "Copy to clipboard");
    }
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

  function openPreview(wi_number: number) {
    previewNode = wi_number;
  }

  onMount(() => {
    projectFilter = readProjectScope() ?? ALL_PROJECTS;
    void load();
  });
</script>

{#snippet card(p: ProposalRow)}
  <!-- WI #671 — an active proposal gets a border the eye lands on. The hue is
       42, which is what Ken described as "the colour claude-cleo is in": that
       was a name-hash accident when he wrote it, and is now the Ops category
       colour from #678 — so it is stable rather than arbitrary, and reusing it
       here keeps warm = "this is the live one", as on Today's target day.
       The transparent border is always present so an active card does not
       shift its neighbours by 2px when its status changes. -->
  <div
    class="cursor-grab rounded border-2 border-transparent bg-[var(--color-surface-hi)] p-3 active:cursor-grabbing"
    style={p.status === "active" ? "border-color: hsl(42 55% 62%)" : ""}
    data-testid={`proposal-${p.node_id}`}
  >
    <div class="flex items-start justify-between gap-2">
      <!-- WI #870 — the proposal's own detail view. Every *other* node on this
           card could be previewed (covered items, handoffs, related edges) and
           the proposal could not: the one shape the page is actually about was
           the one with no way to open it. The title is the affordance because it
           is where you already point to mean "this one"; a button inside the
           draggable is fine, as the covered chips below have always been. -->
      <button
        class="text-left text-sm font-medium hover:text-[var(--color-accent)]"
        title="Open this proposal's detail"
        data-testid="proposal-open-detail"
        onclick={() => (previewNode = p.node_id)}
      >{p.title}</button>
      <div class="flex shrink-0 items-center gap-1">
        <!-- WI #1162 — "a control to the left of the pin and copy controls
             (magnifying glass) that would do the detail view", verbatim. The
             title keeps the pop-out: the two are different questions ("what is
             this?" vs "let me pour over this"), and Ken's ask is explicit that
             the quick look is worth keeping.
             An `<a>`, not a button with `goto`, so the ordinary link gestures
             work — a proposal is a thing you may well want in a second tab
             beside the repo. It lives inside the draggable card, which the
             covered chips and these two buttons have always done. -->
        <a
          class="rounded px-1.5 py-0.5 text-xs hover:bg-[var(--color-surface)]"
          href={`/planning/${p.node_id}`}
          title="Open the full detail view"
          data-testid="proposal-detail-link">🔍</a
        >
        <button
          class="rounded px-1.5 py-0.5 text-xs hover:bg-[var(--color-surface)]"
          class:text-[var(--color-accent)]={p.pinned}
          title={p.pinned ? "Unpin" : "Pin to top"}
          onclick={() => togglePin(p)}
        >📌</button>
        <button
          class="rounded px-1.5 py-0.5 text-xs hover:bg-[var(--color-surface)]"
          title="Copy /start-sprint prompt"
          onclick={() => copyStart(p)}
        >{copiedId === p.node_id ? "✓" : "⧉"}</button>
      </div>
    </div>
    <!-- WI #1106 — the row half. `tags` are on the node for every kind and were
         rendered on no proposal surface: proposal 1080 carries
         ["ui","audit","planning","vocabulary"] and you could not see that
         anywhere in the web app. They belong beside the project chip because
         they answer the same question it does — what is this about — and the
         shared `chip.tag` brings the #571 contrast fix with it. -->
    {#if p.project || p.tags.length > 0}
      <div class="mt-1 flex flex-wrap items-center gap-1">
        {#if p.project}
          <span class={chip.project} data-testid="proposal-project">{p.project}</span>
        {/if}
        {#each p.tags as t (t)}<span class={chip.tag}>#{t}</span>{/each}
      </div>
    {/if}
    <p class="mt-1 text-xs text-[var(--color-muted)]">{p.summary}</p>
    <!-- WI #860 — `summary` is a 500-char routing contract now and the analysis
         lives in `notes`. A card that showed only the contract would be hiding
         prose this page used to render in full, so the long form stays one
         click away rather than one fetch away: the row already carries it. -->
    {#if p.notes}
      <details class="mt-1">
        <summary class="cursor-pointer text-xs text-[var(--color-muted)] hover:text-[var(--color-accent)]"
          >Notes</summary
        >
        <p class="mt-1 whitespace-pre-wrap text-xs text-[var(--color-muted)]">{p.notes}</p>
      </details>
    {/if}
    {#if covers[p.node_id]?.length}
      <div class="mt-2 flex flex-wrap gap-1">
        {#each covers[p.node_id] as c (c.wi_number)}
          <button
            class="rounded bg-black px-1.5 py-0.5 text-[10px] text-white hover:bg-[var(--color-accent)]"
            title={`Preview #${c.wi_number} — ${c.title}`}
            onclick={() => openPreview(c.wi_number)}
          >#{c.wi_number} {c.title}</button>
        {/each}
      </div>
    {/if}
    {#if relatedByProposal[p.node_id]?.length}
      <div class="mt-1 flex flex-wrap gap-1">
        {#each relatedByProposal[p.node_id] as r (r.rel_id)}
          <button
            class="rounded border border-[var(--color-border)] px-1.5 py-0.5 text-[10px] hover:bg-[var(--color-accent-soft)]"
            class:border-[var(--color-accent)]={r.label === "has_handoff"}
            title={`Preview: ${r.label} — ${r.title}`}
            onclick={() => (previewNode = r.node_id)}
          >{r.label === "has_handoff" ? "📄" : r.label} {r.title}</button>
        {/each}
      </div>
    {/if}
    <div class="mt-2 flex items-center gap-2">
      <!-- Park is offered from both live statuses, because a proposal can go
           dormant before it starts as easily as after (#1534). Un-parking is
           two buttons rather than one, and deliberately so: the status a row
           was parked out of is not recoverable from the row, so korg refuses to
           guess it — the UI asks the same question the tool does instead of
           inventing a default here. -->
      {#if p.status === "proposed"}
        <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={() => setStatus(p, "active")}>Start</button>
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, PARKED)}>Park</button>
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, "declined")}>Decline</button>
      {:else if p.status === "active"}
        <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={() => setStatus(p, "done")}>Done</button>
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, PARKED)}>Park</button>
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, "declined")}>Decline</button>
      {:else if isParked(p.status)}
        <button class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-xs hover:bg-[var(--color-accent)]" onclick={() => setStatus(p, "active")}>Resume</button>
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, "proposed")}>To queue</button>
      {:else}
        <button class="rounded border border-[var(--color-border)] px-2 py-1 text-xs hover:bg-[var(--color-surface)]" onclick={() => setStatus(p, "proposed")}>Reopen</button>
      {/if}
      <!-- "When was this queued?" — the one timestamp question #1106 found
           worth a row rather than only a slide-over, because the queue is
           ordered by rank and nothing else on the card says how long something
           has been sitting in it. Date only: the hour a proposal was filed has
           never been the question. -->
      <span
        class="ml-auto text-xs text-[var(--color-muted)]"
        title={`Queued ${p.created}`}
        data-testid="proposal-queued">queued {stampDay(p.created)}</span
      >
      <span class="text-xs font-bold text-blue-400">Sprint Plan ID: {p.node_id}</span>
    </div>
  </div>
{/snippet}

<section class="space-y-4">
  <!-- Wraps at narrow widths (WI #549). The header row pushed 41px past a
       390px viewport, which scrolls the whole page sideways — the same class of
       mobile failure as the nav, just less obvious because the overflowing part
       is a filter rather than a link. -->
  <!-- WI #823 — the blurb sits beside the header now that the project selector
       has become a rail below. It reads as a subtitle to "Planning", which is
       what it always was. -->
  <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
    <h1 class="text-xl font-semibold">Planning</h1>
    <p class="text-xs text-[var(--color-muted)]">Proposals agents (or you) can drag to reorder — pinned always sort first.</p>
  </div>

  {#if loadError}
    <ErrorNotice error={loadError} what="the planning queue" retry={load} />
  {/if}

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else}
    <!-- Same 14rem rail + fluid content grid as Work Items, so the two pages
         line up when you flip between them. -->
    <div class="grid grid-cols-1 gap-4 md:grid-cols-[14rem_1fr]">
      <aside class="space-y-2">
        <!-- The legend sits ABOVE the rail, not under it with the checkboxes:
             the rail runs to thirty-odd entries, so anything below it is off
             the bottom of the screen and three bare numbers stay unexplained
             for as long as you use the page. Cheaper than three tooltips
             nobody hovers. -->
        <p class="px-1 text-[0.65rem] leading-relaxed text-[var(--color-muted)]">
          <span class="font-mono">proposals</span> |
          <span class="font-mono" style={`color: ${IN_PROPOSAL_COLOR}`}>in a proposal</span
          ><span class="font-mono">/open</span>
        </p>
        <nav class="overflow-hidden rounded border border-[var(--color-border)] bg-[var(--color-surface)]" data-testid="planning-project-rail">
          <button
            class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
            class:bg-[var(--color-surface-hi)]={projectFilter === ALL_PROJECTS}
            aria-current={projectFilter === ALL_PROJECTS ? "true" : "false"}
            onclick={() => pickProject(ALL_PROJECTS)}
          >
            <span class="flex-1">All Projects</span>
            {@render counts(allCounts)}
          </button>
          {#if groupByCategory}
            {#each projectGroups as g (g.key)}
              <p class="border-t border-[var(--color-border)] bg-[var(--color-bg)] px-3 py-1 text-[0.65rem] uppercase tracking-wide text-[var(--color-muted)]">{g.label}</p>
              {#each g.projects as p (p.id)}{@render projectRow(p)}{/each}
            {/each}
          {:else}
            {#each visibleProjects as p (p.id)}{@render projectRow(p)}{/each}
          {/if}
        </nav>
        <label class="flex items-center gap-1 px-1 text-xs text-[var(--color-muted)]">
          <input type="checkbox" bind:checked={showAllProjects} /> show all projects
        </label>
        <label class="flex items-center gap-1 px-1 text-xs text-[var(--color-muted)]">
          <input
            type="checkbox"
            bind:checked={groupByCategory}
            onchange={() => writeSticky(GROUP_KEY, groupByCategory ? "1" : "0")}
          /> by category
        </label>
      </aside>

      <div class="min-w-0 space-y-4">
    {#if active.length > 0}
      <div>
        <h2 class="mb-2 text-sm font-semibold text-[var(--color-muted)]">Active</h2>
        <div class="space-y-2">{#each active as p (p.node_id)}{@render card(p)}{/each}</div>
      </div>
    {/if}

    <div>
      <h2 class="mb-2 text-sm font-semibold text-[var(--color-muted)]">📌 Pinned</h2>
      <div
        class="min-h-[3rem] space-y-2 rounded border border-dashed border-[var(--color-border)] p-2"
        data-testid="pinned-zone"
        use:dndzone={{ items: pinnedBuf, flipDurationMs: flip, dropTargetStyle: {} }}
        onconsider={(e) => considerPinned(e as CustomEvent<DndEvent<DndItem>>)}
        onfinalize={(e) => finalizePinned(e as CustomEvent<DndEvent<DndItem>>)}
      >
        {#each pinnedBuf as item (item.id)}{@render card(item.proposal)}{:else}<p class="p-2 text-xs text-[var(--color-muted)]">Drag a proposal here to pin it.</p>{/each}
      </div>
    </div>

    <div>
      <h2 class="mb-2 text-sm font-semibold text-[var(--color-muted)]">Queue</h2>
      <div
        class="min-h-[3rem] space-y-2 rounded border border-dashed border-[var(--color-border)] p-2"
        data-testid="queue-zone"
        use:dndzone={{ items: queueBuf, flipDurationMs: flip, dropTargetStyle: {} }}
        onconsider={(e) => considerQueue(e as CustomEvent<DndEvent<DndItem>>)}
        onfinalize={(e) => finalizeQueue(e as CustomEvent<DndEvent<DndItem>>)}
      >
        {#each queueBuf as item (item.id)}{@render card(item.proposal)}{:else}<p class="p-2 text-xs text-[var(--color-muted)]">No proposals waiting. Ask your agent to propose some.</p>{/each}
      </div>
    </div>

    {#if parked.length > 0}
      <div data-testid="parked-section">
        <h2
          class="mb-2 border-t border-[var(--color-border)] pt-3 text-xs uppercase tracking-wide text-[var(--color-muted)]"
          data-testid="parked-divider"
        >
          <span class="mr-2">parked</span>
          <span class="text-[0.65rem] normal-case tracking-normal"
            >waiting on a condition, not on you — {parked.length}
            {parked.length === 1 ? "proposal" : "proposals"}</span
          >
        </h2>
        <div class="space-y-2">{#each parked as p (p.node_id)}{@render card(p)}{/each}</div>
      </div>
    {/if}

    <div>
      <button class="text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]" onclick={() => (showDone = !showDone)}>{showDone ? "▾" : "▸"} Done ({done.length})</button>
      {#if showDone}<div class="mt-2 space-y-2">{#each done as p (p.node_id)}{@render card(p)}{/each}</div>{/if}
    </div>
    <div>
      <button class="text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]" onclick={() => (showDeclined = !showDeclined)}>{showDeclined ? "▾" : "▸"} Declined ({declined.length})</button>
      {#if showDeclined}<div class="mt-2 space-y-2">{#each declined as p (p.node_id)}{@render card(p)}{/each}</div>{/if}
    </div>
      </div>
    </div>
  {/if}
</section>

<!-- The rail's count (WI #823), in Ken's preferred full form:
     `<proposals> | <wi_in_proposal> / <wi_total>`.
     His instruction was to time it rather than pre-degrade to fewer figures —
     the measurement is in the sprint record, and all three shipped. The middle
     figure takes the "spoken for" colour so the rail and the rows agree on
     what warm means. -->
{#snippet counts(c: { proposals: number; wi_in_proposal: number; wi_total: number })}
  <span class="shrink-0 font-mono text-[0.65rem] tabular-nums text-[var(--color-muted)]">
    {c.proposals} | <span style={c.wi_in_proposal > 0 ? `color: ${IN_PROPOSAL_COLOR}` : ""}
      >{c.wi_in_proposal}</span
    >/{c.wi_total}
  </span>
{/snippet}

<!-- A rail entry. Deliberately NOT the Work Items rail's snippet: this one has
     no edit-project pencil and no add-area diamond (Ken, #823) — the Planning
     page is for choosing what to work on, not for maintaining the roster — and
     the count takes the space they used. -->
{#snippet projectRow(p: ProjectRow)}
  <button
    class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-[var(--color-surface-hi)]"
    class:bg-[var(--color-surface-hi)]={projectFilter === p.name}
    class:font-semibold={projectFilter === p.name}
    aria-current={projectFilter === p.name ? "true" : "false"}
    onclick={() => pickProject(p.name)}
  >
    <span class="flex-1 truncate" style={projectRailColor(p) ? `color: ${projectRailColor(p)}` : ""}
      >{p.name}{#if p.status !== "active"}<span class="ml-1 text-xs text-[var(--color-muted)]">({p.status})</span>{/if}</span
    >
    {@render counts(rollup[p.name] ?? { proposals: 0, wi_in_proposal: 0, wi_total: 0 })}
  </button>
{/snippet}

{#if previewNode != null}
  <NodePreview nodeId={previewNode} onClose={() => (previewNode = null)} />
{/if}

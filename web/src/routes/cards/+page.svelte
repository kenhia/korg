<script lang="ts">
  import { onMount } from "svelte";
  import { dndzone, type DndEvent } from "svelte-dnd-action";
  import {
    api,
    type CardRow,
    type DailyPlanItem,
    type Comment,
  } from "$lib/api";
  import { CARD_STATUSES, type CardStatus } from "$lib/generated/vocab";
  import { activeCardStatuses, chip, CUT, isCut, midRank } from "$lib/domain";
  import { attempt, notify, reportError } from "$lib/toast.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import Dialog from "$lib/components/Dialog.svelte";
  import Comments from "$lib/components/Comments.svelte";
  import {
    startOfWeek,
    addDays,
    isoDate,
    weekDays,
    WEEKDAY_LABELS,
  } from "$lib/dates";
  import { extractUrls } from "$lib/urls";

  type DndItem = { id: number; card: CardRow };

  const NON_CUT = activeCardStatuses();
  const STATUS_IDX = (s: string) => CARD_STATUSES.indexOf(s as CardStatus);
  let cutExpanded = $state(false);

  let cardsRaw = $state<CardRow[]>([]);
  let board = $state<Record<CardStatus, DndItem[]>>(emptyBoard());
  let loading = $state(true);
  let loadError = $state<unknown>(null);
  let view = $state<"board" | "list">("board");
  let newTitle = $state("");
  const flip = 150;

  // --- filters ---
  let fSearch = $state("");
  let fProject = $state("");
  let fCategory = $state("");
  let fTags = $state<Set<string>>(new Set());
  let showCut = $state(false);
  let showArchived = $state(false);

  const projOptions = $derived(
    [
      ...new Set(
        cardsRaw.map((c) => c.project).filter((p): p is string => !!p),
      ),
    ].sort(),
  );
  const catOptions = $derived(
    [
      ...new Set(
        cardsRaw.map((c) => c.category).filter((c): c is string => !!c),
      ),
    ].sort(),
  );
  const tagOptions = $derived(
    [...new Set(cardsRaw.flatMap((c) => c.tags))].sort(),
  );

  function passesCommon(c: CardRow): boolean {
    if (!showArchived && c.archived) return false;
    if (fProject && c.project !== fProject) return false;
    if (fCategory && c.category !== fCategory) return false;
    for (const t of fTags) if (!c.tags.includes(t)) return false;
    if (fSearch.trim() !== "") {
      const q = fSearch.toLowerCase();
      if (
        !(
          c.title.toLowerCase().includes(q) ||
          c.description.toLowerCase().includes(q)
        )
      )
        return false;
    }
    return true;
  }

  const listCards = $derived(
    cardsRaw
      .filter((c) => passesCommon(c) && (showCut || !isCut(c.status)))
      .sort(
        (a, b) =>
          STATUS_IDX(a.status) - STATUS_IDX(b.status) ||
          Number(a.rank) - Number(b.rank),
      ),
  );

  function resetFilters() {
    fSearch = "";
    fProject = "";
    fCategory = "";
    fTags = new Set();
    showCut = false;
    showArchived = false;
    rebuild();
  }
  function toggleTag(t: string) {
    const s = new Set(fTags);
    if (s.has(t)) s.delete(t);
    else s.add(t);
    fTags = s;
    rebuild();
  }

  function emptyBoard(): Record<CardStatus, DndItem[]> {
    const b = {} as Record<CardStatus, DndItem[]>;
    for (const s of CARD_STATUSES) b[s] = [];
    return b;
  }
  function rebuild() {
    const b = emptyBoard();
    for (const c of cardsRaw) {
      if (!passesCommon(c)) continue;
      b[c.status as CardStatus].push({ id: c.node_id, card: c });
    }
    for (const s of CARD_STATUSES)
      b[s].sort((x, y) => Number(x.card.rank) - Number(y.card.rank));
    board = b;
  }

  async function load() {
    loading = true;
    loadError = null;
    try {
      // The page filters archived client-side behind a toggle, so it needs both.
      cardsRaw = (await api.cards({ archived: "all" })).items;
      rebuild();
    } catch (e) {
      loadError = e;
    } finally {
      loading = false;
    }
  }

  async function add() {
    if (newTitle.trim() === "") return;
    const title = newTitle.trim();
    const created = await attempt(() => api.createCard({ title }), "Add card");
    if (!created) return;
    newTitle = "";
    await load();
  }

  function consider(status: CardStatus, e: CustomEvent<DndEvent<DndItem>>) {
    board[status] = e.detail.items;
  }
  async function finalize(
    status: CardStatus,
    e: CustomEvent<DndEvent<DndItem>>,
  ) {
    board[status] = e.detail.items;
    const movedId = e.detail.info.id;
    const items = board[status];
    const idx = items.findIndex((it) => String(it.id) === String(movedId));
    if (idx < 0) return;
    const rank = midRank(items[idx - 1]?.card.rank, items[idx + 1]?.card.rank);
    const moved = items[idx].card;
    try {
      await api.updateCard(moved.node_id, { status, rank });
      moved.status = status;
      moved.rank = String(rank);
    } catch (err) {
      // A rejected move leaves the board showing a position the server does not
      // have, so reload to resynchronise rather than leaving a lie on screen.
      reportError(err, "Move card");
      await load();
    }
  }

  // --- daily-plan card drop targets ---
  const SOURCE_DRAG = "application/x-korg-source";
  let weekStart = $state(startOfWeek(new Date()));
  let planItems = $state<DailyPlanItem[]>([]);
  let planNotice = $state<string | null>(null);
  const days = $derived(weekDays(weekStart));
  const today = $derived(isoDate(new Date()));
  function planForDay(day: Date): DailyPlanItem[] {
    const date = isoDate(day);
    return planItems
      .filter((item) => item.plan_date === date)
      .sort((a, b) => a.position - b.position);
  }
  async function loadPlan() {
    const from = isoDate(weekStart);
    const to = isoDate(addDays(weekStart, 6));
    planItems = await api.dailyPlan(from, to);
  }
  function shiftWeek(delta: number) {
    weekStart = addDays(weekStart, delta * 7);
    void loadPlan();
  }
  function dragCard(event: DragEvent, card: CardRow) {
    event.stopPropagation();
    event.dataTransfer?.setData(SOURCE_DRAG, String(card.node_id));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy";
  }
  async function dropCard(event: DragEvent, planDate: string) {
    event.preventDefault();
    if (planDate < today) return;
    const raw = event.dataTransfer?.getData(SOURCE_DRAG) ?? "";
    if (!/^\d+$/.test(raw)) return;
    try {
      await api.createDailyPlanItem(Number(raw), planDate);
      planNotice =
        "Card added to the daily plan; its board status is unchanged.";
      await loadPlan();
    } catch (err) {
      reportError(err, "Add card to the daily plan");
    }
  }

  // --- edit modal (+ comments) ---
  let editing = $state<CardRow | null>(null);
  let form = $state({
    title: "",
    status: "Backlog" as CardStatus,
    project: "",
    category: "",
    description: "",
    tags: "",
  });
  let original = $state("");
  let showDiscard = $state(false);
  let comments = $state<Comment[]>([]); // bound from <Comments>, read for launchUrls
  let launchUrls = $derived(
    extractUrls(form.description, ...comments.map((c) => c.body)),
  );
  const dirty = $derived(editing !== null && JSON.stringify(form) !== original);

  function openEdit(card: CardRow) {
    editing = card;
    form = {
      title: card.title,
      // The server validates the vocabulary, so a row's status is always one of
      // CARD_STATUSES; the generated type carries the DB's plain text.
      status: card.status as CardStatus,
      project: card.project ?? "",
      category: card.category ?? "",
      description: card.description,
      tags: card.tags.join(", "),
    };
    original = JSON.stringify(form);
    showDiscard = false;
    comments = [];
  }
  function requestClose() {
    if (dirty) showDiscard = true;
    else editing = null;
  }
  function discard() {
    showDiscard = false;
    editing = null;
  }
  async function saveEdit() {
    if (!editing) return;
    const tags = form.tags
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t !== "");
    // The API now takes project_id on both surfaces (WI #537). Creating a
    // project from a typed name is a UI affordance, so it happens here rather
    // than as a hidden side effect of a card PATCH. createProject is
    // idempotent, so this is one call whether or not the project exists.
    const projectName = form.project.trim();
    const node_id = editing.node_id;
    const saved = await attempt(async () => {
      const project_id =
        projectName === "" ? null : (await api.createProject(projectName)).id;
      return api.updateCard(node_id, {
        title: form.title,
        status: form.status,
        description: form.description,
        project_id,
        category: form.category.trim() === "" ? null : form.category.trim(),
        tags,
      });
    }, "Save card");
    // Keep the editor open on failure — closing it would discard the edits the
    // user just failed to save.
    if (!saved) return;
    editing = null;
    await load();
  }

  // Archiving is reversible, so it gets an undo toast rather than a confirm
  // (WI #549). Confirms on reversible actions teach people to click through
  // confirms, which is how the one that mattered stops working.
  async function toggleArchiveCard() {
    if (!editing) return;
    const node_id = editing.node_id;
    const was = editing.archived;
    const r = await attempt(
      () => api.updateCard(node_id, { archived: !was }),
      was ? "Restore card" : "Archive card",
    );
    if (!r) return;
    editing.archived = !was;
    await load();
    notify(was ? "Card restored." : "Card archived.", async () => {
      const undone = await attempt(
        () => api.updateCard(node_id, { archived: was }),
        "Undo",
      );
      if (undone) {
        if (editing?.node_id === node_id) editing.archived = was;
        await load();
      }
    });
  }

  onMount(async () => {
    await load();
    await loadPlan();
  });
</script>

{#snippet tile(item: DndItem)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- The keyboard handler is `onkeydowncapture`, which the compiler does not
       count as one. It has to be: `svelte-dnd-action` registers its own keydown
       listener and stops propagation, so a bubble-phase handler here never
       runs — which is why the pre-existing Enter binding silently did nothing.
       See the handler below. -->
  <div
    class="cursor-grab rounded bg-[var(--color-surface-hi)] p-2 active:cursor-grabbing"
    data-testid={`card-${item.id}`}
    onclick={() => openEdit(item.card)}
    role="button"
    tabindex="0"
    aria-label={`Edit card: ${item.card.title}`}
    onkeydowncapture={(e) => {
      // Capture phase, deliberately. The WI reports that the tile "ignores
      // Space"; measuring it showed the tile ignored **Enter** too, despite
      // having handled it since the board was written. `svelte-dnd-action`
      // registers its own keydown listener for keyboard dragging and stops
      // propagation, and Svelte 5 delegates `onkeydown` to the root — so the
      // bubble-phase handler never ran. Capture fires first, which is the only
      // way to keep both behaviours on one element.
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        e.stopPropagation();
        openEdit(item.card);
      }
    }}
  >
    <div class="flex items-start gap-2">
      <div class="min-w-0 flex-1 text-sm">{item.card.title}</div>
      <button
        type="button"
        draggable="true"
        class="cursor-grab rounded px-1 text-xs text-[var(--color-muted)] hover:bg-[var(--color-bg)] hover:text-[var(--color-accent)]"
        aria-label={`Plan ${item.card.title}`}
        title="Drag to a day above"
        onclick={(event) => event.stopPropagation()}
        ondragstart={(event) => dragCard(event, item.card)}>↗</button
      >
    </div>
    {#if item.card.project || item.card.category}
      <div class="mt-1 flex flex-wrap gap-1">
        {#if item.card.project}<span class={chip.project} title="Project"
            >{item.card.project}</span
          >{/if}
        {#if item.card.category}<span class={chip.category} title="Category"
            >{item.card.category}</span
          >{/if}
      </div>
    {/if}
    {#if item.card.tags.length > 0}
      <div class="mt-1 flex flex-wrap gap-1">
        {#each item.card.tags as tag (tag)}<span class={chip.tag}
            >{tag}</span
          >{/each}
      </div>
    {/if}
  </div>
{/snippet}

<section class="space-y-4">
  <div class="flex items-center justify-between">
    <h1 class="text-xl font-semibold">Cards</h1>
    <div
      class="flex overflow-hidden rounded border border-[var(--color-border)] text-sm"
    >
      <button
        class="px-3 py-1"
        class:bg-[var(--color-surface-hi)]={view === "board"}
        onclick={() => (view = "board")}>Board</button
      >
      <button
        class="px-3 py-1"
        class:bg-[var(--color-surface-hi)]={view === "list"}
        onclick={() => (view = "list")}>List</button
      >
    </div>
  </div>

  <div
    class="flex items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
  >
    <input
      class="flex-1 rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
      placeholder="New card title…"
      bind:value={newTitle}
      onkeydown={(e) => e.key === "Enter" && add()}
    />
    <button
      class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]"
      onclick={add}>Add</button
    >
  </div>

  {#if loadError}
    <ErrorNotice error={loadError} what="the board" retry={load} />
  {/if}

  <!-- Daily-plan targets: dropping adds a source occurrence and does not move the card. -->
  <div
    class="rounded border border-[var(--color-border)] bg-[var(--color-surface)]"
  >
    <div class="flex items-center justify-between px-3 py-2">
      <div>
        <h2 class="text-sm font-medium">Plan this week</h2>
        <p class="text-xs text-[var(--color-muted)]">
          Drag a card to an open day.
        </p>
      </div>
      <div class="flex items-center gap-1 text-xs">
        <button
          class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]"
          onclick={() => shiftWeek(-1)}>← Prev</button
        ><button
          class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]"
          onclick={() => {
            weekStart = startOfWeek(new Date());
            void loadPlan();
          }}>Today</button
        ><button
          class="rounded px-2 py-1 hover:bg-[var(--color-surface-hi)]"
          onclick={() => shiftWeek(1)}>Next →</button
        >
      </div>
    </div>
    {#if planNotice}<p
        class="mx-3 rounded bg-sky-950 px-2 py-1 text-xs text-sky-200"
        role="status"
      >
        {planNotice}
      </p>{/if}
    <div class="grid grid-cols-2 gap-2 p-3 sm:grid-cols-4 lg:grid-cols-7">
      {#each days as day, i (isoDate(day))}
        {@const date = isoDate(day)}
        <div
          class="min-h-20 rounded border border-dashed border-[var(--color-border)] bg-[var(--color-bg)] p-2"
          class:opacity-50={date < today}
          data-testid={`card-plan-day-${date}`}
          role="group"
          aria-label={`Plan for ${date}${date < today ? ", frozen" : ", drop card here"}`}
          ondragover={(event) => {
            if (date >= today) event.preventDefault();
          }}
          ondrop={(event) => dropCard(event, date)}
        >
          <div class="mb-1 text-xs font-medium text-[var(--color-muted)]">
            {WEEKDAY_LABELS[i]}
            {day.getDate()}
            {#if date < today}<span class="float-right text-[9px] uppercase"
                >frozen</span
              >{/if}
          </div>
          {#each planForDay(day) as item (item.node_id)}<div
              class="mb-1 truncate rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-[10px]"
              title={item.display}
            >
              {item.display}
            </div>{:else}<div
              class="pt-2 text-center text-[10px] text-[var(--color-muted)]"
            >
              {date < today ? "No items" : "Drop card"}
            </div>{/each}
        </div>
      {/each}
    </div>
  </div>

  {#if loading}
    <p class="text-[var(--color-muted)]">Loading…</p>
  {:else if view === "board"}
    <p class="mb-2 text-xs text-[var(--color-muted)]">
      Drag cards within or across columns — drop anywhere to set order.
    </p>
    <div class="flex flex-wrap gap-3">
      {#each NON_CUT as status (status)}
        <div
          class="min-w-[8rem] flex-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2"
        >
          <div class="mb-2 flex items-center justify-between">
            <span class="text-xs font-medium text-[var(--color-muted)]"
              >{status}</span
            >
            <span class="text-xs text-[var(--color-muted)]"
              >{board[status].length}</span
            >
          </div>
          <div
            class="min-h-[3rem] space-y-2"
            data-testid={`col-${status}`}
            use:dndzone={{
              items: board[status],
              flipDurationMs: flip,
              dropTargetStyle: {},
            }}
            onconsider={(e) =>
              consider(status, e as CustomEvent<DndEvent<DndItem>>)}
            onfinalize={(e) =>
              finalize(status, e as CustomEvent<DndEvent<DndItem>>)}
          >
            {#each board[status] as item (item.id)}{@render tile(item)}{/each}
          </div>
        </div>
      {/each}
      <div
        class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 transition-all"
        class:flex-1={cutExpanded}
        class:min-w-[8rem]={cutExpanded}
        class:w-12={!cutExpanded}
      >
        <button
          class="mb-2 flex w-full items-center justify-between gap-1 text-xs font-medium text-[var(--color-muted)] hover:text-[var(--color-text)]"
          data-testid="cut-toggle"
          onclick={() => (cutExpanded = !cutExpanded)}
          title={cutExpanded ? "Collapse Cut" : "Expand Cut"}
        >
          <span>{cutExpanded ? CUT : CUT.toUpperCase()}</span><span
            >{board[CUT].length}</span
          >
        </button>
        <div
          class="min-h-[3rem] space-y-2"
          class:overflow-hidden={!cutExpanded}
          data-testid="col-Cut"
          use:dndzone={{
            items: board[CUT],
            flipDurationMs: flip,
            dropTargetStyle: {},
          }}
          onconsider={(e) =>
            consider(CUT, e as CustomEvent<DndEvent<DndItem>>)}
          onfinalize={(e) =>
            finalize(CUT, e as CustomEvent<DndEvent<DndItem>>)}
        >
          {#each board[CUT] as item (item.id)}<div
              class:hidden={!cutExpanded}
            >
              {@render tile(item)}
            </div>{/each}
        </div>
      </div>
    </div>
  {:else}
    <div class="grid gap-4 lg:grid-cols-[15rem_1fr]">
      {@render filtersPanel()}
      <div class="overflow-auto rounded border border-[var(--color-border)]">
        <table class="w-full text-sm">
          <thead
            class="sticky top-0 bg-[var(--color-surface)] text-left text-xs text-[var(--color-muted)]"
          >
            <tr
              ><th class="px-3 py-2">Title</th><th class="px-3 py-2">Status</th
              ><th class="px-3 py-2">Project</th><th class="px-3 py-2"
                >Category</th
              ><th class="px-3 py-2">Tags</th><th class="px-3 py-2">Updated</th
              ></tr
            >
          </thead>
          <tbody>
            {#each listCards as card (card.node_id)}
              <tr
                class="cursor-pointer border-t border-[var(--color-border)] hover:bg-[var(--color-surface-hi)]"
                class:opacity-55={card.archived}
                onclick={() => openEdit(card)}
              >
                <!-- A real button in the cell rather than role="button" on the
                     <tr> (WI #548): a row that claims to be a button is no
                     longer a row, and the table loses its semantics. This keeps
                     both, and Enter/Space come free with the element. -->
                <td class="px-3 py-1.5 font-medium">
                  <button
                    class="w-full cursor-pointer text-left font-medium"
                    onclick={(e) => {
                      e.stopPropagation();
                      openEdit(card);
                    }}>{card.title}</button
                  >
                </td>
                <td class="px-3 py-1.5">{card.status}</td>
                <td class="px-3 py-1.5"
                  >{#if card.project}<span class={chip.project}
                      >{card.project}</span
                    >{/if}</td
                >
                <td class="px-3 py-1.5"
                  >{#if card.category}<span class={chip.category}
                      >{card.category}</span
                    >{/if}</td
                >
                <td class="px-3 py-1.5"
                  ><div class="flex flex-wrap gap-1">
                    {#each card.tags as t (t)}<span class={chip.tag}
                        >{t}</span
                      >{/each}
                  </div></td
                >
                <td class="px-3 py-1.5 text-[var(--color-muted)]"
                  >{new Date(card.updated).toLocaleDateString()}</td
                >
              </tr>
            {:else}
              <tr
                ><td
                  class="px-3 py-3 text-sm text-[var(--color-muted)]"
                  colspan="6">No cards.</td
                ></tr
              >
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {/if}

  {#snippet filtersPanel()}
    <aside
      class="space-y-3 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm"
    >
      <div>
        <label
          class="mb-1 block text-xs text-[var(--color-muted)]"
          for="card-search">Search</label
        >
        <input
          id="card-search"
          type="search"
          data-testid="filter-search"
          class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none"
          placeholder="title or description…"
          bind:value={fSearch}
          oninput={rebuild}
        />
      </div>
      <div>
        <label
          class="mb-1 block text-xs text-[var(--color-muted)]"
          for="card-project">Project</label
        >
        <select
          id="card-project"
          class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none"
          bind:value={fProject}
          onchange={rebuild}
        >
          <option value="">— any —</option>
          {#each projOptions as p (p)}<option value={p}>{p}</option>{/each}
        </select>
      </div>
      <div>
        <label
          class="mb-1 block text-xs text-[var(--color-muted)]"
          for="card-category">Category</label
        >
        <select
          id="card-category"
          class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none"
          bind:value={fCategory}
          onchange={rebuild}
        >
          <option value="">— any —</option>
          {#each catOptions as c (c)}<option value={c}>{c}</option>{/each}
        </select>
      </div>
      <fieldset class="border-0 p-0">
        <legend class="mb-1 text-xs text-[var(--color-muted)]"
          >Tags (AND)</legend
        >
        <div class="flex flex-wrap gap-1" data-testid="filter-tags">
          {#each tagOptions as t (t)}
            <button
              type="button"
              class="rounded border px-2 py-0.5 text-xs"
              class:border-[var(--color-accent)]={fTags.has(t)}
              class:bg-[var(--color-accent-soft)]={fTags.has(t)}
              class:border-[var(--color-border)]={!fTags.has(t)}
              onclick={() => toggleTag(t)}>{t}</button
            >
          {:else}<span class="text-xs text-[var(--color-muted)]"
              >no tags yet</span
            >{/each}
        </div>
      </fieldset>
      <label class="flex items-center gap-2 text-xs text-[var(--color-muted)]"
        ><input
          type="checkbox"
          data-testid="toggle-cut"
          bind:checked={showCut}
          onchange={rebuild}
        /> Show Cut</label
      >
      <label class="flex items-center gap-2 text-xs text-[var(--color-muted)]"
        ><input
          type="checkbox"
          data-testid="toggle-archived"
          bind:checked={showArchived}
          onchange={rebuild}
        /> Show Archived</label
      >
      <button
        type="button"
        class="w-full rounded border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-muted)] hover:text-[var(--color-text)]"
        onclick={resetFilters}>Reset filters</button
      >
    </aside>
  {/snippet}

  {#if editing}
    <!-- Was a hand-built overlay with Escape but no focus trap and no focus
         restore, behind a full-screen `<button>` scrim. `<dialog>` supplies all
         three (WI #548). -->
    <Dialog
      open={true}
      onClose={requestClose}
      placement="center"
      title="Edit card"
    >
      <div class="space-y-3" data-testid="card-modal">
        {#if editing.archived}
          <p>
            <span
              class="rounded bg-[var(--color-surface-hi)] px-1.5 py-0.5 text-xs uppercase text-[var(--color-muted)]"
              >archived</span
            >
          </p>
        {/if}

        <label class="sr-only" for="edit-card-title">Card title</label>
        <input
          id="edit-card-title"
          class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
          placeholder="Title"
          data-testid="edit-title"
          bind:value={form.title}
        />
        <div class="grid grid-cols-2 gap-2 text-sm">
          <label class="flex flex-col gap-1"
            ><span class="text-xs text-[var(--color-muted)]">Status</span>
            <select
              class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none"
              bind:value={form.status}
              >{#each CARD_STATUSES as s (s)}<option value={s}>{s}</option
                >{/each}</select
            >
          </label>
          <label class="flex flex-col gap-1"
            ><span class="text-xs text-[var(--color-muted)]">Project</span>
            <input
              class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none"
              bind:value={form.project}
            />
          </label>
          <label class="flex flex-col gap-1"
            ><span class="text-xs text-[var(--color-muted)]">Category</span>
            <input
              class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none"
              bind:value={form.category}
            />
          </label>
          <label class="flex flex-col gap-1"
            ><span class="text-xs text-[var(--color-muted)]">Tags</span>
            <input
              class="rounded bg-[var(--color-surface-hi)] px-2 py-1 outline-none"
              placeholder="comma, separated"
              bind:value={form.tags}
            />
          </label>
        </div>
        <textarea
          class="min-h-[8rem] w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none"
          placeholder="Description (markdown)"
          bind:value={form.description}
        ></textarea>

        <div class="flex items-center justify-between gap-2">
          <button
            class="rounded border border-[var(--color-border)] px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]"
            onclick={toggleArchiveCard}
            >{editing.archived ? "Unarchive" : "Archive"}</button
          >
          <div class="flex items-center gap-2">
            {#if dirty}<span class="text-xs text-[var(--color-muted)]"
                >Unsaved</span
              >{/if}
            <button
              class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]"
              onclick={requestClose}>Close</button
            >
            <button
              class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)]"
              onclick={saveEdit}>Save</button
            >
          </div>
        </div>

        <!-- comments -->
        <Comments node_id={editing.node_id} bind:comments />

        {#if launchUrls.length}
          <div
            class="border-t border-[var(--color-border)] pt-2"
            data-testid="launch-links"
          >
            <p class="mb-1 text-xs font-semibold text-[var(--color-muted)]">
              Links
            </p>
            <ul class="space-y-1">
              {#each launchUrls as url (url)}
                <li>
                  <a
                    class="block truncate text-sm text-[var(--color-accent)] hover:underline"
                    href={url}
                    target="_blank"
                    rel="noopener noreferrer"
                    title={url}>{url}</a
                  >
                </li>
              {/each}
            </ul>
          </div>
        {/if}

        {#if showDiscard}
          <div
            class="rounded border border-[var(--color-border)] bg-[var(--color-bg)] p-3"
            data-testid="discard-prompt"
          >
            <p class="mb-2 text-sm">Discard unsaved changes?</p>
            <div class="flex justify-end gap-2">
              <button
                class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]"
                onclick={() => (showDiscard = false)}>Keep editing</button
              >
              <button
                class="rounded bg-red-900 px-3 py-1.5 text-sm hover:bg-red-800"
                data-testid="discard-confirm"
                onclick={discard}>Discard</button
              >
            </div>
          </div>
        {/if}
      </div>
    </Dialog>
  {/if}
</section>

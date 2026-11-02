<script lang="ts">
  // The activity log (WI 2936): what changed in korg, newest first. Ken runs
  // several agents at once; this is where he sees what they did.
  //
  // One line per change, in Ken's v1 shape (2936 comment 4279):
  //   11/1 20:37: Comment Added - WI:1234 Some title  proposal:5678 program:9012
  // The proposal and program are resolved by the server from the edges at read
  // time, never stored on the row. Filters live in the URL, as on Search, so a
  // filtered view is linkable and the back button works.
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import {
    api,
    type ActivityAction,
    type ActivityEvent,
    type ActivityRef,
    type ProjectRow,
  } from "$lib/api";
  import { ACTIVITY_ACTIONS, NODE_KINDS } from "$lib/generated/vocab";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";

  const PAGE = 100;

  const action = $derived($page.url.searchParams.get("action") ?? "");
  const kind = $derived($page.url.searchParams.get("kind") ?? "");
  const project = $derived($page.url.searchParams.get("project") ?? "");
  const windowKey = $derived($page.url.searchParams.get("window") ?? "");

  // Windows are relative to the moment the view loads; `since` is computed
  // then, so a refresh moves it forward with the clock.
  const WINDOWS: [string, string, number | null][] = [
    ["", "Any time", null],
    ["1h", "Last hour", 60 * 60 * 1000],
    ["24h", "Last 24 hours", 24 * 60 * 60 * 1000],
    ["7d", "Last 7 days", 7 * 24 * 60 * 60 * 1000],
    ["30d", "Last 30 days", 30 * 24 * 60 * 60 * 1000],
  ];

  let events = $state<ActivityEvent[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state<unknown>(null);
  let projects = $state<ProjectRow[]>([]);
  let refreshed = $state(0);

  api
    .projects()
    .then((rows) => (projects = rows.filter((p) => p.status === "active")))
    .catch(() => (projects = []));

  function params(offset: number) {
    const span = WINDOWS.find(([v]) => v === windowKey)?.[2] ?? null;
    return {
      action: (action || undefined) as ActivityAction | undefined,
      kind: kind || undefined,
      project: project || undefined,
      since: span === null ? undefined : new Date(Date.now() - span).toISOString(),
      limit: PAGE,
      offset,
    };
  }

  $effect(() => {
    void refreshed;
    const p = params(0);
    loading = true;
    error = null;
    api
      .activity(p)
      .then((r) => {
        events = r.items;
        total = r.total;
      })
      .catch((e) => (error = e))
      .finally(() => (loading = false));
  });

  function more() {
    loading = true;
    api
      .activity(params(events.length))
      .then((r) => {
        events = [...events, ...r.items];
        total = r.total;
      })
      .catch((e) => (error = e))
      .finally(() => (loading = false));
  }

  function navigate(next: Record<string, string>) {
    const url = new URL($page.url);
    for (const [k, v] of Object.entries(next)) {
      if (v === "") url.searchParams.delete(k);
      else url.searchParams.set(k, v);
    }
    goto(`${url.pathname}${url.search}`, { keepFocus: true, noScroll: true });
  }

  /** `11/1 20:37`, in the browser's own zone. */
  function when(iso: string): string {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    const hh = String(d.getHours()).padStart(2, "0");
    const mm = String(d.getMinutes()).padStart(2, "0");
    return `${d.getMonth() + 1}/${d.getDate()} ${hh}:${mm}`;
  }

  /** `status_changed` -> `Status Changed`. */
  function label(a: string): string {
    return a
      .split("_")
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ");
  }

  const HANDLE: Record<string, string> = {
    workitem: "WI",
    sprint_proposal: "proposal",
    program: "program",
    card: "card",
    link: "link",
    report: "report",
    handoff: "handoff",
    schedule: "schedule",
    attachment: "image",
  };

  /** `WI:1234`, `proposal:5678` — the handle Ken reads a line by. */
  function handle(r: ActivityRef): string {
    return `${HANDLE[r.kind] ?? r.kind}:${r.wi_number ?? r.node_id}`;
  }

  const KIND_LABELS: Record<string, string> = {
    workitem: "Work items",
    card: "Cards",
    link: "Links",
    sprint_proposal: "Proposals",
    report: "Reports",
    handoff: "Handoffs",
    program: "Programs",
    schedule: "Schedules",
    attachment: "Images",
  };
</script>

<svelte:head><title>korg — activity</title></svelte:head>

<div class="mb-4 flex items-baseline gap-3">
  <h1 class="text-xl font-semibold">Activity</h1>
  <button
    class="text-sm underline text-[var(--color-muted)]"
    onclick={() => (refreshed += 1)}>refresh</button
  >
</div>

<div class="mb-4 flex flex-wrap items-center gap-3 text-sm">
  {#snippet picker(
    id: string,
    what: string,
    value: string,
    key: string,
    options: [string, string][],
  )}
    <label class="sr-only" for={id}>{what}</label>
    <select
      {id}
      class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1"
      {value}
      onchange={(e) => navigate({ [key]: e.currentTarget.value })}
    >
      {#each options as [v, text] (v)}
        <option value={v}>{text}</option>
      {/each}
    </select>
  {/snippet}

  {@render picker("activity-action", "Filter by action", action, "action", [
    ["", "All actions"],
    ...ACTIVITY_ACTIONS.map((a): [string, string] => [a, label(a)]),
  ])}
  {@render picker("activity-kind", "Filter by kind", kind, "kind", [
    ["", "All kinds"],
    ...NODE_KINDS.map((k): [string, string] => [k, KIND_LABELS[k] ?? k]),
  ])}
  {@render picker("activity-project", "Filter by project", project, "project", [
    ["", "All projects"],
    ...projects.map((p): [string, string] => [p.name, p.name]),
  ])}
  {@render picker(
    "activity-window",
    "Filter by time",
    windowKey,
    "window",
    WINDOWS.map(([v, text]): [string, string] => [v, text]),
  )}
</div>

{#if error}
  <ErrorNotice what="the activity log" {error} />
{:else if loading && events.length === 0}
  <p class="text-[var(--color-muted)]">Loading…</p>
{:else if events.length === 0}
  <!-- No backfill (0038): the log starts the day it shipped, so an empty
       unfiltered view is the honest state of a new install, not a fault. -->
  <p class="text-[var(--color-muted)]">
    Nothing matches. The log records changes from the day it shipped onward;
    there is no history before that.
  </p>
{:else}
  <p class="mb-3 text-sm text-[var(--color-muted)]">
    {total}
    {total === 1 ? "change" : "changes"}
  </p>
  <ul class="space-y-1 font-mono text-sm">
    {#each events as e (e.key)}
      <li class="leading-snug">
        <span class="text-[var(--color-muted)]">{when(e.at)}:</span>
        <span class="font-semibold">{label(e.action)}</span>
        {#if e.action === "status_changed"}
          <span class="text-[var(--color-muted)]">({e.from_status} → {e.to_status})</span>
        {/if}
        -
        <a class="underline" href={e.target.url ?? `/n/${e.target.node_id}`}
          >{handle(e.target)}</a
        >
        <span class="font-sans">{e.target.title}</span>
        {#if e.other}
          <span class="text-[var(--color-muted)]">{e.label === "has_attachment" ? "" : e.label} →</span>
          <a class="underline" href={e.other.url ?? `/n/${e.other.node_id}`}
            >{handle(e.other)}</a
          >
        {/if}
        {#if e.proposal}
          <a
            class="ml-2 text-[var(--color-accent)] underline"
            href={e.proposal.url ?? `/n/${e.proposal.node_id}`}
            title={e.proposal.title}>proposal:{e.proposal.node_id}</a
          >
        {/if}
        {#if e.program}
          <a
            class="ml-1 text-[var(--color-accent)] underline"
            href={e.program.url ?? `/n/${e.program.node_id}`}
            title={e.program.title}>program:{e.program.node_id}</a
          >
        {/if}
      </li>
    {/each}
  </ul>
  {#if events.length < total}
    <button
      class="mt-4 rounded bg-[var(--color-accent-soft)] px-3 py-1 text-sm hover:bg-[var(--color-accent)]"
      disabled={loading}
      onclick={more}>Load more</button
    >
  {/if}
{/if}

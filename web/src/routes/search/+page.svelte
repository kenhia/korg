<script lang="ts">
  // Full-text search (#1177, sprint 066). The box lives in the header on every
  // page and lands here; the query is entirely in the URL, so a result set is
  // linkable and the back button works.
  //
  // Deliberately NOT a top-nav tab. Search is something you do *from* wherever
  // you are, not a place you go — a tab would make it a destination and put the
  // box one click further away from every page that isn't it.
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import { api, type SearchResults } from "$lib/api";
  import { nodeHref, stamp } from "$lib/domain";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import NodePreview from "$lib/components/NodePreview.svelte";

  const q = $derived($page.url.searchParams.get("q") ?? "");
  const everything = $derived($page.url.searchParams.get("scope") === "all");
  const kind = $derived($page.url.searchParams.get("kind") ?? "");

  let results = $state<SearchResults | null>(null);
  let loading = $state(false);
  let error = $state<unknown>(null);
  let preview = $state<number | null>(null);

  // One request per distinct query, driven by the URL rather than by the input,
  // so a shared link and a typed search take exactly the same path.
  $effect(() => {
    const query = q.trim();
    if (!query) {
      results = null;
      return;
    }
    const params = {
      q: query,
      kind: kind || undefined,
      // "Everything" means everything: terminal rows AND archived ones. Two
      // switches where the user asked one question is how a search that says
      // "all" quietly keeps hiding things.
      scope: everything ? "all" : undefined,
      archived: everything ? "all" : undefined,
    };
    loading = true;
    error = null;
    api
      .search(params)
      .then((r) => (results = r))
      .catch((e) => (error = e))
      .finally(() => (loading = false));
  });

  function navigate(next: Record<string, string | null>) {
    const url = new URL($page.url);
    for (const [k, v] of Object.entries(next)) {
      if (v === null || v === "") url.searchParams.delete(k);
      else url.searchParams.set(k, v);
    }
    goto(`${url.pathname}${url.search}`, { keepFocus: true, noScroll: true });
  }

  const KINDS = [
    ["", "All kinds"],
    ["workitem", "Work items"],
    ["comment", "Comments"],
    ["sprint_proposal", "Proposals"],
    ["program", "Programs"],
    ["report", "Reports"],
    ["handoff", "Handoffs"],
    ["card", "Cards"],
    ["schedule", "Schedules"],
    ["link", "Links"],
  ];

  const hidden = $derived(
    results ? results.omitted.terminal + results.omitted.archived : 0,
  );
</script>

<svelte:head><title>Search · korg</title></svelte:head>

<h1 class="mb-4 text-xl font-semibold">Search</h1>

<div class="mb-4 flex flex-wrap items-center gap-3 text-sm">
  <label class="sr-only" for="search-kind">Filter by kind</label>
  <select
    id="search-kind"
    class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1"
    value={kind}
    onchange={(e) => navigate({ kind: e.currentTarget.value })}
  >
    {#each KINDS as [value, label] (value)}
      <option {value}>{label}</option>
    {/each}
  </select>

  <label class="flex items-center gap-2">
    <input
      type="checkbox"
      checked={everything}
      onchange={(e) => navigate({ scope: e.currentTarget.checked ? "all" : null })}
    />
    <span>Search everything</span>
  </label>
  <!-- Named for what it does, not for what it hides. The default is tuned for
       "what is in flight"; most questions about a decision are questions about
       something already closed, so this switch has to be obvious. -->
  <span class="text-[var(--color-muted)]">
    closed, declined and archived rows too
  </span>
</div>

{#if error}
  <ErrorNotice what="search" {error} />
{:else if !q.trim()}
  <p class="text-[var(--color-muted)]">
    Type in the box above. All terms are required; if that finds nothing the
    search relaxes to any term and says so. <code>"quoted phrases"</code> and
    <code>-excluded</code> work too.
  </p>
{:else if loading && !results}
  <p class="text-[var(--color-muted)]">Searching…</p>
{:else if results}
  <p class="mb-3 text-sm text-[var(--color-muted)]">
    {results.total}
    {results.total === 1 ? "result" : "results"}
    {#if results.relaxed}
      <!-- The failure this feature was built out of was a query path that
           returned nothing while reporting health. A relaxed answer is a
           genuinely different answer and the page says so rather than letting
           a broad result set pass for a precise one. -->
      · <strong class="text-[var(--color-accent)]">any-term</strong>
      — no result had every word, so these match at least one
    {/if}
    {#if hidden > 0 && !everything}
      · {hidden} hidden by scope
      <button
        class="underline"
        onclick={() => navigate({ scope: "all" })}>show everything</button
      >
    {/if}
  </p>

  {#if results.items.length === 0}
    <p class="text-[var(--color-muted)]">
      Nothing matched. This is a lexical search — it matches words, not meaning,
      so try the words the record itself would use.
    </p>
  {/if}

  <ul class="space-y-3">
    {#each results.items as hit (hit.locator)}
      {@const href = nodeHref(
        hit.kind,
        hit.node_id,
        hit.kind === "workitem" ? hit.node_id : null,
      )}
      <li
        class="rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3"
      >
        <div class="flex flex-wrap items-baseline gap-2">
          <button
            class="font-mono text-sm text-[var(--color-accent)] hover:underline"
            onclick={() => (preview = hit.node_id)}>{hit.locator}</button
          >
          <span class="text-sm font-medium">
            {hit.title ?? "comment"}
          </span>
          <span class="text-xs text-[var(--color-muted)]">
            {hit.kind}{hit.status ? ` · ${hit.status}` : ""}{hit.project
              ? ` · ${hit.project}`
              : ""}
          </span>
          <span class="ml-auto text-xs text-[var(--color-muted)]"
            >{stamp(hit.updated)}</span
          >
        </div>
        <p class="mt-1 text-sm text-[var(--color-muted)]">{hit.snippet}</p>
        {#if href}
          <a class="text-xs underline" {href}>open in context</a>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

{#if preview !== null}
  <NodePreview nodeId={preview} onClose={() => (preview = null)} />
{/if}
